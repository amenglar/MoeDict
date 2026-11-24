use mdict_core::util::{adler32, hash_id, normalize_resource_path};
use mdict_core::{IndexBuilder, IndexFile};

#[test]
fn adler32_matches_reference() {
    assert_eq!(adler32(b"123456789"), 0x091e01de);
    assert_eq!(adler32(b""), 1);
}

#[test]
fn ids_are_stable_hex() {
    let a = hash_id("/tmp/词典/a.mdx");
    let b = hash_id("/tmp/词典/a.mdx");
    assert_eq!(a, b);
    assert_eq!(a.len(), 64);
    assert!(a.bytes().all(|b| b.is_ascii_hexdigit()));
}

#[test]
fn resource_paths_are_normalised() {
    assert_eq!(normalize_resource_path("\\Sound\\A.WAV"), "sound/a.wav");
    assert_eq!(
        normalize_resource_path("file:///C:/Images/Pic.png"),
        "c:/images/pic.png"
    );
    assert_eq!(normalize_resource_path("\u{1e}img.png\u{1f}"), "img.png");
    assert_eq!(
        normalize_resource_path("/images/a%20b.png"),
        "images/a b.png"
    );
}

#[test]
fn index_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.idx");
    let mut builder = IndexBuilder::new(dir.path(), 42, 7).unwrap();
    builder.add("apple", b"<b>apple</b>").unwrap();
    builder.add("Apple", b"<b>Apple</b>").unwrap();
    builder.add("apply", b"<i>apply</i>").unwrap();
    builder.add("香蕉", "香蕉".as_bytes()).unwrap();
    builder.add("", b"ignored empty key").unwrap();
    let stats = builder.finish(&path).unwrap();
    assert_eq!(stats.entries, 4);
    assert_eq!(stats.raw_bytes, 12 + 12 + 12 + "香蕉".len() as u64);

    let index = IndexFile::open(&path).unwrap();
    assert_eq!(index.len(), 4);
    assert_eq!(index.source_size(), 42);
    assert_eq!(index.source_mtime(), 7);

    let (key, html) = index.exact("apple").unwrap().unwrap();
    assert_eq!(key, "apple");
    assert!(html.contains("apple"));

    // Case-insensitive fallback still works.
    assert!(index.exact("APPLE").unwrap().is_some());

    let prefix = index.prefix("appl", 10).unwrap();
    assert!(prefix.iter().any(|word| word.eq_ignore_ascii_case("apple")));
    assert!(prefix.iter().any(|word| word == "apply"));

    let chinese = index.prefix("香", 10).unwrap();
    assert!(chinese.contains(&"香蕉".to_string()));

    assert!(index.exact("missing").unwrap().is_none());
    assert!(index.random_key().unwrap().is_some());
    assert!(index.definition(0).is_ok());
}

#[test]
fn index_follows_link_redirects() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("links.idx");
    let mut builder = IndexBuilder::new(dir.path(), 1, 1).unwrap();
    builder.add("Buch", b"<p><b>Buch</b> book</p>").unwrap();
    builder.add("buch", b"@@@LINK=Buch").unwrap();
    builder.add("Buchstabe", b"letter").unwrap();
    builder.add("wrapped", b"<p>@@@LINK=Buchstabe</p>").unwrap();
    builder.add("chained", b"@@@LINK=buch\r\n").unwrap();
    builder.add("loopA", b"@@@LINK=loopB").unwrap();
    builder.add("loopB", b"@@@LINK=loopA").unwrap();
    builder.add("dangling", b"@@@LINK=missing").unwrap();
    builder.finish(&path).unwrap();

    let index = IndexFile::open(&path).unwrap();

    let (key, html) = index.exact("buch").unwrap().unwrap();
    assert_eq!(key, "buch");
    assert!(html.contains("book"));
    assert!(!html.contains("@@@LINK"));

    let (_, html) = index.exact("wrapped").unwrap().unwrap();
    assert_eq!(html, "letter");

    // Redirects are followed transitively, and case fallbacks still apply.
    let (_, html) = index.exact("chained").unwrap().unwrap();
    assert!(html.contains("book"));

    // Cycles terminate and return whatever was resolved last.
    assert!(index.exact("loopA").unwrap().is_some());

    // Dangling targets keep the raw marker rather than dropping the entry.
    let (_, html) = index.exact("dangling").unwrap().unwrap();
    assert!(html.contains("@@@LINK"));
}

#[test]
fn index_handles_duplicate_headwords() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("dup.idx");
    let mut builder = IndexBuilder::new(dir.path(), 1, 1).unwrap();
    builder.add("bank", b"financial institution").unwrap();
    builder.add("bank", b"river bank").unwrap();
    builder.add("bank", b"financial institution").unwrap();
    builder.finish(&path).unwrap();

    let index = IndexFile::open(&path).unwrap();
    let (key, html) = index.exact("bank").unwrap().unwrap();
    assert_eq!(key, "bank");
    assert!(!html.is_empty());
    assert_eq!(index.prefix("ban", 10).unwrap(), vec!["bank".to_string()]);
}
