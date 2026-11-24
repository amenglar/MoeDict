use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DictEntry {
    pub headword: String,
    pub blocks: Vec<DictBlock>,
}

impl DictEntry {
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    pub fn plain_text(&self) -> String {
        let mut out = String::new();
        for block in &self.blocks {
            block.push_plain_text(&mut out);
        }
        out
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum DictBlock {
    Headword {
        text: String,
        phonetics: Vec<String>,
        pos: Option<String>,
    },
    Sense {
        number: Option<String>,
        pos: Option<String>,
        definition: RichText,
        examples: Vec<RichText>,
        children: Vec<DictBlock>,
        level: u8,
    },
    Example(RichText),
    CrossRef {
        word: String,
        label: String,
        action: LinkAction,
    },
    Note {
        label: Option<String>,
        content: RichText,
    },
    Image {
        path: String,
        caption: Option<String>,
    },
    Table {
        rows: Vec<Vec<RichText>>,
        header: bool,
    },
    List {
        ordered: bool,
        items: Vec<RichText>,
    },
    Paragraph(RichText),
    Divider,
    Fallback(RichText),
}

impl DictBlock {
    fn push_plain_text(&self, out: &mut String) {
        match self {
            Self::Headword { text, .. } => {
                out.push_str(text);
                out.push('\n');
            }
            Self::Sense {
                number,
                definition,
                examples,
                children,
                ..
            } => {
                if let Some(n) = number {
                    out.push_str(n);
                    out.push(' ');
                }
                out.push_str(&definition.plain_text());
                out.push('\n');
                for ex in examples {
                    out.push_str(&ex.plain_text());
                    out.push('\n');
                }
                for child in children {
                    child.push_plain_text(out);
                }
            }
            Self::Example(rt) | Self::Paragraph(rt) | Self::Fallback(rt) => {
                out.push_str(&rt.plain_text());
                out.push('\n');
            }
            Self::CrossRef { label, .. } => {
                out.push_str(label);
                out.push('\n');
            }
            Self::Note { content, .. } => {
                out.push_str(&content.plain_text());
                out.push('\n');
            }
            Self::Image { caption, .. } => {
                if let Some(c) = caption {
                    out.push_str(c);
                    out.push('\n');
                }
            }
            Self::Table { rows, .. } => {
                for row in rows {
                    for cell in row {
                        out.push_str(&cell.plain_text());
                        out.push('\t');
                    }
                    out.push('\n');
                }
            }
            Self::List { items, .. } => {
                for item in items {
                    out.push_str(&item.plain_text());
                    out.push('\n');
                }
            }
            Self::Divider => out.push('\n'),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RichText {
    pub runs: Vec<StyledRun>,
}

impl RichText {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_plain(text: impl Into<String>) -> Self {
        Self {
            runs: vec![StyledRun::new(text)],
        }
    }

    pub fn push(
        &mut self,
        text: impl Into<String>,
        style: InlineStyle,
        action: Option<LinkAction>,
    ) {
        let text = text.into();
        if text.is_empty() {
            return;
        }
        if let Some(last) = self.runs.last_mut() {
            if last.style == style && last.action == action {
                last.text.push_str(&text);
                return;
            }
        }
        self.runs.push(StyledRun {
            text,
            style,
            action,
        });
    }

    pub fn is_empty(&self) -> bool {
        self.runs.iter().all(|r| r.text.trim().is_empty())
    }

    pub fn plain_text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }

    pub fn text(&self) -> String {
        self.plain_text()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StyledRun {
    pub text: String,
    pub style: InlineStyle,
    pub action: Option<LinkAction>,
}

impl StyledRun {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            style: InlineStyle::default(),
            action: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InlineStyle {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub hidden: bool,
    pub block: bool,
    pub color: Option<SemanticColor>,
    pub background: Option<SemanticColor>,
    pub font: FontRole,
    pub size_delta: i8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FontRole {
    #[default]
    Sans,
    Serif,
    Phonetic,
    Mono,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SemanticColor {
    Primary,
    Secondary,
    Muted,
    Accent,
    Headword,
    Phonetic,
    Pos,
    Example,
    Link,
    Warning,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LinkAction {
    Entry {
        word: String,
        dict_id: Option<String>,
        anchor: Option<String>,
    },
    Sound {
        path: String,
    },
    External {
        url: String,
    },
    Anchor {
        id: String,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PhoneticVariant {
    #[default]
    Ipa,
    Uk,
    Us,
}
