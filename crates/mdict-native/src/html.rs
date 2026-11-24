use html5ever::{ParseOpts, parse_document, tendril::TendrilSink};
use markup5ever_rcdom::{Handle, NodeData, RcDom};

use crate::ast::{
    DictBlock, DictEntry, FontRole, InlineStyle, LinkAction, RichText, SemanticColor,
};

pub fn parse_entry(html: &str) -> DictEntry {
    parse_entry_with_css(html, None)
}

pub fn parse_entry_with_css(html: &str, extra_css: Option<&str>) -> DictEntry {
    let dom = parse_document(RcDom::default(), ParseOpts::default()).one(html);
    let root = find_body(&dom.document).unwrap_or_else(|| dom.document.clone());
    let rules = CssRules::collect(&dom.document, extra_css);
    let ctx = ConvertCtx { rules };
    let mut blocks = Vec::new();
    convert_children(&root, &ctx, &mut blocks);
    if blocks.is_empty() {
        let text = plain_text_of(&root).trim().to_string();
        if !text.is_empty() {
            blocks.push(DictBlock::Fallback(RichText::from_plain(text)));
        }
    }
    DictEntry {
        headword: String::new(),
        blocks,
    }
}

fn find_body(node: &Handle) -> Option<Handle> {
    if tag(node).as_deref() == Some("body") {
        return Some(node.clone());
    }
    for child in children(node) {
        if let Some(found) = find_body(&child) {
            return Some(found);
        }
    }
    None
}

fn children(node: &Handle) -> Vec<Handle> {
    node.children.borrow().clone()
}

fn tag(node: &Handle) -> Option<String> {
    match &node.data {
        NodeData::Element { name, .. } => Some(name.local.as_ref().to_ascii_lowercase()),
        _ => None,
    }
}

fn attr(node: &Handle, wanted: &str) -> Option<String> {
    match &node.data {
        NodeData::Element { attrs, .. } => attrs
            .borrow()
            .iter()
            .find(|a| a.name.local.as_ref().eq_ignore_ascii_case(wanted))
            .map(|a| a.value.trim().to_string()),
        _ => None,
    }
}

fn class_list(node: &Handle) -> Vec<String> {
    let mut classes: Vec<String> = attr(node, "class")
        .unwrap_or_default()
        .split_whitespace()
        .map(|s| s.to_ascii_lowercase())
        .collect();
    if let Some(id) = attr(node, "id") {
        for token in id.split_whitespace() {
            let token = token.to_ascii_lowercase();
            if !classes.contains(&token) {
                classes.push(token);
            }
        }
    }
    classes
}

fn has_class(node: &Handle, names: &[&str]) -> bool {
    class_list(node)
        .iter()
        .any(|c| names.iter().any(|n| c == n || c.starts_with(n)))
}

fn ancestors(node: &Handle) -> Vec<Handle> {
    let mut out = vec![node.clone()];
    let mut current = node.clone();
    while let Some(parent) = current.parent.take() {
        let Some(parent) = parent.upgrade() else {
            break;
        };
        out.push(parent.clone());
        current = parent;
    }
    out
}

fn plain_text_of(node: &Handle) -> String {
    let mut out = String::new();
    collect_text(node, &mut out);
    out
}

fn collect_text(node: &Handle, out: &mut String) {
    match &node.data {
        NodeData::Text { contents } => out.push_str(&contents.borrow()),
        NodeData::Element { name, .. } => {
            let t = name.local.as_ref();
            if matches!(
                t,
                "script" | "style" | "head" | "meta" | "title" | "noscript"
            ) {
                return;
            }
            for child in children(node) {
                collect_text(&child, out);
            }
        }
        _ => {}
    }
}

struct ConvertCtx {
    rules: CssRules,
}

fn convert_children(node: &Handle, ctx: &ConvertCtx, out: &mut Vec<DictBlock>) {
    let mut pending = RichText::new();
    for child in children(node) {
        match &child.data {
            NodeData::Text { contents } => {
                let text = contents.borrow();
                if !text.is_empty() {
                    push_collapsed(&mut pending, &text, InlineStyle::default(), None);
                }
            }
            NodeData::Element { .. } => {
                let Some(t) = tag(&child) else { continue };
                let style = ctx.rules.style_for(&child, &InlineStyle::default());
                if style.hidden {
                    continue;
                }
                match t.as_str() {
                    "script" | "style" | "head" | "meta" | "link" | "noscript" | "iframe"
                    | "object" | "embed" | "form" | "input" | "button" | "select" | "textarea"
                    | "title" | "base" => continue,
                    "br" => pending.push("\n", InlineStyle::default(), None),
                    "hr" => {
                        flush(&mut pending, out);
                        out.push(DictBlock::Divider);
                    }
                    "img" | "image" => {
                        flush(&mut pending, out);
                        let path = attr(&child, "src")
                            .or_else(|| attr(&child, "xlink:href"))
                            .unwrap_or_default();
                        if !path.is_empty() {
                            out.push(DictBlock::Image {
                                path,
                                caption: attr(&child, "alt")
                                    .filter(|s| !s.is_empty())
                                    .or_else(|| attr(&child, "title")),
                            });
                        }
                    }
                    "ul" | "ol" => {
                        flush(&mut pending, out);
                        out.push(parse_list(&child, t == "ol", ctx));
                    }
                    "table" => {
                        flush(&mut pending, out);
                        out.push(parse_table(&child, ctx));
                    }
                    "p" | "div" | "section" | "article" | "blockquote" | "center" | "dl" | "h1"
                    | "h2" | "h3" | "h4" | "h5" | "h6" | "li" => {
                        flush(&mut pending, out);
                        convert_element_block(&child, &t, ctx, out);
                    }
                    _ if out.is_empty()
                        && pending.is_empty()
                        && has_class(&child, &["headword", "head", "hw", "word", "entry", "id"]) =>
                    {
                        convert_element_block(&child, &t, ctx, out);
                    }
                    _ if style.block => {
                        flush(&mut pending, out);
                        convert_element_block(&child, &t, ctx, out);
                    }
                    _ => inline_into(&child, &style, ctx, &mut pending),
                }
            }
            _ => {}
        }
    }
    flush(&mut pending, out);
}

fn flush(pending: &mut RichText, out: &mut Vec<DictBlock>) {
    if pending.is_empty() {
        return;
    }
    let rt = std::mem::take(pending);
    out.push(DictBlock::Paragraph(rt));
}

fn convert_element_block(node: &Handle, t: &str, ctx: &ConvertCtx, out: &mut Vec<DictBlock>) {
    let style = ctx.rules.style_for(node, &InlineStyle::default());
    let mut inner = Vec::new();
    convert_children(node, ctx, &mut inner);
    if inner.is_empty() {
        return;
    }

    let oxford_headword = has_class(node, &["h"])
        && ancestors(node)
            .iter()
            .any(|ancestor| has_class(ancestor, &["h-g", "top-g"]));
    if has_class(node, &["headword", "head", "hw", "word", "entry", "id"]) || oxford_headword {
        let mut text = String::new();
        let mut phonetics = Vec::new();
        let mut pos = None;
        for block in &inner {
            if let DictBlock::Paragraph(rt) = block {
                for run in &rt.runs {
                    if run.style.font == FontRole::Phonetic {
                        let p = run.text.trim();
                        if !p.is_empty() {
                            phonetics.push(p.to_string());
                        }
                    } else if run.style.color == Some(SemanticColor::Pos) {
                        pos = Some(run.text.trim().to_string());
                    } else {
                        text.push_str(&run.text);
                    }
                }
            }
        }
        let text = text.trim().to_string();
        if !text.is_empty() {
            out.push(DictBlock::Headword {
                text,
                phonetics,
                pos,
            });
            return;
        }
    }

    if has_class(node, &["example", "examp", "eg", "ex"]) {
        for block in inner {
            match block {
                DictBlock::Paragraph(rt) => out.push(DictBlock::Example(rt)),
                other => out.push(other),
            }
        }
        return;
    }

    if has_class(node, &["note", "usage", "tip", "warning", "gram"]) {
        let mut content = RichText::new();
        for block in &inner {
            if let DictBlock::Paragraph(rt) = block {
                for run in &rt.runs {
                    content.push(run.text.clone(), run.style.clone(), run.action.clone());
                }
            }
        }
        if !content.is_empty() {
            out.push(DictBlock::Note {
                label: None,
                content,
            });
            return;
        }
    }

    if has_class(
        node,
        &["sense", "meaning", "definition", "def", "item", "s"],
    ) || matches!(t, "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
    {
        let mut definition = RichText::new();
        let mut examples = Vec::new();
        let mut children_blocks = Vec::new();
        for block in std::mem::take(&mut inner) {
            match block {
                DictBlock::Paragraph(rt) if definition.is_empty() => definition = rt,
                DictBlock::Example(rt) => examples.push(rt),
                other => children_blocks.push(other),
            }
        }
        if !definition.is_empty() || !children_blocks.is_empty() {
            let number = leading_number(&definition.plain_text());
            out.push(DictBlock::Sense {
                number,
                pos: if style.color == Some(SemanticColor::Pos) {
                    Some(definition.plain_text().trim().to_string())
                } else {
                    None
                },
                definition,
                examples,
                children: children_blocks,
                level: 0,
            });
            return;
        }
    }

    if inner.len() == 1 {
        out.push(inner.into_iter().next().unwrap());
    } else {
        out.extend(inner);
    }
}

fn leading_number(text: &str) -> Option<String> {
    let text = text.trim_start();
    let end = text
        .char_indices()
        .take_while(|(_, c)| c.is_ascii_digit() || *c == '.' || *c == ')' || *c == '(')
        .map(|(i, c)| i + c.len_utf8())
        .last()
        .unwrap_or(0);
    if end > 0 && text[..end].chars().any(|c| c.is_ascii_digit()) {
        Some(text[..end].to_string())
    } else {
        None
    }
}

fn parse_list(node: &Handle, ordered: bool, ctx: &ConvertCtx) -> DictBlock {
    let mut items = Vec::new();
    let mut found = Vec::new();
    collect_by_tag(node, "li", &mut found);
    for li in found {
        let mut inner = Vec::new();
        convert_children(&li, ctx, &mut inner);
        let mut rt = RichText::new();
        for block in inner {
            match block {
                DictBlock::Paragraph(p) | DictBlock::Fallback(p) | DictBlock::Example(p) => {
                    for run in p.runs {
                        rt.push(run.text, run.style, run.action);
                    }
                }
                DictBlock::Divider => rt.push(" ", InlineStyle::default(), None),
                _ => {}
            }
        }
        if !rt.is_empty() {
            items.push(rt);
        }
    }
    DictBlock::List { ordered, items }
}

fn parse_table(node: &Handle, ctx: &ConvertCtx) -> DictBlock {
    let mut rows = Vec::new();
    let mut found = Vec::new();
    collect_by_tag(node, "tr", &mut found);
    let mut header = false;
    for tr in found {
        let mut row = Vec::new();
        let mut cells = Vec::new();
        collect_by_tag(&tr, "td", &mut cells);
        collect_by_tag(&tr, "th", &mut cells);
        cells.sort_by_key(|c| cell_index(c));
        for cell in cells {
            if tag(&cell).as_deref() == Some("th") {
                header = true;
            }
            let mut inner = Vec::new();
            convert_children(&cell, ctx, &mut inner);
            let mut rt = RichText::new();
            for block in inner {
                if let DictBlock::Paragraph(p) | DictBlock::Fallback(p) = block {
                    for run in p.runs {
                        rt.push(run.text, run.style, run.action);
                    }
                }
            }
            row.push(rt);
        }
        if !row.is_empty() {
            rows.push(row);
        }
    }
    DictBlock::Table { rows, header }
}

fn cell_index(node: &Handle) -> usize {
    let Some(parent) = node.parent.take().and_then(|p| p.upgrade()) else {
        return 0;
    };
    parent
        .children
        .borrow()
        .iter()
        .position(|c| std::rc::Rc::ptr_eq(c, node))
        .unwrap_or(0)
}

fn collect_by_tag(node: &Handle, wanted: &str, out: &mut Vec<Handle>) {
    for child in children(node) {
        if tag(&child).as_deref() == Some(wanted) {
            out.push(child.clone());
        } else {
            collect_by_tag(&child, wanted, out);
        }
    }
}

fn inline_into(node: &Handle, base: &InlineStyle, ctx: &ConvertCtx, pending: &mut RichText) {
    let mut style = ctx.rules.style_for(node, base);
    if style.hidden {
        return;
    }
    let t = tag(node).unwrap_or_default();
    match t.as_str() {
        "b" | "strong" => style.bold = true,
        "i" | "em" | "cite" => style.italic = true,
        "u" => style.underline = true,
        "s" | "strike" | "del" => style.strike = true,
        "sup" | "sub" => style.size_delta = style.size_delta.saturating_sub(3),
        "small" => style.size_delta = style.size_delta.saturating_sub(1),
        "big" => style.size_delta = style.size_delta.saturating_add(2),
        "font" => {
            if let Some(face) = attr(node, "face") {
                if face.to_ascii_lowercase().contains("phonetic") {
                    style.font = FontRole::Phonetic;
                }
            }
        }
        "a" | "area" => {
            style.color = Some(SemanticColor::Link);
        }
        _ => {}
    }

    let action = if t == "a" || t == "area" {
        attr(node, "href").and_then(|h| link_action(&h))
    } else {
        None
    };

    for child in children(node) {
        match &child.data {
            NodeData::Text { contents } => {
                push_collapsed(pending, &contents.borrow(), style.clone(), action.clone());
            }
            NodeData::Element { .. } => {
                let Some(ct) = tag(&child) else { continue };
                match ct.as_str() {
                    "script" | "style" | "noscript" | "head" | "meta" => continue,
                    "br" => pending.push("\n", style.clone(), action.clone()),
                    "img" | "image" => {
                        let alt = attr(&child, "alt").unwrap_or_default();
                        if !alt.is_empty() {
                            push_collapsed(pending, &alt, style.clone(), action.clone());
                        }
                    }
                    _ => inline_into(&child, &style, ctx, pending),
                }
            }
            _ => {}
        }
    }
}

fn push_collapsed(
    pending: &mut RichText,
    text: &str,
    style: InlineStyle,
    action: Option<LinkAction>,
) {
    let mut buffer = String::new();
    let mut iter = text.chars().peekable();
    while let Some(ch) = iter.next() {
        if ch.is_whitespace() {
            buffer.push(' ');
            while iter.peek().is_some_and(|c| c.is_whitespace()) {
                iter.next();
            }
        } else {
            buffer.push(ch);
        }
    }
    pending.push(buffer, style, action);
}

fn link_action(href: &str) -> Option<LinkAction> {
    let decoded = mdict_core::util::percent_decode(href);
    let decoded = decoded.trim();
    if decoded.is_empty() {
        return None;
    }
    let (target, anchor) = match decoded.split_once('#') {
        Some((a, b)) => (a.trim(), Some(b.trim().to_string())),
        None => (decoded, None),
    };
    let lower = target.to_ascii_lowercase();
    if lower.starts_with("entry://") || lower.starts_with("word://") {
        let word = target.split_once("://").map(|(_, w)| w).unwrap_or(target);
        let (word, dict_id) = match word.split_once("?dict=") {
            Some((w, id)) => (w, Some(id.to_string())),
            None => (word, None),
        };
        if word.is_empty() && anchor.is_none() {
            return None;
        }
        return Some(LinkAction::Entry {
            word: word.to_string(),
            dict_id,
            anchor,
        });
    }
    if lower.starts_with("sound://") {
        let path = target.split_once("://").map(|(_, p)| p).unwrap_or(target);
        return Some(LinkAction::Sound {
            path: path.to_string(),
        });
    }
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:")
    {
        return Some(LinkAction::External {
            url: decoded.to_string(),
        });
    }
    if lower.starts_with("javascript:")
        || lower.starts_with("vbscript:")
        || lower.starts_with("data:")
    {
        return None;
    }
    if target.is_empty() {
        return anchor.map(|id| LinkAction::Anchor { id });
    }
    if target.contains("://") || target.starts_with("file:") {
        return None;
    }
    Some(LinkAction::Entry {
        word: target.to_string(),
        dict_id: None,
        anchor,
    })
}

struct CssRules {
    rules: Vec<CssRule>,
}

impl CssRules {
    fn collect(root: &Handle, extra_css: Option<&str>) -> Self {
        let mut texts = Vec::new();
        let mut styles = Vec::new();
        collect_by_tag(root, "style", &mut styles);
        for style in styles {
            let mut css = String::new();
            for child in children(&style) {
                if let NodeData::Text { contents } = &child.data {
                    css.push_str(&contents.borrow());
                }
            }
            texts.push(css);
        }
        if let Some(css) = extra_css {
            texts.push(css.to_string());
        }
        let mut rules = Vec::new();
        for text in texts {
            for chunk in text.split('}') {
                let Some((selector, body)) = chunk.split_once('{') else {
                    continue;
                };
                let selector = selector.trim();
                if selector.is_empty() || selector.starts_with('@') || selector.starts_with("/*") {
                    continue;
                }
                for part in selector.split(',') {
                    let parsed = SimpleSelector::parse(part);
                    if parsed.parts.is_empty() {
                        continue;
                    }
                    let decls = body
                        .split(';')
                        .filter_map(|decl| decl.split_once(':'))
                        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
                        .collect();
                    rules.push(CssRule {
                        selector: parsed,
                        decls,
                    });
                }
            }
        }
        Self { rules }
    }

    fn style_for(&self, node: &Handle, base: &InlineStyle) -> InlineStyle {
        let mut style = base.clone();
        let ancestors = ancestors(node);
        let mut matched: Vec<&CssRule> = self
            .rules
            .iter()
            .filter(|rule| rule.selector.matches(&ancestors))
            .collect();
        matched.sort_by_key(|rule| rule.selector.specificity());
        for rule in matched {
            for (prop, value) in &rule.decls {
                apply_decl(&mut style, prop, value);
            }
        }
        if let NodeData::Element { attrs, .. } = &node.data {
            for attr in attrs.borrow().iter() {
                if attr.name.local.as_ref() == "style" {
                    for decl in attr.value.split(';') {
                        if let Some((k, v)) = decl.split_once(':') {
                            apply_decl(&mut style, &k.trim().to_ascii_lowercase(), v.trim());
                        }
                    }
                }
            }
        }
        apply_class_hints(&mut style, &class_list(node));
        style
    }
}

fn apply_class_hints(style: &mut InlineStyle, classes: &[String]) {
    for class in classes {
        let c = class.as_str();
        if c.contains("phon") || c.contains("ipa") || c.contains("symbol") {
            style.font = FontRole::Phonetic;
            style.color = Some(SemanticColor::Phonetic);
        } else if c.starts_with("pos") || c.contains("partofspeech") {
            style.color = Some(SemanticColor::Pos);
        } else if matches!(c, "example" | "examp" | "eg" | "ex" | "e") {
            style.color = Some(SemanticColor::Example);
            style.italic = true;
        } else if matches!(c, "headword" | "head" | "hw") {
            style.bold = true;
            style.color = Some(SemanticColor::Headword);
        } else if c.contains("note") || c.contains("usage") {
            style.color = Some(SemanticColor::Secondary);
        } else if c.contains("link") || c.contains("ref") {
            style.color = Some(SemanticColor::Link);
        }
    }
}

fn apply_decl(style: &mut InlineStyle, prop: &str, value: &str) {
    let value = value.trim().trim_end_matches('!').trim();
    let lower = value.to_ascii_lowercase();
    match prop {
        "color" => {
            if let Some(color) = map_color(&lower) {
                style.color = Some(color);
            }
        }
        "background-color" | "background" => {
            if let Some(color) = map_color(&lower) {
                style.background = Some(color);
            }
        }
        "font-weight" => {
            if lower == "bold" || lower == "bolder" || lower.parse::<u16>().unwrap_or(400) >= 600 {
                style.bold = true;
            }
        }
        "font-style" => {
            if lower == "italic" || lower == "oblique" {
                style.italic = true;
            }
        }
        "text-decoration" | "text-decoration-line" => {
            if lower.contains("underline") {
                style.underline = true;
            }
            if lower.contains("line-through") {
                style.strike = true;
            }
        }
        "font-size" => {
            style.size_delta = if let Some(percent) = lower.strip_suffix('%') {
                let factor = percent.parse::<f32>().unwrap_or(100.0) / 100.0;
                size_delta(factor)
            } else {
                match lower.as_str() {
                    "xx-small" => -3,
                    "x-small" | "smaller" | "small" => -1,
                    "large" | "larger" => 1,
                    "x-large" => 2,
                    "xx-large" => 3,
                    _ => {
                        let px = lower.trim_end_matches("px").parse::<f32>().unwrap_or(16.0);
                        size_delta(px / 16.0)
                    }
                }
            };
        }
        "display" => {
            if lower == "none" {
                style.hidden = true;
            } else if lower.contains("block") || lower.contains("table") {
                style.block = true;
            }
        }
        "visibility" => {
            if lower == "hidden" || lower == "collapse" {
                style.hidden = true;
            }
        }
        "font-family" => {
            let lower = lower.replace('"', "");
            if lower.contains("phonetic")
                || lower.contains("doulos")
                || lower.contains("charis")
                || lower.contains("ipa")
            {
                style.font = FontRole::Phonetic;
            } else if lower.contains("mono") || lower.contains("consolas") {
                style.font = FontRole::Mono;
            } else if lower.contains("serif") || lower.contains("songti") || lower.contains("times")
            {
                style.font = FontRole::Serif;
            }
        }
        _ => {}
    }
}

fn size_delta(factor: f32) -> i8 {
    if factor <= 0.72 {
        -3
    } else if factor <= 0.9 {
        -1
    } else if factor >= 1.6 {
        4
    } else if factor >= 1.25 {
        2
    } else if factor >= 1.08 {
        1
    } else {
        0
    }
}

fn map_color(value: &str) -> Option<SemanticColor> {
    let v = value.trim().to_ascii_lowercase();
    if v.is_empty() || v == "inherit" || v == "transparent" || v == "currentcolor" {
        return None;
    }
    if let Some(hex) = v.strip_prefix('#') {
        let rgb = match hex.len() {
            3 => hex
                .chars()
                .map(|c| c.to_digit(16).unwrap_or(0) * 17)
                .collect::<Vec<_>>(),
            6 => hex
                .as_bytes()
                .chunks(2)
                .map(|pair| {
                    std::str::from_utf8(pair)
                        .ok()
                        .and_then(|s| u8::from_str_radix(s, 16).ok())
                        .map(u32::from)
                        .unwrap_or(0)
                })
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        };
        if rgb.len() == 3 {
            return classify_color(
                rgb[0] as f32 / 255.0,
                rgb[1] as f32 / 255.0,
                rgb[2] as f32 / 255.0,
            );
        }
    }
    if let Some(inner) = v.strip_prefix("rgb(").and_then(|s| s.strip_suffix(')')) {
        let parts: Vec<f32> = inner
            .split([',', ' ', '/'])
            .filter_map(|p| p.trim().trim_end_matches('%').parse::<f32>().ok())
            .map(|n| if n > 1.0 { n / 255.0 } else { n })
            .collect();
        if parts.len() >= 3 {
            return classify_color(parts[0], parts[1], parts[2]);
        }
    }
    match v.as_str() {
        "red" | "darkred" | "crimson" | "orange" | "orangered" | "brown" | "maroon" => {
            Some(SemanticColor::Warning)
        }
        "blue" | "darkblue" | "navy" | "royalblue" => Some(SemanticColor::Link),
        "gray" | "grey" | "silver" | "lightgray" | "darkgray" => Some(SemanticColor::Muted),
        "green" | "darkgreen" | "olive" => Some(SemanticColor::Accent),
        "purple" | "violet" | "magenta" | "fuchsia" => Some(SemanticColor::Pos),
        "teal" | "cyan" | "aqua" => Some(SemanticColor::Phonetic),
        "white" | "#fff" | "#ffffff" | "#fafafa" | "#f9f9f9" => None,
        _ => None,
    }
}

fn classify_color(r: f32, g: f32, b: f32) -> Option<SemanticColor> {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    if max < 0.22 && delta < 0.08 {
        return None;
    }
    if delta < 0.08 {
        return if max < 0.75 {
            Some(SemanticColor::Muted)
        } else {
            None
        };
    }
    let hue = if max == r {
        ((g - b) / delta).rem_euclid(6.0)
    } else if max == g {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    } * 60.0;
    if hue < 20.0 || hue >= 345.0 {
        Some(SemanticColor::Warning)
    } else if hue < 70.0 {
        Some(SemanticColor::Accent)
    } else if hue < 165.0 {
        Some(SemanticColor::Accent)
    } else if hue < 200.0 {
        Some(SemanticColor::Phonetic)
    } else if hue < 255.0 {
        Some(SemanticColor::Link)
    } else {
        Some(SemanticColor::Pos)
    }
}

struct CssRule {
    selector: SimpleSelector,
    decls: Vec<(String, String)>,
}

struct SimpleSelector {
    parts: Vec<SelectorPart>,
}

#[derive(Default)]
struct SelectorPart {
    tag: Option<String>,
    classes: Vec<String>,
    id: Option<String>,
    attrs: Vec<(String, String)>,
}

impl SimpleSelector {
    fn parse(input: &str) -> Self {
        let parts = input
            .split_whitespace()
            .map(|token| {
                let (main, bracket) = match token.split_once('[') {
                    Some((m, b)) => (m, Some(b.trim_end_matches(']'))),
                    None => (token, None),
                };
                let mut part = SelectorPart::default();
                let mut current = String::new();
                let mut kind = 't';
                for ch in main.chars().chain(std::iter::once(' ')) {
                    if ch == '.' || ch == '#' || ch == ' ' || ch == ':' {
                        if !current.is_empty() {
                            match kind {
                                't' => part.tag = Some(current.to_ascii_lowercase()),
                                '.' => part.classes.push(current.to_ascii_lowercase()),
                                '#' => part.id = Some(current.to_ascii_lowercase()),
                                _ => {}
                            }
                            current.clear();
                        }
                        kind = ch;
                    } else if ch != '>' && ch != '+' && ch != '~' {
                        current.push(ch);
                    }
                }
                if let Some(condition) = bracket {
                    if let Some((key, value)) = condition.split_once('=') {
                        part.attrs.push((
                            key.trim().to_ascii_lowercase(),
                            value.trim().trim_matches(['"', '\'']).to_string(),
                        ));
                    } else {
                        part.attrs
                            .push((condition.trim().to_ascii_lowercase(), String::new()));
                    }
                }
                part
            })
            .filter(|p: &SelectorPart| {
                p.tag.is_some() || !p.classes.is_empty() || p.id.is_some() || !p.attrs.is_empty()
            })
            .collect();
        Self { parts }
    }

    fn matches(&self, ancestors: &[Handle]) -> bool {
        if self.parts.is_empty() || ancestors.len() < self.parts.len() {
            return false;
        }
        let mut ancestor_idx = 0;
        for part in self.parts.iter().rev() {
            let mut found = false;
            while ancestor_idx < ancestors.len() {
                if part.matches_node(&ancestors[ancestor_idx]) {
                    found = true;
                    ancestor_idx += 1;
                    break;
                }
                ancestor_idx += 1;
            }
            if !found {
                return false;
            }
        }
        true
    }

    fn specificity(&self) -> (u8, u8, u8) {
        let mut ids = 0;
        let mut classes = 0;
        let mut tags = 0;
        for part in &self.parts {
            ids += usize::from(part.id.is_some());
            classes += part.classes.len() + part.attrs.len();
            tags += usize::from(part.tag.is_some());
        }
        (
            ids.min(255) as u8,
            classes.min(255) as u8,
            tags.min(255) as u8,
        )
    }
}

impl SelectorPart {
    fn matches_node(&self, node: &Handle) -> bool {
        if let Some(t) = &self.tag {
            if tag(node).as_deref() != Some(t.as_str()) {
                return false;
            }
        }
        if let Some(id) = &self.id {
            if attr(node, "id").as_deref() != Some(id.as_str()) {
                return false;
            }
        }
        let classes = class_list(node);
        if !self.classes.iter().all(|c| classes.iter().any(|x| x == c)) {
            return false;
        }
        self.attrs.iter().all(|(key, value)| {
            attr(node, key)
                .map(|actual| value.is_empty() || actual.eq_ignore_ascii_case(value))
                .unwrap_or(false)
        })
    }
}
