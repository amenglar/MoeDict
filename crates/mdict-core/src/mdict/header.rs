use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::error::{Error, Result};
use crate::mdict::cursor::Cursor;
use crate::util::adler32;

const MAX_HEADER_LEN: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone)]
pub(crate) struct ParsedHeader {
    pub version: u8,
    pub encrypted: String,
    pub encoding: String,
    pub title: Option<String>,
    /// Number of bytes consumed by length + XML + checksum.
    pub header_len: usize,
}

pub(crate) fn parse_header(data: &[u8]) -> Result<ParsedHeader> {
    let mut cursor = Cursor::new(data);
    let header_len = cursor.u32_be()? as usize;
    if header_len == 0 || header_len > MAX_HEADER_LEN {
        return Err(Error::format(format!(
            "invalid header length: {header_len}"
        )));
    }
    let xml_bytes = cursor.take(header_len)?;
    let checksum = cursor.u32_le()?;
    let actual = adler32(xml_bytes);
    if actual != checksum {
        return Err(Error::format(format!(
            "header checksum mismatch: expected {checksum:#010x}, got {actual:#010x}"
        )));
    }

    let xml = String::from_utf16_lossy(
        &xml_bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect::<Vec<_>>(),
    );

    let version_text = attr(&xml, "GeneratedByEngineVersion")
        .ok_or_else(|| Error::format("missing GeneratedByEngineVersion"))?;
    let version = version_text
        .trim()
        .chars()
        .next()
        .and_then(|c| c.to_digit(10))
        .ok_or_else(|| Error::format(format!("invalid engine version {version_text:?}")))?
        as u8;
    if version != 1 && version != 2 {
        return Err(Error::unsupported(format!(
            "MDict engine version {version_text}"
        )));
    }

    let encrypted = attr(&xml, "Encrypted").unwrap_or("0").to_string();
    let encoding = attr(&xml, "Encoding").unwrap_or("").to_string();
    let title = attr(&xml, "Title")
        .map(unescape_xml)
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty() && t != "Title (No HTML code allowed)")
        .map(|t| truncate_chars(&t, 120));

    Ok(ParsedHeader {
        version,
        encrypted,
        encoding,
        title,
        header_len: cursor.pos(),
    })
}

/// Read only the header of a dictionary and return its `Title` attribute.
/// This does not memory-map the whole file, so it is cheap enough to call
/// while adding dictionaries.
pub fn read_title(path: &Path) -> Result<Option<String>> {
    let mut file = File::open(path)?;
    let mut len_buf = [0u8; 4];
    file.read_exact(&mut len_buf)?;
    let header_len = u32::from_be_bytes(len_buf) as usize;
    if header_len == 0 || header_len > MAX_HEADER_LEN {
        return Err(Error::format(format!(
            "invalid header length in {}: {header_len}",
            path.display()
        )));
    }
    let total = 4 + header_len + 4;
    let mut buf = vec![0u8; total];
    buf[..4].copy_from_slice(&len_buf);
    file.read_exact(&mut buf[4..])?;
    Ok(parse_header(&buf)?.title)
}

fn attr<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("{name}=\"");
    let start = xml.find(&needle)? + needle.len();
    let rest = &xml[start..];
    let end = rest.find('"')?;
    Some(&rest[..end])
}

fn unescape_xml(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

fn truncate_chars(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    let mut out: String = value.chars().take(max).collect();
    out.push('…');
    out
}
