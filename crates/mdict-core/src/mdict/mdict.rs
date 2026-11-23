use std::fs::File;
use std::io::Read;
use std::ops::Deref;
use std::path::Path;

use encoding::DecoderTrap;
use encoding::label::encoding_from_whatwg_label;
use flate2::read::ZlibDecoder;
use memmap2::Mmap;

use crate::error::{Error, Result};
use crate::mdict::cursor::Cursor;
use crate::mdict::header::parse_header;
use crate::util::{adler32, fast_decrypt, ripemd128};

const MAX_CHUNK_DSIZE: u64 = 512 * 1024 * 1024;
const MAX_KEY_LEN: usize = 1024 * 1024;

enum Data {
    Mapped(Mmap),
    Owned(Vec<u8>),
}

impl Deref for Data {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        match self {
            Data::Mapped(map) => map,
            Data::Owned(vec) => vec,
        }
    }
}

#[derive(Debug, Clone)]
pub struct KeyEntry {
    pub text: String,
    /// Offset into the concatenation of all decompressed record blocks.
    pub offset: u64,
}

#[derive(Debug, Clone, Copy)]
struct BlockMeta {
    csize: usize,
    dsize: usize,
}

struct KeyBlockHeader {
    block_count: usize,
    entry_count: usize,
    info_decompressed_len: usize,
    info_len: usize,
    blocks_len: usize,
}

enum KeyEncoding {
    Utf16,
    Label(String),
}

/// A parsed MDX/MDD file.
///
/// Construction only decompresses the key blocks and reads the record-block
/// size table; record blocks are decompressed one at a time on demand.  The
/// whole file is memory-mapped, not copied.
pub struct Mdict {
    data: Data,
    entries: Vec<KeyEntry>,
    blocks: Vec<BlockMeta>,
    block_starts: Vec<u64>,
    block_cstarts: Vec<usize>,
    record_area: usize,
    total_dsize: u64,
    encoding: String,
    title: Option<String>,
    is_mdd: bool,
}

impl Mdict {
    pub fn open(path: &Path) -> Result<Self> {
        let is_mdd = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("mdd"))
            .unwrap_or(false);
        let file = File::open(path)?;
        let metadata = file.metadata()?;
        if metadata.len() == 0 {
            return Err(Error::format(format!("文件为空: {}", path.display())));
        }
        // SAFETY: the caller must not modify the file while the dictionary is
        // open.  We additionally snapshot size/mtime before indexing and
        // reject the result if the source changed.
        let map = unsafe { Mmap::map(&file)? };
        Self::parse(Data::Mapped(map), is_mdd)
    }

    pub fn from_bytes(data: Vec<u8>, is_mdd: bool) -> Result<Self> {
        Self::parse(Data::Owned(data), is_mdd)
    }

    fn parse(data: Data, is_mdd: bool) -> Result<Self> {
        let header = parse_header(&data)?;

        let encoding = header.encoding.trim().to_string();
        let key_encoding = if is_mdd || is_utf16_label(&encoding) {
            KeyEncoding::Utf16
        } else if encoding.is_empty() {
            KeyEncoding::Label("UTF-8".to_string())
        } else {
            KeyEncoding::Label(encoding.clone())
        };

        let mut cursor = Cursor::new(&data);
        cursor.set_pos(header.header_len);

        let key_is_utf16 = matches!(key_encoding, KeyEncoding::Utf16);
        let key_header = parse_key_block_header(&mut cursor, header.version)?;
        let info_bytes = cursor.take(key_header.info_len)?;
        let block_sizes = parse_key_block_sizes(
            info_bytes,
            header.version,
            &header.encrypted,
            key_header.info_decompressed_len,
            key_header.block_count,
            key_is_utf16,
        )?;
        let key_blocks_bytes = cursor.take(key_header.blocks_len)?;
        let mut entries = parse_key_blocks(
            key_blocks_bytes,
            &block_sizes,
            header.version,
            &key_encoding,
        )?;
        entries.sort_by_key(|e| e.offset);

        let (blocks, record_area) = parse_record_block_table(&mut cursor, header.version)?;

        let total_csize: u64 = blocks.iter().map(|b| b.csize as u64).sum();
        let total_dsize: u64 = blocks.iter().map(|b| b.dsize as u64).sum();
        if record_area as u64 + total_csize > data.len() as u64 {
            return Err(Error::format(format!(
                "数据区越界: record_area={record_area}, csize={total_csize}, file={}",
                data.len()
            )));
        }
        for entry in &entries {
            if entry.offset > total_dsize {
                return Err(Error::format(format!(
                    "词条 {} 的偏移 {} 超出记录区大小 {}",
                    entry.text, entry.offset, total_dsize
                )));
            }
        }

        let mut block_starts = Vec::with_capacity(blocks.len() + 1);
        let mut block_cstarts = Vec::with_capacity(blocks.len());
        let mut dacc = 0u64;
        let mut cacc = 0usize;
        for block in &blocks {
            block_starts.push(dacc);
            block_cstarts.push(cacc);
            dacc += block.dsize as u64;
            cacc += block.csize;
        }
        block_starts.push(dacc);

        if entries.len() != key_header.entry_count {
            log::warn!(
                "entry count mismatch: parsed {}, header says {}",
                entries.len(),
                key_header.entry_count
            );
        }

        Ok(Self {
            data,
            entries,
            blocks,
            block_starts,
            block_cstarts,
            record_area,
            total_dsize,
            encoding,
            title: header.title,
            is_mdd,
        })
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    pub fn entries(&self) -> &[KeyEntry] {
        &self.entries
    }

    pub fn encoding(&self) -> &str {
        &self.encoding
    }

    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub fn is_mdd(&self) -> bool {
        self.is_mdd
    }

    pub fn total_decompressed_size(&self) -> u64 {
        self.total_dsize
    }

    /// Iterate every entry, decompressing each record block exactly once.
    ///
    /// The definition bytes are UTF-8 for MDX and raw bytes for MDD.
    pub fn for_each_entry<F>(&self, mut f: F) -> Result<usize>
    where
        F: FnMut(&str, &[u8]) -> Result<()>,
    {
        let mut index = 0usize;
        for (block_index, block) in self.blocks.iter().enumerate() {
            if index >= self.entries.len() {
                break;
            }
            let block_start = self.block_starts[block_index];
            let block_end = block_start + block.dsize as u64;
            if self.entries[index].offset >= block_end {
                continue;
            }

            let data = self.decompress_record_block(block_index)?;
            while index < self.entries.len() {
                let entry = &self.entries[index];
                if entry.offset >= block_end {
                    break;
                }
                let start = entry.offset.saturating_sub(block_start) as usize;
                let end = if index + 1 < self.entries.len() {
                    let next = self.entries[index + 1].offset;
                    if next <= block_end {
                        next.saturating_sub(block_start) as usize
                    } else {
                        data.len()
                    }
                } else {
                    data.len()
                };
                let start = start.min(data.len());
                let end = end.min(data.len()).max(start);
                let bytes = &data[start..end];

                if self.is_mdd {
                    f(&entry.text, bytes)?;
                } else {
                    let decoded = decode_with_label(&self.encoding, bytes);
                    f(&entry.text, decoded.as_bytes())?;
                }
                index += 1;
            }
        }

        if index != self.entries.len() {
            return Err(Error::format(format!(
                "只解析到 {index}/{} 个词条",
                self.entries.len()
            )));
        }
        Ok(index)
    }

    /// Read a range of the concatenated decompressed record area.  Used by
    /// the MDD resource reader.
    pub fn read_range(&self, start: u64, end: u64) -> Result<Vec<u8>> {
        let end = end.min(self.total_dsize);
        if start >= end {
            return Ok(Vec::new());
        }
        let mut out = Vec::with_capacity((end - start).min(16 * 1024 * 1024) as usize);
        let mut cursor = start;
        while cursor < end {
            let block_index = self.find_block(cursor)?;
            let block_start = self.block_starts[block_index];
            let block_end = block_start + self.blocks[block_index].dsize as u64;
            let data = self.decompress_record_block(block_index)?;
            if block_end <= cursor {
                return Err(Error::format("record block offset did not advance"));
            }
            let slice_end = end.min(block_end);
            let s = cursor.saturating_sub(block_start) as usize;
            let e = slice_end.saturating_sub(block_start) as usize;
            let e = e.min(data.len());
            if s < e {
                out.extend_from_slice(&data[s..e]);
            }
            cursor = slice_end;
        }
        Ok(out)
    }

    /// End offset (in the concatenated decompressed record area) of the block
    /// containing `offset`.
    pub fn block_end_for(&self, offset: u64) -> Result<u64> {
        let index = self.find_block(offset)?;
        Ok(self.block_starts[index] + self.blocks[index].dsize as u64)
    }

    fn find_block(&self, offset: u64) -> Result<usize> {
        if self.blocks.is_empty() {
            return Err(Error::format("dictionary has no record blocks"));
        }
        let index = self
            .block_starts
            .partition_point(|&start| start <= offset)
            .saturating_sub(1);
        if index >= self.blocks.len() {
            return Err(Error::format(format!("offset {offset} out of range")));
        }
        Ok(index)
    }

    fn decompress_record_block(&self, index: usize) -> Result<Vec<u8>> {
        let block = self
            .blocks
            .get(index)
            .ok_or_else(|| Error::format("record block index out of range"))?;
        let cstart = self.record_area + self.block_cstarts[index];
        let cend = cstart + block.csize;
        if cend > self.data.len() {
            return Err(Error::format("record block compressed range out of bounds"));
        }
        decompress_chunk(&self.data[cstart..cend], block.dsize, "record block")
    }
}

fn parse_key_block_header(cursor: &mut Cursor<'_>, version: u8) -> Result<KeyBlockHeader> {
    if version == 1 {
        let raw = cursor.take(16)?;
        let block_count = u32::from_be_bytes(raw[0..4].try_into().unwrap()) as usize;
        let entry_count = u32::from_be_bytes(raw[4..8].try_into().unwrap()) as usize;
        let info_len = u32::from_be_bytes(raw[8..12].try_into().unwrap()) as usize;
        let blocks_len = u32::from_be_bytes(raw[12..16].try_into().unwrap()) as usize;
        return Ok(KeyBlockHeader {
            block_count,
            entry_count,
            info_decompressed_len: info_len,
            info_len,
            blocks_len,
        });
    }

    let raw = cursor.take(40)?;
    let checksum = cursor.u32_be()?;
    let actual = adler32(raw);
    if actual != checksum {
        return Err(Error::format(format!(
            "key block header checksum mismatch: expected {checksum:#010x}, got {actual:#010x}"
        )));
    }
    let block_count = read_u64(raw, 0)?;
    let entry_count = read_u64(raw, 8)?;
    let info_decompressed_len = read_u64(raw, 16)?;
    let info_len = read_u64(raw, 24)?;
    let blocks_len = read_u64(raw, 32)?;
    Ok(KeyBlockHeader {
        block_count: to_usize(block_count, "block count")?,
        entry_count: to_usize(entry_count, "entry count")?,
        info_decompressed_len: to_usize(info_decompressed_len, "key info size")?,
        info_len: to_usize(info_len, "key info compressed size")?,
        blocks_len: to_usize(blocks_len, "key blocks size")?,
    })
}

fn parse_key_block_sizes(
    info: &[u8],
    version: u8,
    encrypted: &str,
    expected_len: usize,
    block_count: usize,
    utf16: bool,
) -> Result<Vec<BlockMeta>> {
    let decoded = if version == 1 {
        info.to_vec()
    } else {
        decrypt_key_info(info, encrypted, expected_len)?
    };

    let mut cursor = Cursor::new(&decoded);
    let mut sizes = Vec::with_capacity(block_count.max(1));
    while cursor.remaining() > 0 {
        if cursor.is_zero_tail() {
            break;
        }
        let block = if version == 1 {
            if cursor.remaining() < 16 {
                break;
            }
            let _entries = cursor.u32_be()?;
            skip_v1_text(&mut cursor, utf16)?;
            skip_v1_text(&mut cursor, utf16)?;
            let csize = cursor.u32_be()? as usize;
            let dsize = cursor.u32_be()? as usize;
            BlockMeta { csize, dsize }
        } else {
            if cursor.remaining() < 30 {
                break;
            }
            let _entries = cursor.u64_be()?;
            skip_v2_text(&mut cursor, utf16)?;
            skip_v2_text(&mut cursor, utf16)?;
            let csize = to_usize(cursor.u64_be()?, "block csize")?;
            let dsize = to_usize(cursor.u64_be()?, "block dsize")?;
            BlockMeta { csize, dsize }
        };
        validate_block_size(block)?;
        sizes.push(block);
        if sizes.len() > 10_000_000 {
            return Err(Error::format("too many key blocks"));
        }
    }

    if block_count != sizes.len() {
        log::warn!(
            "key block count mismatch: parsed {}, header says {}",
            sizes.len(),
            block_count
        );
    }
    Ok(sizes)
}

fn decrypt_key_info(info: &[u8], encrypted: &str, expected_len: usize) -> Result<Vec<u8>> {
    if info.len() < 8 {
        return Err(Error::format("key block info is too short"));
    }
    let cap = expected_len.max(1).min(MAX_CHUNK_DSIZE as usize);
    let decrypted: Vec<u8>;
    let payload: &[u8] = match encrypted {
        "0" | "1" => &info[8..],
        "2" | "3" => {
            let key = ripemd128(&[&info[4..8], &0x3695u32.to_le_bytes()]);
            decrypted = fast_decrypt(&info[8..], &key);
            &decrypted
        }
        other => {
            return Err(Error::unsupported(format!(
                "key block encryption type {other}"
            )));
        }
    };
    zlib_decode(payload, cap, "key block info")
}

fn parse_key_blocks(
    mut buf: &[u8],
    sizes: &[BlockMeta],
    version: u8,
    encoding: &KeyEncoding,
) -> Result<Vec<KeyEntry>> {
    let mut entries = Vec::new();
    for (index, size) in sizes.iter().enumerate() {
        if buf.len() < size.csize {
            return Err(Error::format(format!(
                "key block {index} is truncated: need {}, have {}",
                size.csize,
                buf.len()
            )));
        }
        let (chunk, rest) = buf.split_at(size.csize);
        let block = decompress_chunk(chunk, size.dsize, "key block")?;
        parse_key_block(&block, version, encoding, &mut entries)?;
        buf = rest;
    }
    Ok(entries)
}

fn parse_key_block(
    block: &[u8],
    version: u8,
    encoding: &KeyEncoding,
    out: &mut Vec<KeyEntry>,
) -> Result<()> {
    let mut cursor = Cursor::new(block);
    while cursor.remaining() >= 8 {
        if cursor.remaining() < 30 && cursor.is_zero_tail() {
            break;
        }
        let offset = if version == 1 {
            cursor.u32_be()? as u64
        } else {
            cursor.u64_be()?
        };
        let text = match encoding {
            KeyEncoding::Utf16 => read_utf16_key(&mut cursor)?,
            KeyEncoding::Label(label) => {
                let raw = cursor.take_until(0)?;
                cursor.skip(1)?;
                if raw.len() > MAX_KEY_LEN {
                    return Err(Error::format("key is too long"));
                }
                decode_with_label(label, raw)
            }
        };
        if text.is_empty() && offset == 0 {
            // Trailing zero padding.
            break;
        }
        out.push(KeyEntry { text, offset });
    }
    Ok(())
}

fn read_utf16_key(cursor: &mut Cursor<'_>) -> Result<String> {
    let mut units = Vec::new();
    loop {
        if cursor.remaining() < 2 {
            return Err(Error::format("truncated UTF-16 key"));
        }
        let unit = cursor.u16_le()?;
        if unit == 0 {
            break;
        }
        units.push(unit);
        if units.len() > MAX_KEY_LEN / 2 {
            return Err(Error::format("key is too long"));
        }
    }
    Ok(String::from_utf16_lossy(&units))
}

/// V1 stores the text length including its terminator; the legacy parser read
/// exactly `len` bytes.  UTF-16 v1 dictionaries are rare and best-effort.
fn skip_v1_text(cursor: &mut Cursor<'_>, utf16: bool) -> Result<()> {
    let len = cursor.u8()? as usize;
    let bytes = if utf16 { len * 2 } else { len };
    cursor.skip(bytes)
}

fn skip_v2_text(cursor: &mut Cursor<'_>, utf16: bool) -> Result<()> {
    let len = cursor.u16_be()? as usize;
    let bytes = if utf16 { len * 2 + 2 } else { len + 1 };
    cursor.skip(bytes)
}

fn parse_record_block_table(
    cursor: &mut Cursor<'_>,
    version: u8,
) -> Result<(Vec<BlockMeta>, usize)> {
    let mut blocks = Vec::new();
    if version == 1 {
        let records = cursor.u32_be()? as usize;
        let _entries = cursor.u32_be()?;
        let info_len = cursor.u32_be()? as usize;
        let _buf_len = cursor.u32_be()? as usize;
        if records != 0 && records.saturating_mul(8) != info_len {
            return Err(Error::format(format!(
                "record info size mismatch: {records} blocks, {info_len} bytes"
            )));
        }
        for _ in 0..records {
            let csize = cursor.u32_be()? as usize;
            let dsize = cursor.u32_be()? as usize;
            let block = BlockMeta { csize, dsize };
            validate_block_size(block)?;
            blocks.push(block);
        }
    } else {
        let records = to_usize(cursor.u64_be()?, "record block count")?;
        let _entries = cursor.u64_be()?;
        let info_len = to_usize(cursor.u64_be()?, "record info size")?;
        let _buf_len = cursor.u64_be()?;
        if records != 0 && records.saturating_mul(16) != info_len {
            return Err(Error::format(format!(
                "record info size mismatch: {records} blocks, {info_len} bytes"
            )));
        }
        for _ in 0..records {
            let csize = to_usize(cursor.u64_be()?, "record block csize")?;
            let dsize = to_usize(cursor.u64_be()?, "record block dsize")?;
            let block = BlockMeta { csize, dsize };
            validate_block_size(block)?;
            blocks.push(block);
        }
    }
    Ok((blocks, cursor.pos()))
}

fn validate_block_size(block: BlockMeta) -> Result<()> {
    if block.csize < 8 {
        return Err(Error::format(format!(
            "invalid compressed block size {}",
            block.csize
        )));
    }
    if block.dsize as u64 > MAX_CHUNK_DSIZE {
        return Err(Error::format(format!(
            "decompressed block too large: {}",
            block.dsize
        )));
    }
    Ok(())
}

fn decompress_chunk(chunk: &[u8], dsize: usize, what: &str) -> Result<Vec<u8>> {
    if chunk.len() < 8 {
        return Err(Error::format(format!("{what} is shorter than its header")));
    }
    let flags = u32::from_le_bytes(chunk[0..4].try_into().unwrap());
    let checksum = &chunk[4..8];
    let payload = &chunk[8..];
    let enc_method = (flags >> 4) & 0xf;
    let comp_method = flags & 0xf;

    let key = ripemd128(&[checksum]);
    let data = match enc_method {
        0 => payload.to_vec(),
        1 => fast_decrypt(payload, &key),
        other => {
            return Err(Error::unsupported(format!(
                "{what}: unknown encryption method {other}"
            )));
        }
    };

    match comp_method {
        0 => Ok(data),
        1 => {
            let lzo = minilzo_rs::LZO::init()
                .map_err(|_| Error::format(format!("{what}: cannot initialise LZO")))?;
            lzo.decompress(&data, dsize)
                .map_err(|_| Error::format(format!("{what}: LZO decompression failed")))
        }
        2 => zlib_decode(&data, dsize, what),
        other => Err(Error::unsupported(format!(
            "{what}: unknown compression method {other}"
        ))),
    }
}

fn zlib_decode(data: &[u8], cap: usize, what: &str) -> Result<Vec<u8>> {
    if cap == 0 {
        return Err(Error::format(format!("{what}: invalid decompressed size")));
    }
    let limit = (cap as u64).saturating_add(1);
    let mut decoder = ZlibDecoder::new(data).take(limit);
    let mut out = Vec::with_capacity(cap.min(16 * 1024 * 1024));
    decoder
        .read_to_end(&mut out)
        .map_err(|e| Error::format(format!("{what}: zlib decompression failed: {e}")))?;
    if out.len() > cap {
        return Err(Error::format(format!(
            "{what}: decompressed data exceeds declared size {cap}"
        )));
    }
    Ok(out)
}

fn decode_with_label(label: &str, bytes: &[u8]) -> String {
    let trimmed = label.trim();
    if trimmed.is_empty()
        || trimmed.eq_ignore_ascii_case("utf-8")
        || trimmed.eq_ignore_ascii_case("utf8")
    {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    if is_utf16_label(trimmed) {
        return decode_utf16le(bytes);
    }
    match encoding_from_whatwg_label(trimmed) {
        Some(decoder) => decoder
            .decode(bytes, DecoderTrap::Replace)
            .unwrap_or_else(|_| String::from_utf8_lossy(bytes).into_owned()),
        None => String::from_utf8_lossy(bytes).into_owned(),
    }
}

fn is_utf16_label(label: &str) -> bool {
    let label = label.trim().to_ascii_lowercase();
    label == "utf-16" || label == "utf-16le" || label == "utf-16be" || label == "utf16"
}

fn decode_utf16le(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&units)
}

fn to_usize(value: u64, what: &str) -> Result<usize> {
    usize::try_from(value).map_err(|_| Error::format(format!("{what} does not fit in memory")))
}

fn read_u64(buf: &[u8], offset: usize) -> Result<u64> {
    let bytes = buf
        .get(offset..offset + 8)
        .ok_or_else(|| Error::format("truncated 64-bit field"))?;
    Ok(u64::from_be_bytes(bytes.try_into().unwrap()))
}
