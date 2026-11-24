//! Quick diagnostics for a real MDX/MDD file.
//!
//! `cargo run -p mdict-core --example inspect -- <file> [--limit N]`

use std::path::Path;
use std::time::Instant;

use mdict_core::Mdict;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: inspect <file.mdx|file.mdd> [--limit N]");
        std::process::exit(2);
    }
    let path = Path::new(&args[0]);
    let limit = args
        .iter()
        .position(|arg| arg == "--limit")
        .and_then(|index| args.get(index + 1))
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(usize::MAX);

    let started = Instant::now();
    let mdict = match Mdict::open(path) {
        Ok(mdict) => mdict,
        Err(error) => {
            eprintln!("failed to parse {}: {error}", path.display());
            std::process::exit(1);
        }
    };
    println!(
        "{}: title={:?} encoding={:?} entries={} blocks={} parse={:?}",
        path.display(),
        mdict.title(),
        mdict.encoding(),
        mdict.entry_count(),
        mdict.total_decompressed_size(),
        started.elapsed()
    );
    if let Some((first, last)) = mdict.entries().first().zip(mdict.entries().last()) {
        println!(
            "first key={:?} offset={} last key={:?} offset={}",
            first.text, first.offset, last.text, last.offset
        );
    }

    let started = Instant::now();
    let mut count = 0usize;
    let mut bytes = 0usize;
    let result = mdict.for_each_entry(|key, definition| {
        if count < 3 {
            println!(
                "  entry {:?}: {} bytes, preview={:?}",
                key,
                definition.len(),
                String::from_utf8_lossy(&definition[..definition.len().min(120)])
            );
        }
        count += 1;
        bytes += definition.len();
        if count >= limit {
            return Err(mdict_core::Error::Cancelled);
        }
        Ok(())
    });
    match result {
        Ok(_) => println!(
            "iterated {count} entries ({bytes} bytes) in {:?}",
            started.elapsed()
        ),
        Err(mdict_core::Error::Cancelled) => {
            println!(
                "iterated first {count} entries ({bytes} bytes) in {:?}",
                started.elapsed()
            )
        }
        Err(error) => {
            eprintln!("iteration failed after {count} entries: {error}");
            std::process::exit(1);
        }
    }
}
