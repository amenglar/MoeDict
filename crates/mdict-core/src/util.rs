use std::path::{Component, Path};

use ripemd::{Digest, Ripemd128, Ripemd256};

/// Adler-32, as used by the MDict container for header/chunk checksums.
pub fn adler32(buf: &[u8]) -> u32 {
    const MOD: u32 = 65521;
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for chunk in buf.chunks(5552) {
        for &byte in chunk {
            a += byte as u32;
            b += a;
        }
        a %= MOD;
        b %= MOD;
    }
    (b << 16) | a
}

/// RIPEMD-128 over the concatenation of `parts`.
pub(crate) fn ripemd128(parts: &[&[u8]]) -> [u8; 16] {
    let mut hasher = Ripemd128::new();
    for part in parts {
        hasher.update(part);
    }
    let out = hasher.finalize();
    let mut key = [0u8; 16];
    key.copy_from_slice(&out);
    key
}

/// MDict's "fast decrypt" (a byte-rotation XOR cipher).
pub(crate) fn fast_decrypt(encrypted: &[u8], key: &[u8]) -> Vec<u8> {
    let mut buf = encrypted.to_vec();
    let mut prev: u8 = 0x36;
    if key.is_empty() {
        return buf;
    }
    for (i, byte) in buf.iter_mut().enumerate() {
        let t = byte.rotate_left(4) ^ prev ^ (i as u8) ^ key[i % key.len()];
        prev = *byte;
        *byte = t;
    }
    buf
}

/// A stable, filesystem-safe identifier for a dictionary path.
pub fn hash_id(value: &str) -> String {
    let mut hasher = Ripemd256::new();
    hasher.update(value.as_bytes());
    hex(&hasher.finalize())
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xf) as usize] as char);
    }
    out
}

pub(crate) fn unix_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Strip URL/file prefixes and normalise separators, preserving case.
pub fn clean_resource_path(path: &str) -> String {
    let decoded = percent_decode(path);
    let mut value = decoded
        .trim_matches(|c: char| c.is_control() || c.is_whitespace())
        .replace('\\', "/");
    if let Some(rest) = value.strip_prefix("file:///") {
        value = rest.to_string();
    }
    while let Some(rest) = value.strip_prefix("./") {
        value = rest.to_string();
    }
    while let Some(rest) = value.strip_prefix('/') {
        value = rest.to_string();
    }
    value
}

/// Normalise an MDD resource path for lookup: forward slashes, no leading
/// slash, lower-case, URL-decoded.
pub fn normalize_resource_path(path: &str) -> String {
    clean_resource_path(path).to_lowercase()
}

/// Decode `%XX` escapes without treating `+` as a space.
pub fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn mime_type(path: &str) -> &'static str {
    let ext = path
        .rsplit_once('.')
        .map(|(_, ext)| ext)
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "svg" => "image/svg+xml",
        "css" => "text/css; charset=utf-8",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "eot" => "application/vnd.ms-fontobject",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" | "oga" => "audio/ogg",
        "m4a" => "audio/mp4",
        "aac" => "audio/aac",
        "flac" => "audio/flac",
        "spx" => "audio/ogg",
        "pdf" => "application/pdf",
        "json" => "application/json",
        "txt" => "text/plain; charset=utf-8",
        "html" | "htm" => "text/html; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Returns true when `path` is inside `base`, comparing components and being
/// case-insensitive on Windows.
pub(crate) fn path_is_within(base: &Path, path: &Path) -> bool {
    let base = components(base);
    let path = components(path);
    !base.is_empty() && path.len() >= base.len() && path[..base.len()] == base[..]
}

fn components(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().to_lowercase()),
            Component::Prefix(p) => Some(p.as_os_str().to_string_lossy().to_lowercase()),
            Component::RootDir => Some("/".to_string()),
            Component::CurDir => None,
            Component::ParentDir => Some("..".to_string()),
        })
        .collect()
}

/// Nicely printable path (strips the Windows `\\?\` verbatim prefix).
pub fn display_path(path: &Path) -> String {
    let s = path.to_string_lossy();
    #[cfg(windows)]
    {
        if let Some(rest) = s.strip_prefix(r"\\?\") {
            return rest.to_string();
        }
    }
    s.into_owned()
}
