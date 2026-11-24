// 临时诊断程序：验证现有解析器，并探测 MDD key block 的格式
#[path = "../mdict/mod.rs"]
mod mdict;
#[path = "../util/mod.rs"]
mod util;

use std::io::Read;
use std::time::Instant;

use flate2::read::ZlibDecoder;
use ripemd::{Digest, Ripemd128};

use mdict::mdx::Mdx;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = &args[1];
    let data = std::fs::read(path).unwrap();
    println!("file={} size={} bytes", path, data.len());

    let t0 = Instant::now();
    let mdx = Mdx::new(&data);
    println!("parse took {:?}, entries={}", t0.elapsed(), mdx.records_offset.len());

    // 统计 record block 的重复解压情况
    let mut blocks = std::collections::HashMap::new();
    for r in &mdx.records_offset {
        *blocks.entry(r.block_offset_in_buf).or_insert(0usize) += 1;
    }
    println!("record blocks={}, max entries per block={:?}",
        blocks.len(), blocks.values().max());

    let t1 = Instant::now();
    let mut count = 0usize;
    let mut def_bytes = 0usize;
    for item in mdx.items().take(2000) {
        count += 1;
        def_bytes += item.definition.len();
        if count <= 3 {
            println!("  entry {:?} def_len={}", item.text, item.definition.len());
        }
    }
    println!("first {} items decompressed in {:?} ({} bytes)", count, t1.elapsed(), def_bytes);

    if path.ends_with(".mdd") {
        dump_key_block(&data);
    }
}

// ---- 以下为 MDD key block 探测 ----
fn dump_key_block(data: &[u8]) {
    let (_, hlen) = read_u32_be(data).unwrap();
    let header_xml = String::from_utf16_lossy(
        &data[4..4 + hlen as usize]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect::<Vec<_>>(),
    );
    let encrypted = attr(&header_xml, "Encrypted");
    println!("mdd header encrypted={:?}", encrypted);

    let mut p = 4 + hlen as usize;
    let kb_header = &data[p..p + 40];
    let fields: Vec<u64> = kb_header
        .chunks_exact(8)
        .map(|c| u64::from_be_bytes(c.try_into().unwrap()))
        .collect();
    println!("kb header block_num={} entry_num={} info_dlen={} info_len={} blocks_len={}",
        fields[0], fields[1], fields[2], fields[3], fields[4]);
    p += 40 + 4; // checksum

    let info_len = fields[3] as usize;
    let block_info = &data[p..p + info_len];
    p += info_len;
    let info = if encrypted == "2" || encrypted == "3" {
        let mut md = Ripemd128::new();
        let mut v = Vec::from(&block_info[4..8]);
        v.extend_from_slice(&0x3695u32.to_le_bytes());
        md.update(v);
        let key = md.finalize();
        let mut d = Vec::from(&block_info[0..8]);
        d.extend(fast_decrypt(&block_info[8..], &key));
        let mut out = vec![];
        ZlibDecoder::new(&d[8..]).read_to_end(&mut out).unwrap();
        out
    } else {
        let mut out = vec![];
        ZlibDecoder::new(&block_info[8..]).read_to_end(&mut out).unwrap();
        out
    };
    println!("key info decompressed {} bytes", info.len());

    // 解析第一个 key block 的 size
    let mut q = 0usize;
    let take_u64 = |q: &mut usize| {
        let v = u64::from_be_bytes(info[*q..*q + 8].try_into().unwrap());
        *q += 8;
        v
    };
    let take_text = |q: &mut usize| {
        let n = u16::from_be_bytes([info[*q], info[*q + 1]]) as usize + 1;
        *q += 2 + n;
    };
    let csize0 = take_u64(&mut q) as usize; // 注意：先读 block num? 这里按现有顺序
    // 现有顺序: be_u64, text, text, be_u64(csize), be_u64(dsize)
    // 上面误读了，重新按结构来
    q = 0;
    take_u64(&mut q); // num entries
    take_text(&mut q);
    take_text(&mut q);
    let csize = take_u64(&mut q) as usize;
    let dsize = take_u64(&mut q) as usize;
    println!("first key block csize={} dsize={} (probe read {})", csize, dsize, csize0);

    // 第一个 key block 在 key blocks 区域
    let kb = &data[p..p + csize];
    let enc = u32::from_le_bytes(kb[0..4].try_into().unwrap());
    let checksum = &kb[4..8];
    let enc_method = (enc >> 4) & 0xf;
    let comp_method = enc & 0xf;
    println!("first key block enc_method={} comp_method={}", enc_method, comp_method);
    let mut md = Ripemd128::new();
    md.update(checksum);
    let key = md.finalize();
    let payload = if enc_method == 1 { fast_decrypt(&kb[8..], &key) } else { kb[8..].to_vec() };
    let block = if comp_method == 2 {
        let mut out = vec![];
        ZlibDecoder::new(&payload[..]).read_to_end(&mut out).unwrap();
        out
    } else {
        payload
    };
    println!("key block decompressed {} bytes, first 160: {:02x?}", block.len(), &block[..block.len().min(160)]);

    // 尝试按 UTF-16LE、2 字节终止符解析
    let mut r = 0usize;
    for i in 0..3 {
        if r + 8 > block.len() { break; }
        let off = u64::from_be_bytes(block[r..r + 8].try_into().unwrap());
        r += 8;
        let mut units = vec![];
        while r + 2 <= block.len() {
            let u = u16::from_le_bytes([block[r], block[r + 1]]);
            r += 2;
            if u == 0 { break; }
            units.push(u);
        }
        println!("  utf16 entry {}: offset={} key={:?}", i, off, String::from_utf16_lossy(&units));
    }
    // 尝试按 UTF-8、1 字节终止符解析（对照）
    let mut r = 0usize;
    for i in 0..3 {
        if r + 8 > block.len() { break; }
        let off = u64::from_be_bytes(block[r..r + 8].try_into().unwrap());
        r += 8;
        let start = r;
        while r < block.len() && block[r] != 0 { r += 1; }
        println!("  utf8  entry {}: offset={} key={:?}", i, off,
            String::from_utf8_lossy(&block[start..r]));
        r += 1;
    }
}

fn attr(xml: &str, name: &str) -> String {
    let pat = format!("{}=\"", name);
    if let Some(i) = xml.find(&pat) {
        let rest = &xml[i + pat.len()..];
        if let Some(j) = rest.find('"') {
            return rest[..j].to_string();
        }
    }
    String::new()
}

fn read_u32_be(d: &[u8]) -> Option<(usize, u32)> {
    Some((4, u32::from_be_bytes(d[0..4].try_into().ok()?)))
}

fn fast_decrypt(encrypted: &[u8], key: &[u8]) -> Vec<u8> {
    let mut buf = Vec::from(encrypted);
    let mut prev = 0x36u8;
    for i in 0..buf.len() {
        let mut t = buf[i].rotate_left(4);
        t = t ^ prev ^ (i as u8) ^ key[i % key.len()];
        prev = buf[i];
        buf[i] = t;
    }
    buf
}
