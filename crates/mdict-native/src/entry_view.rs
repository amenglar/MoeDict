use std::{ops::Range, sync::Arc};

use gpui::{
    AnyElement, App, Div, FontStyle, FontWeight, HighlightStyle, Hsla, IntoElement, ObjectFit,
    SharedString, Stateful, Styled, StyledText, TextStyle, UnderlineStyle, Window, div, img,
    prelude::*, px,
};
use mdict_core::DictionaryStore;

use crate::{
    ast::{DictBlock, DictEntry, LinkAction, RichText, StyledRun},
    resources::ImageCache,
    theme::Theme,
};

pub type ActionHandler = Arc<dyn Fn(LinkAction, &mut Window, &mut App) + 'static>;

pub fn render_entry(
    entry: &DictEntry,
    theme: &Theme,
    store: &Arc<DictionaryStore>,
    images: &ImageCache,
    dict_id: &str,
    on_action: ActionHandler,
) -> AnyElement {
    div()
        .id("entry-root")
        .flex()
        .flex_col()
        .gap(px(8.0))
        .text_size(px(theme.base_size))
        .text_color(theme.colors.text)
        .line_height(px(theme.base_size * 1.55))
        .children(
            entry.blocks.iter().enumerate().map(|(i, block)| {
                render_block(block, theme, store, images, dict_id, &on_action, i, 0)
            }),
        )
        .into_any_element()
}

fn render_block(
    block: &DictBlock,
    theme: &Theme,
    store: &Arc<DictionaryStore>,
    images: &ImageCache,
    dict_id: &str,
    on_action: &ActionHandler,
    index: usize,
    depth: u8,
) -> AnyElement {
    match block {
        DictBlock::Headword {
            text,
            phonetics,
            pos,
        } => {
            let mut row = div()
                .flex()
                .flex_row()
                .flex_wrap()
                .items_baseline()
                .gap(px(10.0))
                .pb(px(6.0))
                .border_b_1()
                .border_color(theme.colors.border);
            row = row.child(
                div()
                    .text_size(px(theme.size(6)))
                    .font_weight(FontWeight::BOLD)
                    .text_color(theme.colors.headword)
                    .child(text.clone()),
            );
            for phonetic in phonetics {
                row = row.child(
                    div()
                        .text_size(px(theme.size(-1)))
                        .text_color(theme.colors.phonetic)
                        .font_family(theme.colors.font(crate::ast::FontRole::Phonetic))
                        .child(phonetic.clone()),
                );
            }
            if let Some(pos) = pos {
                row = row.child(
                    div()
                        .px(px(8.0))
                        .py(px(2.0))
                        .rounded(px(6.0))
                        .bg(theme.colors.note_bg)
                        .text_size(px(theme.size(-2)))
                        .text_color(theme.colors.pos)
                        .child(pos.clone()),
                );
            }
            row.into_any_element()
        }
        DictBlock::Sense {
            number,
            definition,
            examples,
            children,
            level,
            ..
        } => {
            let indent = (*level as f32) * 14.0;
            let mut content = div().flex().flex_col().flex_1().gap(px(5.0));
            if !definition.is_empty() {
                content = content.child(render_rich_text(
                    definition,
                    theme,
                    on_action,
                    &format!("def-{index}-{depth}"),
                ));
            }
            for (i, example) in examples.iter().enumerate() {
                content = content.child(
                    div()
                        .flex()
                        .flex_col()
                        .px(px(10.0))
                        .py(px(6.0))
                        .rounded(px(6.0))
                        .bg(theme.colors.example_bg)
                        .border_l_2()
                        .border_color(theme.colors.note_border)
                        .child(render_rich_text(
                            example,
                            theme,
                            on_action,
                            &format!("ex-{index}-{depth}-{i}"),
                        )),
                );
            }
            for (i, child) in children.iter().enumerate() {
                content = content.child(render_block(
                    child,
                    theme,
                    store,
                    images,
                    dict_id,
                    on_action,
                    index * 16 + i,
                    depth + 1,
                ));
            }
            let mut row = div().flex().flex_row().gap(px(8.0)).pl(px(indent));
            if let Some(number) = number {
                row = row.child(
                    div()
                        .w(px(28.0))
                        .text_color(theme.colors.muted)
                        .text_size(px(theme.size(-1)))
                        .child(number.clone()),
                );
            }
            row.child(content).into_any_element()
        }
        DictBlock::Example(rt) => div()
            .flex()
            .flex_col()
            .px(px(10.0))
            .py(px(6.0))
            .rounded(px(6.0))
            .bg(theme.colors.example_bg)
            .border_l_2()
            .border_color(theme.colors.note_border)
            .child(render_rich_text(
                rt,
                theme,
                on_action,
                &format!("ex-{index}"),
            ))
            .into_any_element(),
        DictBlock::CrossRef { label, action, .. } => div()
            .id(SharedString::from(format!("cross-{index}")))
            .cursor_pointer()
            .on_click({
                let action = action.clone();
                let on_action = on_action.clone();
                move |_, window, cx| on_action(action.clone(), window, cx)
            })
            .child(
                div()
                    .text_color(theme.colors.link)
                    .underline()
                    .child(label.clone()),
            )
            .into_any_element(),
        DictBlock::Note { label, content } => {
            let mut note = div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .px(px(12.0))
                .py(px(8.0))
                .rounded(px(8.0))
                .bg(theme.colors.note_bg)
                .border_l_2()
                .border_color(theme.colors.note_border);
            if let Some(label) = label {
                note = note.child(
                    div()
                        .text_size(px(theme.size(-1)))
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme.colors.accent)
                        .child(label.clone()),
                );
            }
            note.child(render_rich_text(
                content,
                theme,
                on_action,
                &format!("note-{index}"),
            ))
            .into_any_element()
        }
        DictBlock::Image { path, caption } => {
            let mut column = div().flex().flex_col().gap(px(4.0)).items_center();
            if let Some(image) = images.get(store, dict_id, path) {
                column = column.child(
                    img(image)
                        .max_w(px(480.0))
                        .max_h(px(360.0))
                        .object_fit(ObjectFit::Contain),
                );
            } else {
                column = column.child(
                    div()
                        .text_size(px(theme.size(-2)))
                        .text_color(theme.colors.muted)
                        .child(format!("[图片: {path}]")),
                );
            }
            if let Some(caption) = caption {
                column = column.child(
                    div()
                        .text_size(px(theme.size(-2)))
                        .text_color(theme.colors.muted)
                        .child(caption.clone()),
                );
            }
            column.into_any_element()
        }
        DictBlock::Table { rows, header } => div()
            .flex()
            .flex_col()
            .rounded(px(8.0))
            .border_1()
            .border_color(theme.colors.border)
            .overflow_hidden()
            .children(rows.iter().enumerate().map(|(r, row)| {
                div()
                    .flex()
                    .flex_row()
                    .border_color(theme.colors.border)
                    .when(r + 1 < rows.len(), |el| el.border_b_1())
                    .children(row.iter().enumerate().map(|(c, cell)| {
                        div()
                            .flex_1()
                            .px(px(8.0))
                            .py(px(5.0))
                            .when(*header && r == 0, |el| {
                                el.bg(theme.colors.note_bg).font_weight(FontWeight::BOLD)
                            })
                            .child(render_rich_text(
                                cell,
                                theme,
                                on_action,
                                &format!("cell-{index}-{r}-{c}"),
                            ))
                    }))
            }))
            .into_any_element(),
        DictBlock::List { ordered, items } => {
            div()
                .flex()
                .flex_col()
                .gap(px(3.0))
                .pl(px(16.0))
                .children(items.iter().enumerate().map(|(i, item)| {
                    div()
                        .flex()
                        .flex_row()
                        .gap(px(6.0))
                        .child(div().w(px(16.0)).text_color(theme.colors.muted).child(
                            if *ordered {
                                format!("{}.", i + 1)
                            } else {
                                "•".to_string()
                            },
                        ))
                        .child(render_rich_text(
                            item,
                            theme,
                            on_action,
                            &format!("li-{index}-{i}"),
                        ))
                }))
                .into_any_element()
        }
        DictBlock::Paragraph(rt) | DictBlock::Fallback(rt) => {
            render_rich_text(rt, theme, on_action, &format!("p-{index}"))
        }
        DictBlock::Divider => div()
            .my(px(6.0))
            .border_b_1()
            .border_color(theme.colors.border)
            .into_any_element(),
    }
}

fn append_plain(
    container: Stateful<Div>,
    plain: &mut String,
    ranges: &mut Vec<(Range<usize>, HighlightStyle)>,
    theme: &Theme,
) -> Stateful<Div> {
    if plain.is_empty() {
        return container;
    }
    let text = std::mem::take(plain);
    let highlights = std::mem::take(ranges);
    container.child(
        StyledText::new(text)
            .with_default_highlights(&base_style(theme, theme.colors.text), highlights),
    )
}

fn base_style(theme: &Theme, color: Hsla) -> TextStyle {
    TextStyle {
        font_size: px(theme.base_size).into(),
        line_height: px(theme.base_size * 1.55).into(),
        color,
        ..Default::default()
    }
}

fn render_rich_text(
    rt: &RichText,
    theme: &Theme,
    on_action: &ActionHandler,
    id: &str,
) -> AnyElement {
    let mut container = div()
        .id(SharedString::from(id.to_string()))
        .flex()
        .flex_row()
        .flex_wrap()
        .items_baseline();
    let mut plain = String::new();
    let mut ranges: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
    let mut segment = 0usize;

    for run in &rt.runs {
        if run.action.is_none() {
            let start = plain.len();
            plain.push_str(&run.text);
            ranges.push((start..plain.len(), highlight_for(run, theme)));
            continue;
        }
        container = append_plain(container, &mut plain, &mut ranges, theme);
        let action = run.action.clone().unwrap();
        let styled = StyledText::new(run.text.clone()).with_default_highlights(
            &base_style(theme, theme.colors.link),
            [(0..run.text.len(), highlight_for_link(run, theme))],
        );
        container = container.child(
            div()
                .id(SharedString::from(format!("{id}-link-{segment}")))
                .cursor_pointer()
                .on_click({
                    let action = action.clone();
                    let on_action = on_action.clone();
                    move |_, window, cx| on_action(action.clone(), window, cx)
                })
                .child(styled),
        );
        segment += 1;
    }
    append_plain(container, &mut plain, &mut ranges, theme).into_any_element()
}

fn highlight_for(run: &StyledRun, theme: &Theme) -> HighlightStyle {
    let mut style = HighlightStyle::default();
    if run.style.bold {
        style.font_weight = Some(FontWeight::BOLD);
    }
    if run.style.italic {
        style.font_style = Some(FontStyle::Italic);
    }
    if run.style.underline {
        style.underline = Some(UnderlineStyle {
            thickness: px(1.0),
            color: None,
            wavy: false,
        });
    }
    if run.style.strike {
        style.strikethrough = Some(gpui::StrikethroughStyle {
            thickness: px(1.0),
            color: None,
        });
    }
    if let Some(color) = run.style.color {
        style.color = Some(theme.colors.semantic(color));
    }
    if let Some(background) = run.style.background {
        style.background_color = Some(theme.colors.semantic(background));
    }
    style
}

fn highlight_for_link(run: &StyledRun, theme: &Theme) -> HighlightStyle {
    let mut style = highlight_for(run, theme);
    style.color = Some(theme.colors.link);
    style.underline = Some(UnderlineStyle {
        thickness: px(1.0),
        color: None,
        wavy: false,
    });
    style
}
