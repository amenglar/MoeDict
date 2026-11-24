use moedict_native::ast::DictBlock;
use moedict_native::html::parse_entry;

#[test]
fn parses_inline_styles_and_class_hints() {
    let html = r#"
        <style>
          .pracpron { display: none; }
          .phon-gb { color: #004aac; font-family: "Kingsoft Phonetic Plain"; }
        </style>
        <div class="h-g"><span class="top-g"><span class="h">abandon</span></span></div>
        <p><span class="phon-gb">/əˈbændən/</span> <span class="pos">v.</span></p>
        <div class="sd">to leave someone or something</div>
        <div class="def-g"><span class="d">to give up</span></div>
        <div class="x-g"><span class="x">He abandoned his car.</span></div>
        <div class="pracpron">hidden pronunciation</div>
    "#;
    let entry = parse_entry(html);
    let text = entry.plain_text();
    assert!(text.contains("abandon"), "{text}");
    assert!(text.contains("əˈbændən"), "{text}");
    assert!(text.contains("He abandoned his car."), "{text}");
    assert!(
        !text.contains("hidden pronunciation"),
        "display:none must be honoured: {text}"
    );
    assert!(
        entry
            .blocks
            .iter()
            .any(|b| matches!(b, DictBlock::Headword { .. })),
        "{entry:?}"
    );
}

#[test]
fn parses_attribute_selectors_and_oxford_colors() {
    let html = r#"
        <style>
          .cf[display="block"] { font-weight: bold; color: #C76E06; }
          .xr-g[level="2"] { border: 1px solid #e8c49a; }
          .symbols-coresym { color: green; }
          .infl { display: none; }
        </style>
        <span class="cf" display="block">phrasal verb</span>
        <span class="symbols-coresym">◆</span>
        <span class="infl">should not appear</span>
    "#;
    let entry = parse_entry(html);
    let text = entry.plain_text();
    assert!(text.contains("phrasal verb"), "{text}");
    assert!(text.contains("◆"), "{text}");
    assert!(!text.contains("should not appear"), "{text}");
}

#[test]
fn parses_tables_lists_and_images() {
    let html = r#"
        <ul><li>first sense</li><li>second sense</li></ul>
        <table><tr><th>Form</th><th>Meaning</th></tr><tr><td>abandon</td><td>放弃</td></tr></table>
        <img src="images/example.png" alt="example"/>
        <a href="entry://escape">see escape</a>
    "#;
    let entry = parse_entry(html);
    assert!(
        entry
            .blocks
            .iter()
            .any(|b| matches!(b, DictBlock::List { .. })),
        "{entry:?}"
    );
    assert!(
        entry
            .blocks
            .iter()
            .any(|b| matches!(b, DictBlock::Table { .. })),
        "{entry:?}"
    );
    assert!(
        entry
            .blocks
            .iter()
            .any(|b| matches!(b, DictBlock::Image { .. })),
        "{entry:?}"
    );
    let text = entry.plain_text();
    assert!(text.contains("escape"), "{text}");
}

#[test]
fn never_drops_unknown_content() {
    let html = r"<custom-widget><mystery>something unusual</mystery></custom-widget>";
    let entry = parse_entry(html);
    assert!(
        entry.plain_text().contains("something unusual"),
        "{entry:?}"
    );
}
