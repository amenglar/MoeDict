use std::path::PathBuf;

use mdict_core::{DictStatus, DictionaryStore};

fn sample(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("resources/mdx/en")
        .join(name)
}

#[test]
fn indexes_queries_and_reloads_a_real_dictionary() {
    let mdx = sample("牛津英语搭配词典.mdx");
    if !mdx.exists() {
        eprintln!(
            "skipping: sample dictionary {} is not present",
            mdx.display()
        );
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let config_dir = temp.path().join("config");
    let data_dir = temp.path().join("data");

    let store = DictionaryStore::new(&config_dir, &data_dir).unwrap();
    let outcome = store.add_paths(&[mdx.clone()]).unwrap();
    assert_eq!(outcome.added.len(), 1);
    let dict = &outcome.added[0];
    let id = dict.id.clone();
    assert_eq!(dict.status, DictStatus::Unindexed);
    assert!(dict.has_mdd, "the sample ships with an MDD");

    let stats = store.build_index(&id, |_| true).unwrap();
    assert!(stats.entries > 1000, "entries = {}", stats.entries);

    let hits = store.query("ability");
    assert!(!hits.is_empty(), "expected a hit for 'ability'");
    assert!(!hits[0].html.is_empty());

    let suggestions = store.suggest("th");
    assert!(!suggestions.is_empty());
    assert!(store.lucky().is_some());

    let resource = store.read_resource(&id, "95D5C9C1.png").unwrap();
    assert!(resource.is_some(), "expected the MDD image to resolve");
    let (bytes, mime) = resource.unwrap();
    assert!(bytes.starts_with(b"\x89PNG"));
    assert_eq!(mime, "image/png");

    let info = store.list();
    assert_eq!(info[0].status, DictStatus::Ready);
    assert_eq!(info[0].entry_count, stats.entries as u64);

    drop(store);
    let reloaded = DictionaryStore::new(&config_dir, &data_dir).unwrap();
    let info = reloaded.list();
    assert_eq!(info.len(), 1);
    assert_eq!(info[0].status, DictStatus::Ready);
    assert_eq!(info[0].entry_count, stats.entries as u64);
}

#[test]
fn missing_source_is_marked_missing() {
    let temp = tempfile::tempdir().unwrap();
    let store =
        DictionaryStore::new(&temp.path().join("config"), &temp.path().join("data")).unwrap();
    assert!(store.list().is_empty());
    let outcome = store.add_paths(&[temp.path().join("nope.mdx")]).unwrap();
    assert!(outcome.added.is_empty());
    assert_eq!(outcome.skipped.len(), 1);
    assert!(store.list().is_empty());
}
