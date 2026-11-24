//! Research tool: sample real MDX entries and report AST conversion coverage.
//!
//! `cargo run --release -p moedict-native --example coverage -- <file.mdx> [--limit N]`

use std::{collections::HashMap, env, path::Path};

use mdict_core::{Error, Mdict};
use moedict_native::ast::DictBlock;
use moedict_native::html::parse_entry_with_css;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!("usage: coverage <file.mdx> [--limit N]");
        std::process::exit(2);
    };
    let limit = args
        .iter()
        .position(|a| a == "--limit")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(200);

    let mdict = Mdict::open(Path::new(path)).expect("open mdx");
    let external_css = Path::new(path)
        .parent()
        .and_then(|dir| std::fs::read_dir(dir).ok())
        .and_then(|entries| {
            entries.filter_map(Result::ok).find(|e| {
                e.path()
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("css"))
            })
        })
        .and_then(|e| std::fs::read(e.path()).ok())
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
    let encoding = encoding_rs::Encoding::for_label(mdict.encoding().as_bytes())
        .unwrap_or(encoding_rs::UTF_8);
    println!(
        "title={:?} encoding={} entries={}",
        mdict.title(),
        mdict.encoding(),
        mdict.entry_count()
    );

    let mut kinds: HashMap<&'static str, usize> = HashMap::new();
    let mut count = 0usize;
    let mut fallbacks = 0usize;
    let mut chars = 0usize;
    let mut html_chars = 0usize;
    let result = mdict.for_each_entry(|key, definition| {
        let (html, _, _) = encoding.decode(&definition);
        let entry = parse_entry_with_css(&html, external_css.as_deref());
        html_chars += html.chars().count();
        chars += entry.plain_text().chars().count();
        if entry.blocks.is_empty() {
            fallbacks += 1;
        }
        for block in &entry.blocks {
            let kind = match block {
                DictBlock::Headword { .. } => "headword",
                DictBlock::Sense { .. } => "sense",
                DictBlock::Example(_) => "example",
                DictBlock::CrossRef { .. } => "crossref",
                DictBlock::Note { .. } => "note",
                DictBlock::Image { .. } => "image",
                DictBlock::Table { .. } => "table",
                DictBlock::List { .. } => "list",
                DictBlock::Paragraph(_) => "paragraph",
                DictBlock::Divider => "divider",
                DictBlock::Fallback(_) => "fallback",
            };
            *kinds.entry(kind).or_default() += 1;
        }
        if count == 0 {
            println!("sample key={:?}\ntext={}", key, &entry.plain_text()[..entry.plain_text().len().min(500)]);
            println!("html preview={}", &html[..html.len().min(2500)]);
        }
        count += 1;
        if count >= limit {
            return Err(Error::Cancelled);
        }
        Ok(())
    });
    if !matches!(result, Err(Error::Cancelled)) {
        result.expect("iterate");
    }
    let mut summary: Vec<_> = kinds.into_iter().collect();
    summary.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    println!(
        "entries={count} empty={fallbacks} html_chars={html_chars} ast_chars={chars} ratio={:.1}%",
        if html_chars == 0 { 0.0 } else { chars as f64 / html_chars as f64 * 100.0 }
    );
    for (kind, n) in summary {
        println!("  {kind}: {n}");
    }
}
