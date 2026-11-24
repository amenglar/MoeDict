//! End-to-end check: add a dictionary, build its index, query and reload.
//!
//! `cargo run -p mdict-core --example e2e -- <file.mdx> [word]`

use std::path::PathBuf;
use std::time::Instant;

use mdict_core::{DictionaryStore, IndexFile};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: e2e <file.mdx> [word]");
        std::process::exit(2);
    }
    let file = PathBuf::from(&args[0]);
    let word = args.get(1).map(String::as_str).unwrap_or("throw");

    let temp = tempfile::tempdir().expect("tempdir");
    let config_dir = temp.path().join("config");
    let data_dir = temp.path().join("data");

    let store = DictionaryStore::new(&config_dir, &data_dir).expect("store");
    println!("adding {}", file.display());
    let outcome = store.add_paths(&[file.clone()]).expect("add");
    for info in &outcome.added {
        println!(
            "added {:?}: id={} status={:?} entries={} has_mdd={}",
            info.name, info.id, info.status, info.entry_count, info.has_mdd
        );
    }
    for skipped in &outcome.skipped {
        println!("skipped {}: {}", skipped.path, skipped.reason);
    }
    let id = outcome
        .added
        .first()
        .expect("no dictionary added")
        .id
        .clone();

    let started = Instant::now();
    let mut last_percent = -1;
    let stats = store
        .build_index(&id, |progress| {
            let percent = progress.percent as i32;
            if percent / 10 != last_percent / 10 {
                last_percent = percent;
                println!(
                    "  {} {}/{} ({:.1}%)",
                    progress.phase, progress.current, progress.total, progress.percent
                );
            }
            true
        })
        .expect("index");
    println!(
        "indexed {} entries, raw={} stored={} index={} file={} in {:?}",
        stats.entries,
        stats.raw_bytes,
        stats.stored_bytes,
        stats.index_bytes,
        stats.file_bytes,
        started.elapsed()
    );

    let info = store.list();
    for entry in &info {
        println!(
            "list: {:?} status={:?} entries={} index_size={} error={:?}",
            entry.name, entry.status, entry.entry_count, entry.index_size, entry.error
        );
    }

    let started = Instant::now();
    let hits = store.query(word);
    println!(
        "query {:?}: {} hits in {:?}",
        word,
        hits.len(),
        started.elapsed()
    );
    for hit in &hits {
        println!(
            "  {}: headword={:?} html={} bytes, preview={:?}",
            hit.dict_name,
            hit.headword,
            hit.html.len(),
            &hit.html.chars().take(160).collect::<String>()
        );
    }

    let started = Instant::now();
    let suggestions = store.suggest("th");
    println!(
        "suggest 'th': {} in {:?}: {:?}",
        suggestions.len(),
        started.elapsed(),
        suggestions
            .iter()
            .map(|s| (&s.word, &s.dicts))
            .collect::<Vec<_>>()
    );
    println!("lucky: {:?}", store.lucky());

    for resource in [
        "O8C.css",
        "LSC4.css",
        "05DF8469.png",
        "95D5C9C1.png",
        "123.gif",
    ] {
        match store.read_resource(&id, resource) {
            Ok(Some((bytes, mime))) => {
                println!("resource {resource}: {} bytes, {mime}", bytes.len())
            }
            Ok(None) => println!("resource {resource}: not found"),
            Err(error) => println!("resource {resource}: error {error}"),
        }
    }

    // Confirm the index can be reopened without the parser.
    let idx = IndexFile::open(&store.index_dir().join(format!("{id}.idx"))).expect("reopen");
    println!(
        "reopened index: entries={} source_size={}",
        idx.len(),
        idx.source_size()
    );

    // Reload from disk: configuration and index metadata survive.
    drop(store);
    let store = DictionaryStore::new(&config_dir, &data_dir).expect("reload");
    for entry in store.list() {
        println!(
            "reloaded: {:?} status={:?} entries={}",
            entry.name, entry.status, entry.entry_count
        );
    }
}
