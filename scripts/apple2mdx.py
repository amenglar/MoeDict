import argparse
import datetime
import html
import os
import re
import struct
import sys
import tempfile
import traceback
import zipfile
import zlib
from pathlib import Path
from urllib.parse import quote

from lxml import etree
from pyglossary.glossary_v2 import Glossary

_orig_fromstring = etree.fromstring


def tolerant_fromstring(data, *args, **kwargs):
    try:
        return _orig_fromstring(data, *args, **kwargs)
    except etree.XMLSyntaxError:
        parser = etree.XMLParser(recover=True, huge_tree=True, no_network=True)
        return _orig_fromstring(data, parser)


etree.fromstring = tolerant_fromstring


class ParameterError(Exception):
    pass


def mdx_compress(data, compression_type=2):
    header = struct.pack("<L", compression_type) + struct.pack(">L", zlib.adler32(data) & 0xFFFFFFFF)
    if compression_type == 0:
        return header + data
    if compression_type == 2:
        return header + zlib.compress(data, 6)
    raise ParameterError("unsupported compression type")


class OffsetEntry:
    def __init__(self, key, key_null, key_len, offset, record):
        self.key = key
        self.key_null = key_null
        self.key_len = key_len
        self.offset = offset
        self.record = record


class MdxWriter:
    def __init__(self, items, title, description, block_size=1 << 20, is_mdd=False):
        self.title = title
        self.description = description
        self.block_size = block_size
        self.is_mdd = is_mdd
        self.encoding = "UTF-16" if is_mdd else "UTF-8"
        self.python_encoding = "utf_16_le" if is_mdd else "utf_8"
        self.encoding_length = 2 if is_mdd else 1
        self.version = "2.0"
        self.compression_type = 2
        ordered = sorted(items.items(), key=lambda pair: pair[0])
        self.entries = []
        offset = 0
        for key, record in ordered:
            key_bytes = key.encode(self.python_encoding)
            null = b"\x00\x00" if is_mdd else b"\x00"
            key_null = key_bytes + null
            key_len = len(key_bytes) // self.encoding_length
            record_bytes = record if is_mdd else (record + "\x00").encode(self.python_encoding)
            self.entries.append(OffsetEntry(key_bytes, key_null, key_len, offset, record_bytes))
            offset += len(record_bytes)
        self.num_entries = len(self.entries)
        self.total_record_len = offset
        self.key_blocks = self.split_blocks(True)
        self.record_blocks = self.split_blocks(False)
        self.key_index = b"".join(block.index_entry(True) for block in self.key_blocks)
        self.key_index_comp = mdx_compress(self.key_index, self.compression_type)
        self.record_index = b"".join(block.index_entry(False) for block in self.record_blocks)

    def split_blocks(self, is_key):
        blocks = []
        start = 0
        size = 0
        for i in range(len(self.entries) + 1):
            entry = self.entries[i] if i < len(self.entries) else None
            length = (8 + len(entry.key_null)) if (is_key and entry) else (len(entry.record) if entry else 0)
            flush = i == len(self.entries) or (i > 0 and size + length > self.block_size)
            if flush:
                blocks.append(MdxBlock(self.entries[start:i], is_key, self.compression_type, self.version))
                start = i
                size = 0
            if entry:
                size += length
        return blocks

    def write(self, outfile):
        self.write_header(outfile)
        self.write_key_section(outfile)
        self.write_record_section(outfile)

    def write_header(self, outfile):
        today = datetime.date.today()
        attrs = [
            ('GeneratedByEngineVersion', self.version),
            ('RequiredEngineVersion', self.version),
            ('Encrypted', '0'),
            ('Encoding', self.encoding),
            ('Format', 'Html'),
            ('CreationDate', f'{today.year}-{today.month}-{today.day}'),
            ('Compact', 'No'),
            ('Compat', 'No'),
            ('KeyCaseSensitive', 'No'),
            ('Description', html.escape(self.description, quote=True)),
            ('Title', html.escape(self.title, quote=True)),
            ('DataSourceFormat', '106'),
            ('StyleSheet', ''),
            ('RegisterBy', ''),
            ('RegCode', ''),
        ]
        tag = 'Library_Data' if self.is_mdd else 'Dictionary'
        header = ('<' + tag + ' ' + ' '.join(f'{k}="{v}"' for k, v in attrs) + '/>\r\n\x00').encode('utf_16_le')
        outfile.write(struct.pack('>L', len(header)))
        outfile.write(header)
        outfile.write(struct.pack('<L', zlib.adler32(header) & 0xFFFFFFFF))

    def write_key_section(self, outfile):
        total = sum(len(block.data) for block in self.key_blocks)
        preamble = struct.pack('>QQQQQ', len(self.key_blocks), self.num_entries,
                               len(self.key_index), len(self.key_index_comp), total)
        outfile.write(preamble)
        outfile.write(struct.pack('>L', zlib.adler32(preamble) & 0xFFFFFFFF))
        outfile.write(self.key_index_comp)
        for block in self.key_blocks:
            outfile.write(block.data)

    def write_record_section(self, outfile):
        total = sum(len(block.data) for block in self.record_blocks)
        outfile.write(struct.pack('>QQQQ', len(self.record_blocks), self.num_entries,
                                  len(self.record_index), total))
        outfile.write(self.record_index)
        for block in self.record_blocks:
            outfile.write(block.data)


class MdxBlock:
    def __init__(self, entries, is_key, compression_type, version):
        if is_key:
            raw = b''.join(struct.pack('>Q', e.offset) + e.key_null for e in entries)
        else:
            raw = b''.join(e.record for e in entries)
        self.raw = raw
        self.data = mdx_compress(raw, compression_type)
        self.is_key = is_key
        self.version = version
        self.num_entries = len(entries)
        if entries:
            self.first_key = entries[0].key_null
            self.last_key = entries[-1].key_null
            self.first_len = entries[0].key_len
            self.last_len = entries[-1].key_len
        else:
            self.first_key = self.last_key = b''
            self.first_len = self.last_len = 0

    def index_entry(self, is_key):
        if is_key:
            return (struct.pack('>Q', self.num_entries)
                    + struct.pack('>H', self.first_len) + self.first_key
                    + struct.pack('>H', self.last_len) + self.last_key
                    + struct.pack('>QQ', len(self.data), len(self.raw)))
        return struct.pack('>QQ', len(self.data), len(self.raw))


def word_of(entry):
    raw = getattr(entry, 'b_term', None)
    if raw is None:
        raw = getattr(entry, 'b_word', None)
    if raw is None:
        raw = entry.word.encode('utf-8')
    return raw.decode('utf-8', 'replace')


def defi_of(entry):
    raw = getattr(entry, 'b_defi', None)
    if raw is None:
        raw = entry.defi.encode('utf-8')
    return raw.decode('utf-8', 'replace')


def convert_bundle(bundle_path, out_dir, display_name=None):
    glos = Glossary()
    glos.directRead(str(bundle_path), formatName='AppleDictBin')
    css = ''
    definitions = {}
    resources = {}
    for entry in glos:
        if entry.isData():
            name = word_of(entry).replace('\\', '/')
            data = entry.data or b''
            resources[name] = data
            if name == 'style.css' or name.lower().endswith('.css'):
                if name == 'style.css':
                    css = data.decode('utf-8', 'replace')
        else:
            word = word_of(entry)
            definitions[word] = defi_of(entry)
    if not definitions:
        raise RuntimeError('no definitions found in ' + str(bundle_path))
    title = display_name or Path(bundle_path).name
    for suffix in ('.dictionary', '.dictionary.codex-backup'):
        if title.endswith(suffix):
            title = title[:-len(suffix)]
    stem = re.sub(r'[^\w\u4e00-\u9fff\- ]+', '_', title).strip() or 'dictionary'
    style_block = '<style>' + css + '</style>' if css else ''

    def rewrite_bword(match):
        target = match.group(1)
        if not target:
            return 'href="#"'
        if target.startswith('#'):
            return 'href="' + target + '"'
        target = re.sub(r':' + re.escape(title) + r'$', '', target)
        return 'href="entry://' + quote(target, safe='') + '"'

    converted = {}
    for word, definition in definitions.items():
        definition = re.sub(
            r'<link[^>]+href=["\']style\.css["\'][^>]*>',
            style_block,
            definition,
            flags=re.I,
        )
        definition = re.sub(
            r'href=["\']bword://([^"\']*)["\']',
            rewrite_bword,
            definition,
            flags=re.I,
        )
        converted[word] = definition
    mdx_path = out_dir / (stem + '.mdx')
    with open(mdx_path, 'wb') as f:
        MdxWriter(converted, title, 'Converted from Apple Dictionary').write(f)
    print(f'wrote {mdx_path} ({len(converted)} entries)')
    if resources:
        mdd_path = out_dir / (stem + '.mdd')
        with open(mdd_path, 'wb') as f:
            MdxWriter(resources, title, 'Resources', is_mdd=True).write(f)
        print(f'wrote {mdd_path} ({len(resources)} resources)')


def bundle_names(zip_path):
    names = []
    with zipfile.ZipFile(zip_path) as zf:
        for info in zf.infolist():
            if info.is_dir():
                continue
            parts = Path(info.filename).parts
            for i, part in enumerate(parts):
                if part.endswith('.dictionary') or part.endswith('.dictionary.codex-backup'):
                    if '.codex-backup' in part:
                        break
                    name = '/'.join(parts[:i + 1])
                    if name not in names:
                        names.append(name)
                    break
    return names


def extract_bundle(zip_path, name, dest):
    prefix = name + '/'
    with zipfile.ZipFile(zip_path) as zf:
        for info in zf.infolist():
            if not info.filename.startswith(prefix):
                continue
            target = dest / Path(info.filename).relative_to(name)
            if info.is_dir():
                target.mkdir(parents=True, exist_ok=True)
                continue
            target.parent.mkdir(parents=True, exist_ok=True)
            with zf.open(info) as src, open(target, 'wb') as dst:
                while True:
                    chunk = src.read(1 << 20)
                    if not chunk:
                        break
                    dst.write(chunk)


def main():
    Glossary.init()
    parser = argparse.ArgumentParser()
    root = Path(__file__).resolve().parent.parent
    parser.add_argument('--zip', type=Path, default=root / 'resources' / '词典.zip')
    parser.add_argument('--out', type=Path, default=root / 'resources' / 'converted')
    parser.add_argument('--bundle', action='append')
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    names = bundle_names(args.zip)
    if args.bundle:
        wanted = set(args.bundle)
        names = [name for name in names if Path(name).name in wanted]
    print(f'bundles: {len(names)}')
    for name in names:
        print('converting', name)
        with tempfile.TemporaryDirectory(prefix='apple2mdx-') as tmp:
            dest = Path(tmp) / 'bundle'
            dest.mkdir()
            extract_bundle(args.zip, name, dest)
            convert_bundle(dest, args.out, Path(name).name)


if __name__ == '__main__':
    try:
        main()
    except SystemExit:
        raise
    except BaseException:
        traceback.print_exc()
        sys.exit(1)
