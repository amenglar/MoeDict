use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};

use gpui::{
    App, AppContext, ClipboardItem, Context, Entity, ExternalPaths, FontStyle, FontWeight,
    IntoElement, ParentElement, Render, SharedString, Stateful, Styled, Subscription, Task,
    TextStyle, WeakEntity, Window, actions, div, prelude::*, px,
};
use gpui_component::input::{Input, InputEvent, InputState};
use mdict_core::{DictStatus, DictionaryInfo, DictionaryStore, Settings, Suggestion};

use crate::{
    ast::{DictEntry, LinkAction},
    entry_view::render_entry,
    html::parse_entry_with_css,
    indexer::{IndexEvent, IndexManager, ProgressInfo},
    resources::{AudioPlayer, ImageCache},
    theme::{Theme, ThemeId},
};

actions!(
    moedict,
    [
        FocusSearch,
        OpenSettings,
        AddFiles,
        AddFolder,
        ToggleNav,
        GoBack,
        GoForward,
        CloseOverlay,
        Lucky,
    ]
);

#[derive(Clone, Copy)]
enum PendingAction {
    FocusSearch,
    OpenSettings,
    AddFiles,
    AddFolder,
    ToggleNav,
    Back,
    Forward,
    Close,
    Lucky,
}

#[derive(Clone, Copy)]
enum ToastKind {
    Info,
    Success,
    Warn,
    Error,
}

#[derive(Clone)]
struct Toast {
    id: usize,
    message: String,
    kind: ToastKind,
}

enum Dialog {
    None,
    Settings {
        theme: ThemeId,
        follow_system: bool,
        font_size: u16,
        suggestion_limit: usize,
        default_expanded: usize,
        auto_index: bool,
        show_all_dicts: bool,
    },
    Rename {
        id: String,
    },
    Remove {
        id: String,
        name: String,
    },
}

#[derive(Clone)]
struct Hit {
    dict_id: String,
    dict_name: String,
    headword: String,
    entry: DictEntry,
    collapsed: bool,
}

pub struct AppView {
    store: Arc<DictionaryStore>,
    indexer: Arc<IndexManager>,
    images: ImageCache,
    audio: AudioPlayer,
    search: Entity<InputState>,
    filter: Entity<InputState>,
    rename_input: Option<Entity<InputState>>,
    settings: Settings,
    dicts: Vec<DictionaryInfo>,
    hits: Vec<Hit>,
    suggestions: Vec<Suggestion>,
    suggestions_open: bool,
    active_suggestion: usize,
    history: Vec<String>,
    nav: Vec<String>,
    nav_index: usize,
    nav_pane: bool,
    selected_tags: Vec<String>,
    has_queried: bool,
    status: String,
    progress: HashMap<String, ProgressInfo>,
    theme_id: ThemeId,
    follow_system: bool,
    dialog: Dialog,
    menu_for: Option<String>,
    toasts: Vec<Toast>,
    next_toast: usize,
    suggest_task: Option<Task<()>>,
    pending: Option<PendingAction>,
    _subs: Vec<Subscription>,
}

impl AppView {
    pub fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        store: Arc<DictionaryStore>,
        theme_id: ThemeId,
        follow_system: bool,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("输入单词或词组…"));
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("筛选词典…"));
        let sub = cx.subscribe_in(&search, window, {
            let search = search.clone();
            move |this: &mut Self, _, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    let value = search.read(cx).value().to_string();
                    this.on_search_changed(value, cx);
                }
                InputEvent::PressEnter { .. } => {
                    let value = search.read(cx).value().to_string();
                    this.run_search(value, None, window, cx);
                }
                _ => {}
            }
        });
        let view = Self {
            indexer: Arc::new(IndexManager::new(store.clone())),
            store,
            images: ImageCache::default(),
            audio: AudioPlayer::new(),
            search,
            filter,
            rename_input: None,
            settings: Settings::default(),
            dicts: Vec::new(),
            hits: Vec::new(),
            suggestions: Vec::new(),
            suggestions_open: false,
            active_suggestion: 0,
            history: Vec::new(),
            nav: Vec::new(),
            nav_index: 0,
            nav_pane: false,
            selected_tags: Vec::new(),
            has_queried: false,
            status: String::new(),
            progress: HashMap::new(),
            theme_id,
            follow_system,
            dialog: Dialog::None,
            menu_for: None,
            toasts: Vec::new(),
            next_toast: 0,
            suggest_task: None,
            pending: None,
            _subs: vec![sub],
        };
        let entity = cx.entity();
        App::on_action(cx, move |_: &FocusSearch, cx: &mut App| {
            let _ = entity.update(cx, |this, cx| {
                this.pending = Some(PendingAction::FocusSearch);
                cx.notify();
            });
        });
        let entity = cx.entity();
        App::on_action(cx, move |_: &OpenSettings, cx: &mut App| {
            let _ = entity.update(cx, |this, cx| {
                this.pending = Some(PendingAction::OpenSettings);
                cx.notify();
            });
        });
        let entity = cx.entity();
        App::on_action(cx, move |_: &AddFiles, cx: &mut App| {
            let _ = entity.update(cx, |this, cx| {
                this.pending = Some(PendingAction::AddFiles);
                cx.notify();
            });
        });
        let entity = cx.entity();
        App::on_action(cx, move |_: &AddFolder, cx: &mut App| {
            let _ = entity.update(cx, |this, cx| {
                this.pending = Some(PendingAction::AddFolder);
                cx.notify();
            });
        });
        let entity = cx.entity();
        App::on_action(cx, move |_: &ToggleNav, cx: &mut App| {
            let _ = entity.update(cx, |this, cx| {
                this.pending = Some(PendingAction::ToggleNav);
                cx.notify();
            });
        });
        let entity = cx.entity();
        App::on_action(cx, move |_: &GoBack, cx: &mut App| {
            let _ = entity.update(cx, |this, cx| {
                this.pending = Some(PendingAction::Back);
                cx.notify();
            });
        });
        let entity = cx.entity();
        App::on_action(cx, move |_: &GoForward, cx: &mut App| {
            let _ = entity.update(cx, |this, cx| {
                this.pending = Some(PendingAction::Forward);
                cx.notify();
            });
        });
        let entity = cx.entity();
        App::on_action(cx, move |_: &CloseOverlay, cx: &mut App| {
            let _ = entity.update(cx, |this, cx| {
                this.pending = Some(PendingAction::Close);
                cx.notify();
            });
        });
        let entity = cx.entity();
        App::on_action(cx, move |_: &Lucky, cx: &mut App| {
            let _ = entity.update(cx, |this, cx| {
                this.pending = Some(PendingAction::Lucky);
                cx.notify();
            });
        });
        view
    }

    pub fn initialize(&mut self, cx: &mut Context<Self>) {
        self.refresh(cx);
        self.apply_theme(cx);
        cx.spawn(
            async move |this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(150))
                        .await;
                    let _ = this.update(cx, |this, cx| this.poll_indexer(cx));
                }
            },
        )
        .detach();
    }

    fn theme(&self) -> Theme {
        Theme::new(self.theme_id, self.settings.font_size)
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.dicts = self.store.list();
        self.settings = self.store.settings();
        cx.notify();
    }

    fn apply_theme(&mut self, cx: &mut Context<Self>) {
        if self.follow_system {
            let dark = matches!(
                cx.window_appearance(),
                gpui::WindowAppearance::Dark | gpui::WindowAppearance::VibrantDark
            );
            self.theme_id = if dark { ThemeId::Dark } else { ThemeId::Light };
        }
        cx.notify();
    }

    fn poll_indexer(&mut self, cx: &mut Context<Self>) {
        let events = self.indexer.poll();
        for event in events {
            match event {
                IndexEvent::Progress(info) => {
                    self.progress.insert(info.id.clone(), info);
                    cx.notify();
                }
                IndexEvent::Done {
                    id,
                    ok,
                    cancelled,
                    error,
                    ..
                } => {
                    self.progress.remove(&id);
                    if cancelled {
                        self.toast("索引已取消", ToastKind::Info, cx);
                    } else if ok {
                        self.toast("索引完成", ToastKind::Success, cx);
                    } else {
                        self.toast(
                            format!("索引失败: {}", error.unwrap_or_default()),
                            ToastKind::Error,
                            cx,
                        );
                    }
                    self.refresh(cx);
                }
            }
        }
    }

    fn toast(&mut self, message: impl Into<String>, kind: ToastKind, cx: &mut Context<Self>) {
        self.next_toast += 1;
        let id = self.next_toast;
        self.toasts.push(Toast {
            id,
            message: message.into(),
            kind,
        });
        cx.spawn(
            async move |this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                cx.background_executor().timer(Duration::from_secs(4)).await;
                let _ = this.update(cx, |this, cx| {
                    this.toasts.retain(|t| t.id != id);
                    cx.notify();
                });
            },
        )
        .detach();
        cx.notify();
    }

    fn on_search_changed(&mut self, value: String, cx: &mut Context<Self>) {
        self.suggest_task = None;
        self.suggest_task = Some(cx.spawn(
            async move |this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                cx.background_executor()
                    .timer(Duration::from_millis(140))
                    .await;
                let _ = this.update(cx, |this, cx| this.fetch_suggestions(&value, cx));
            },
        ));
    }

    fn fetch_suggestions(&mut self, prefix: &str, cx: &mut Context<Self>) {
        let current = self.search.read(cx).value();
        if current.trim() != prefix.trim() {
            return;
        }
        let prefix = prefix.trim();
        self.suggestions = if prefix.is_empty() {
            Vec::new()
        } else {
            self.store.suggest_with_tags(prefix, &self.selected_tags)
        };
        self.suggestions_open = !self.suggestions.is_empty();
        self.active_suggestion = 0;
        cx.notify();
    }


    fn external_css(&self, html: &str, dict_id: &str) -> Option<String> {
        let lower = html.to_ascii_lowercase();
        let mut css = String::new();
        let mut offset = 0usize;
        while let Some(pos) = lower[offset..].find("<link") {
            let start = offset + pos;
            let end = lower[start..]
                .find('>')
                .map(|e| start + e)
                .unwrap_or(lower.len());
            let tag = &html[start..end];
            let tag_lower = tag.to_ascii_lowercase();
            if tag_lower.contains("stylesheet") {
                if let Some(href_pos) = tag_lower.find("href=") {
                    let rest = tag[href_pos + 5..].trim_start();
                    if let Some(quote) = rest.chars().next().filter(|c| *c == '"' || *c == '\'') {
                        let value = &rest[1..];
                        let path = &value[..value.find(quote).unwrap_or(value.len())];
                        if !path.is_empty() {
                            if let Ok(Some((bytes, _))) = self.store.read_resource(dict_id, path) {
                                css.push_str(&String::from_utf8_lossy(&bytes));
                                css.push('\n');
                            }
                        }
                    }
                }
            }
            offset = end + 1;
            if offset >= html.len() {
                break;
            }
        }
        if css.is_empty() { None } else { Some(css) }
    }

    fn set_search_value(&self, value: String, window: &mut Window, cx: &mut Context<Self>) {
        let input = self.search.clone();
        input.update(cx, |state, cx| state.set_value(value, window, cx));
    }

    fn run_search(
        &mut self,
        word: String,
        dict_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let word = word.trim().to_string();
        if word.is_empty() {
            return;
        }
        self.suggestions_open = false;
        self.set_search_value(word.clone(), window, cx);
        if self.history.first().map(String::as_str) != Some(word.as_str()) {
            self.history.retain(|h| !h.eq_ignore_ascii_case(&word));
            self.history.insert(0, word.clone());
            self.history.truncate(24);
        }
        if self.nav.get(self.nav_index).map(String::as_str) != Some(word.as_str()) {
            self.nav.truncate(self.nav_index + 1);
            self.nav.push(word.clone());
            self.nav_index = self.nav.len() - 1;
            if self.nav.len() > 50 {
                self.nav.remove(0);
                self.nav_index -= 1;
            }
        }
        let started = std::time::Instant::now();
        let default_expanded = self.settings.default_expanded;
        self.hits = self
            .store
            .query(&word)
            .into_iter()
            .enumerate()
            .map(|(i, hit)| Hit {
                dict_id: hit.dict_id.clone(),
                dict_name: hit.dict_name,
                headword: hit.headword,
                entry: {
                    let css = self.external_css(&hit.html, &hit.dict_id);
                    parse_entry_with_css(&hit.html, css.as_deref())
                },
                collapsed: i >= default_expanded,
            })
            .collect();
        if let Some(id) = dict_id {
            if let Some(pos) = self.hits.iter().position(|h| h.dict_id == id) {
                self.active_suggestion = pos;
            }
        }
        self.has_queried = true;
        self.status = format!(
            "在 {} 本词典中找到结果 · {} ms",
            self.hits.len(),
            started.elapsed().as_millis()
        );
        cx.notify();
    }

    fn lucky(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.store.lucky_with_tags(&self.selected_tags) {
            Some(word) => self.run_search(word, None, window, cx),
            None => self.toast("没有启用的词典", ToastKind::Warn, cx),
        }
    }

    fn navigate(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let next = self.nav_index as isize + delta;
        if next < 0 || next >= self.nav.len() as isize {
            return;
        }
        self.nav_index = next as usize;
        let word = self.nav[self.nav_index].clone();
        self.run_search(word, None, window, cx);
    }

    fn reset_to_welcome(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_search_value(String::new(), window, cx);
        self.hits.clear();
        self.suggestions.clear();
        self.suggestions_open = false;
        self.has_queried = false;
        self.status.clear();
        cx.notify();
    }

    fn handle_action(&mut self, action: LinkAction, window: &mut Window, cx: &mut Context<Self>) {
        match action {
            LinkAction::Entry { word, dict_id, .. } => self.run_search(word, dict_id, window, cx),
            LinkAction::Sound { path } => {
                let Some(dict_id) = self.hits.first().map(|h| h.dict_id.clone()) else {
                    return;
                };
                if let Err(e) = self.audio.play(&self.store, &dict_id, &path) {
                    self.toast(e, ToastKind::Warn, cx);
                }
            }
            LinkAction::External { url } => {
                if url.starts_with("http://")
                    || url.starts_with("https://")
                    || url.starts_with("mailto:")
                {
                    cx.open_url(&url);
                }
            }
            LinkAction::Anchor { id } => {
                self.status = format!("锚点: {id}");
                cx.notify();
            }
        }
    }

    fn add_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        match self.store.add_paths(&paths) {
            Ok(outcome) => {
                let added = outcome.added.len();
                for info in outcome.added {
                    if info.status == DictStatus::Unindexed {
                        self.indexer.enqueue(info.id, info.name);
                    }
                }
                let skipped = outcome.skipped.len();
                self.refresh(cx);
                if added > 0 {
                    self.toast(format!("已添加 {added} 本词典"), ToastKind::Success, cx);
                }
                if skipped > 0 {
                    self.toast(format!("跳过 {skipped} 个文件"), ToastKind::Warn, cx);
                }
            }
            Err(e) => self.toast(format!("添加失败: {e}"), ToastKind::Error, cx),
        }
    }

    fn pick_paths(&mut self, files: bool, prompt: String, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(gpui::PathPromptOptions {
            files,
            directories: !files,
            multiple: files,
            prompt: Some(prompt.into()),
        });
        cx.spawn(
            async move |this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                if let Ok(Ok(Some(paths))) = rx.await {
                    let _ = this.update(cx, |this, cx| this.add_paths(paths, cx));
                }
            },
        )
        .detach();
    }

    fn reindex(&mut self, info: &DictionaryInfo, cx: &mut Context<Self>) {
        self.indexer.enqueue(info.id.clone(), info.name.clone());
        self.toast(
            format!("已加入索引队列: {}", info.name),
            ToastKind::Info,
            cx,
        );
    }

    fn save_settings(&mut self, cx: &mut Context<Self>) {
        let Dialog::Settings {
            theme,
            follow_system,
            font_size,
            suggestion_limit,
            default_expanded,
            auto_index,
            show_all_dicts,
        } = &self.dialog
        else {
            return;
        };
        let (
            theme,
            follow_system,
            font_size,
            suggestion_limit,
            default_expanded,
            auto_index,
            show_all_dicts,
        ) = (
            *theme,
            *follow_system,
            *font_size,
            *suggestion_limit,
            *default_expanded,
            *auto_index,
            *show_all_dicts,
        );
        let mut settings = self.settings.clone();
        settings.theme = theme.setting_value().to_string();
        settings.font_size = font_size;
        settings.suggestion_limit = suggestion_limit;
        settings.default_expanded = default_expanded;
        settings.auto_index = auto_index;
        settings.show_all_dicts = show_all_dicts;
        if let Err(e) = self.store.update_settings(settings.clone()) {
            self.toast(format!("保存失败: {e}"), ToastKind::Error, cx);
            return;
        }
        self.settings = settings;
        self.theme_id = theme;
        self.follow_system = follow_system;
        self.dialog = Dialog::None;
        self.apply_theme(cx);
        self.toast("设置已保存", ToastKind::Success, cx);
    }

    fn button(
        label: impl Into<SharedString>,
        theme: &Theme,
        action: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Stateful<gpui::Div> {
        let label: SharedString = label.into();
        div()
            .id(label.clone())
            .cursor_pointer()
            .on_click(move |_, window, cx: &mut App| action(window, cx))
            .px(px(10.0))
            .py(px(5.0))
            .rounded(px(6.0))
            .bg(theme.colors.elevated)
            .border_1()
            .border_color(theme.colors.border)
            .hover(|el| el.bg(theme.colors.note_bg))
            .child(label)
    }
}

impl Render for AppView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(action) = self.pending.take() {
            match action {
                PendingAction::FocusSearch => {
                    let search = self.search.clone();
                    search.update(cx, |state, cx| state.focus(window, cx));
                }
                PendingAction::OpenSettings => {
                    self.dialog = Dialog::Settings {
                        theme: self.theme_id,
                        follow_system: self.follow_system,
                        font_size: self.settings.font_size,
                        suggestion_limit: self.settings.suggestion_limit,
                        default_expanded: self.settings.default_expanded,
                        auto_index: self.settings.auto_index,
                        show_all_dicts: self.settings.show_all_dicts,
                    };
                }
                PendingAction::AddFiles => {
                    self.pick_paths(true, "选择 MDX 词典文件".to_string(), cx);
                }
                PendingAction::AddFolder => {
                    self.pick_paths(false, "选择包含 MDX 词典的文件夹".to_string(), cx);
                }
                PendingAction::ToggleNav => self.nav_pane = !self.nav_pane,
                PendingAction::Back => self.navigate(-1, window, cx),
                PendingAction::Forward => self.navigate(1, window, cx),
                PendingAction::Close => {
                    self.dialog = Dialog::None;
                    self.menu_for = None;
                    self.suggestions_open = false;
                }
                PendingAction::Lucky => self.lucky(window, cx),
            }
        }
        let theme = self.theme();
        let colors = theme.colors;
        let this = cx.entity();
        let mut root = div()
            .id("app-root")
            .size_full()
            .flex()
            .flex_col()
            .bg(colors.bg)
            .text_color(colors.text)
            .font_family(".SystemUIFont")
            .on_drop::<ExternalPaths>(move |paths, _, cx: &mut App| {
                let paths: Vec<PathBuf> = paths.paths().to_vec();
                let _ = this.update(cx, |this, cx| this.add_paths(paths, cx));
            });
        root = root.child(self.render_sidebar(&theme, cx));
        root = root.child(self.render_main(&theme, cx));
        if self.nav_pane {
            root = root.child(self.render_nav(&theme));
        }
        root = root.child(self.render_status(&theme));

        if !matches!(self.dialog, Dialog::None) || self.menu_for.is_some() {
            root = root.child(
                div()
                    .id("backdrop")
                    .absolute()
                    .inset_0()
                    .bg(gpui::hsla(0.0, 0.0, 0.0, 0.6))
                    .on_click({
                        let this = cx.entity();
                        move |_, _, cx: &mut App| {
                            let _ = this.update(cx, |this, cx| {
                                this.dialog = Dialog::None;
                                this.menu_for = None;
                                cx.notify();
                            });
                        }
                    }),
            );
        }
        match &self.dialog {
            Dialog::Settings { .. } => {
                root = root.child(self.render_settings(&theme, cx));
            }
            Dialog::Rename { id } => {
                let id = id.clone();
                root = root.child(self.render_rename(&theme, &id, cx));
            }
            Dialog::Remove { id, name } => {
                let id = id.clone();
                let name = name.clone();
                root = root.child(self.render_remove(&theme, &id, &name, cx));
            }
            Dialog::None => {}
        }
        if self.menu_for.is_some() {
            root = root.child(self.render_menu(&theme, window, cx));
        }
        if !self.toasts.is_empty() {
            root = root.child(self.render_toasts(&theme));
        }
        root
    }
}

impl AppView {
    fn render_sidebar(&self, theme: &Theme, cx: &mut Context<Self>) -> gpui::AnyElement {
        let c = theme.colors;
        let enabled = self.dicts.iter().filter(|d| d.enabled).count();
        let filter = self
            .filter
            .read(cx)
            .value()
            .to_string()
            .to_ascii_lowercase();
        let mut list = div().flex().flex_col().gap(px(2.0));
        for info in self.dicts.iter().filter(|d| {
            filter.is_empty()
                || d.name.to_ascii_lowercase().contains(&filter)
                || d.path.to_ascii_lowercase().contains(&filter)
        }) {
            let id = info.id.clone();
            let name = info.name.clone();
            let on = info.enabled;
            let status = status_label(info, self.progress.get(&id));
            list = list.child(
                div()
                    .id(SharedString::from(format!("dict-{id}")))
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .px(px(10.0))
                    .py(px(7.0))
                    .rounded(px(8.0))
                    .hover(|el| el.bg(c.elevated))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(6.0))
                            .child(Self::button(if on { "☑" } else { "☐" }, theme, {
                                let id = id.clone();
                                let entity = cx.entity();
                                move |_, cx: &mut App| {
                                    let _ = entity.update(cx, |this, cx| {
                                        if let Err(e) = this.store.set_enabled(&id, !on) {
                                            this.toast(
                                                format!("操作失败: {e}"),
                                                ToastKind::Error,
                                                cx,
                                            );
                                        }
                                        this.refresh(cx);
                                    });
                                }
                            }))
                            .child(
                                div()
                                    .id(SharedString::from(format!("jump-{id}")))
                                    .flex_1()
                                    .cursor_pointer()
                                    .on_click({
                                        let id = id.clone();
                                        let entity = cx.entity();
                                        move |_, _, cx: &mut App| {
                                            let _ = entity.update(cx, |this, cx| {
                                                if let Some(pos) =
                                                    this.hits.iter().position(|h| h.dict_id == id)
                                                {
                                                    this.active_suggestion = pos;
                                                    this.nav_pane = true;
                                                } else {
                                                    this.toast(
                                                        "当前结果中没有这本词典",
                                                        ToastKind::Info,
                                                        cx,
                                                    );
                                                }
                                                cx.notify();
                                            });
                                        }
                                    })
                                    .child(name),
                            )
                            .child(if info.has_mdd { "MDD" } else { "" })
                            .child(Self::button("⋯", theme, {
                                let id = id.clone();
                                let entity = cx.entity();
                                move |_, cx: &mut App| {
                                    let _ = entity.update(cx, |this, cx| {
                                        this.menu_for = Some(id.clone());
                                        cx.notify();
                                    });
                                }
                            })),
                    )
                    .child(
                        div()
                            .pl(px(20.0))
                            .text_size(px(11.0))
                            .text_color(status_color(info, theme))
                            .child(status),
                    ),
            );
        }
        div()
            .w(px(276.0))
            .h_full()
            .flex()
            .flex_col()
            .bg(c.panel)
            .border_r_1()
            .border_color(c.border)
            .child(
                div()
                    .px(px(16.0))
                    .py(px(14.0))
                    .border_b_1()
                    .border_color(c.border)
                    .child(
                        div()
                            .text_size(px(18.0))
                            .font_weight(FontWeight::BOLD)
                            .text_color(c.accent)
                            .child("Moe 词典"),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(c.muted)
                            .child("MDX Desktop · Native"),
                    ),
            )
            .child(
                div()
                    .px(px(12.0))
                    .py(px(8.0))
                    .child(Input::new(&self.filter))
                    .child(self.render_tag_chips(theme, cx)),
            )
            .child(
                div()
                    .id("dict-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .child(list),
            )
            .child(
                div()
                    .px(px(12.0))
                    .py(px(8.0))
                    .border_t_1()
                    .border_color(c.border)
                    .text_size(px(11.0))
                    .text_color(c.muted)
                    .child(format!("{enabled}/{} 词典启用", self.dicts.len())),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .gap(px(6.0))
                    .px(px(12.0))
                    .pb(px(12.0))
                    .child(Self::button("添加文件", theme, {
                        let e = cx.entity();
                        move |_, cx: &mut App| {
                            let _ = e.update(cx, |this, cx| {
                                this.pick_paths(true, "选择 MDX 词典文件".to_string(), cx)
                            });
                        }
                    }))
                    .child(Self::button("扫描文件夹", theme, {
                        let e = cx.entity();
                        move |_, cx: &mut App| {
                            let _ = e.update(cx, |this, cx| {
                                this.pick_paths(false, "选择包含 MDX 词典的文件夹".to_string(), cx)
                            });
                        }
                    }))
                    .child(Self::button("设置", theme, {
                        let e = cx.entity();
                        move |_, cx: &mut App| {
                            let _ = e.update(cx, |this, cx| {
                                this.dialog = Dialog::Settings {
                                    theme: this.theme_id,
                                    follow_system: this.follow_system,
                                    font_size: this.settings.font_size,
                                    suggestion_limit: this.settings.suggestion_limit,
                                    default_expanded: this.settings.default_expanded,
                                    auto_index: this.settings.auto_index,
                                    show_all_dicts: this.settings.show_all_dicts,
                                };
                                cx.notify();
                            });
                        }
                    })),
            )
            .into_any_element()
    }


    fn render_tag_chips(&self, theme: &Theme, cx: &mut Context<Self>) -> gpui::AnyElement {
        let c = theme.colors;
        let all = self.store.all_tags();
        let mut row = div()
            .id("tag-chips")
            .flex()
            .flex_row()
            .flex_wrap()
            .gap(px(6.0))
            .px(px(12.0))
            .pb(px(6.0));
        for tag in all {
            let active = self.selected_tags.contains(&tag);
            let label = tag.clone();
            row = row.child(
                div()
                    .id(SharedString::from(format!("tag-{tag}")))
                    .cursor_pointer()
                    .on_click({
                        let entity = cx.entity();
                        let tag = tag.clone();
                        move |_, _, cx: &mut App| {
                            let _ = entity.update(cx, |this, cx| {
                                if let Some(pos) = this.selected_tags.iter().position(|t| t == &tag) {
                                    this.selected_tags.remove(pos);
                                } else {
                                    this.selected_tags.push(tag.clone());
                                }
                                cx.notify();
                            });
                        }
                    })
                    .px(px(8.0))
                    .py(px(3.0))
                    .rounded(px(10.0))
                    .bg(if active { c.accent } else { c.elevated })
                    .text_color(if active { c.bg } else { c.text })
                    .text_size(px(11.0))
                    .child(label),
            );
        }
        row.into_any_element()
    }

    fn render_main(&mut self, theme: &Theme, cx: &mut Context<Self>) -> gpui::AnyElement {
        let c = theme.colors;
        let entity = cx.entity();
        let action: Arc<dyn Fn(LinkAction, &mut Window, &mut App) + 'static> =
            Arc::new(move |link, window: &mut Window, cx: &mut App| {
                let _ = entity.update(cx, |this, cx| this.handle_action(link, window, cx));
            });
        let search = self.search.clone();
        let suggestion_words = self.suggestions.clone();
        let mut main = div().flex().flex_1().flex_col().min_w(px(0.0)).child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .px(px(14.0))
                .py(px(10.0))
                .border_b_1()
                .border_color(c.border)
                .child(Self::button("←", theme, {
                    let e = cx.entity();
                    move |window, cx: &mut App| {
                        let _ = e.update(cx, |this, cx| this.navigate(-1, window, cx));
                    }
                }))
                .child(Self::button("→", theme, {
                    let e = cx.entity();
                    move |window, cx: &mut App| {
                        let _ = e.update(cx, |this, cx| this.navigate(1, window, cx));
                    }
                }))
                .child(div().flex_1().child(Input::new(&search)).on_key_down({
                    let e = cx.entity();
                    move |event: &gpui::KeyDownEvent, window, cx: &mut App| {
                        let key = event.keystroke.key.as_str();
                        if key == "escape" {
                            let _ = e.update(cx, |this, cx| {
                                this.suggestions_open = false;
                                this.reset_to_welcome(window, cx);
                            });
                        } else if key == "up" || key == "down" {
                            let delta = if key == "up" { -1isize } else { 1 };
                            let _ = e.update(cx, |this, cx| {
                                if !this.suggestions.is_empty() {
                                    this.suggestions_open = true;
                                    let len = this.suggestions.len() as isize;
                                    this.active_suggestion =
                                        (this.active_suggestion as isize + delta).rem_euclid(len)
                                            as usize;
                                    cx.notify();
                                }
                            });
                        } else if key == "tab" {
                            let _ = e.update(cx, |this, cx| {
                                if let Some(word) = this.suggestions.get(this.active_suggestion) {
                                    let word = word.word.clone();
                                    this.set_search_value(word, window, cx);
                                    this.suggestions_open = false;
                                    cx.notify();
                                }
                            });
                        }
                    }
                }))
                .child(Self::button("手气", theme, {
                    let e = cx.entity();
                    move |window, cx: &mut App| {
                        let _ = e.update(cx, |this, cx| this.lucky(window, cx));
                    }
                }))
                .child(Self::button("导航", theme, {
                    let e = cx.entity();
                    move |_, cx: &mut App| {
                        let _ = e.update(cx, |this, cx| {
                            this.nav_pane = !this.nav_pane;
                            cx.notify();
                        });
                    }
                })),
        );

        if self.suggestions_open && !suggestion_words.is_empty() {
            let mut rows = div().flex().flex_col();
            for (i, suggestion) in suggestion_words.iter().enumerate() {
                let word = suggestion.word.clone();
                let click_word = word.clone();
                let dicts = suggestion.dicts.join(" · ");
                rows = rows.child(
                    div()
                        .id(SharedString::from(format!("suggest-{i}")))
                        .cursor_pointer()
                        .on_click({
                            let e = cx.entity();
                            move |_, window, cx: &mut App| {
                                let _ = e.update(cx, |this, cx| {
                                    this.run_search(click_word.clone(), None, window, cx)
                                });
                            }
                        })
                        .px(px(14.0))
                        .py(px(6.0))
                        .when(i == self.active_suggestion, |el| el.bg(c.note_bg))
                        .child(word)
                        .child(div().text_size(px(11.0)).text_color(c.muted).child(dicts)),
                );
            }
            main = main.child(
                div()
                    .id("suggest-scroll")
                    .flex()
                    .flex_col()
                    .max_h(px(280.0))
                    .overflow_y_scroll()
                    .border_b_1()
                    .border_color(c.border)
                    .bg(c.elevated)
                    .child(rows),
            );
        }

        let results = if !self.has_queried {
            self.render_welcome(theme, cx)
        } else if self.hits.is_empty() {
            self.render_no_results(theme, cx)
        } else {
            let mut list = div().flex().flex_col().gap(px(12.0)).p(px(16.0));
            for (i, hit) in self.hits.iter().enumerate() {
                list = list.child(self.render_hit(i, hit, theme, &action, cx));
            }
            list.into_any_element()
        };
        main.child(
            div()
                .px(px(16.0))
                .py(px(6.0))
                .text_size(px(12.0))
                .text_color(c.muted)
                .child(self.status.clone()),
        )
        .child(
            div()
                .id("results-scroll")
                .flex_1()
                .overflow_y_scroll()
                .child(results),
        )
        .into_any_element()
    }

    fn render_hit(
        &self,
        index: usize,
        hit: &Hit,
        theme: &Theme,
        action: &Arc<dyn Fn(LinkAction, &mut Window, &mut App) + 'static>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let c = theme.colors;
        let mut card = div()
            .id(SharedString::from(format!("hit-{index}")))
            .flex()
            .flex_col()
            .rounded(px(10.0))
            .border_1()
            .border_color(c.border)
            .bg(c.panel)
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.0))
                    .px(px(14.0))
                    .py(px(9.0))
                    .bg(c.elevated)
                    .border_b_1()
                    .border_color(c.border)
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.0))
                            .text_color(c.muted)
                            .child(hit.dict_name.clone()),
                    )
                    .child(
                        div()
                            .font_weight(FontWeight::BOLD)
                            .text_color(c.headword)
                            .child(hit.headword.clone()),
                    )
                    .child(Self::button(
                        if hit.collapsed { "展开" } else { "收起" },
                        theme,
                        {
                            let e = cx.entity();
                            move |_, cx: &mut App| {
                                let _ = e.update(cx, |this, cx| {
                                    if let Some(h) = this.hits.get_mut(index) {
                                        h.collapsed = !h.collapsed;
                                    }
                                    cx.notify();
                                });
                            }
                        },
                    ))
                    .child(Self::button("复制", theme, {
                        let e = cx.entity();
                        move |_, cx: &mut App| {
                            let _ = e.update(cx, |this, cx| {
                                if let Some(h) = this.hits.get(index) {
                                    let text =
                                        format!("{}\n\n{}", h.headword, h.entry.plain_text());
                                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                                    this.toast("释义已复制", ToastKind::Success, cx);
                                }
                            });
                        }
                    })),
            );
        if !hit.collapsed {
            card = card.child(div().p(px(16.0)).child(render_entry(
                &hit.entry,
                theme,
                &self.store,
                &self.images,
                &hit.dict_id,
                action.clone(),
            )));
        }
        card.into_any_element()
    }

    fn render_welcome(&self, theme: &Theme, cx: &mut Context<Self>) -> gpui::AnyElement {
        let c = theme.colors;
        let enabled = self.dicts.iter().filter(|d| d.enabled).count();
        let mut column =
            div()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(16.0))
                .p(px(40.0))
                .child(
                    div()
                        .text_size(px(30.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(c.accent)
                        .child("查一个词，读懂世界"),
                )
                .child(div().text_size(px(14.0)).text_color(c.muted).child(
                    if self.dicts.is_empty() {
                        "先添加一本 MDX 词典，然后开始查词".to_string()
                    } else if enabled == 0 {
                        "当前没有启用的词典，请在侧栏启用".to_string()
                    } else {
                        format!("已启用 {enabled} 本词典，输入单词开始查询")
                    },
                ));
        column = column.child(Self::button("试试手气", theme, {
            let e = cx.entity();
            move |window, cx: &mut App| {
                let _ = e.update(cx, |this, cx| this.lucky(window, cx));
            }
        }));
        if !self.history.is_empty() {
            let mut chips = div()
                .flex()
                .flex_row()
                .flex_wrap()
                .justify_center()
                .gap(px(8.0));
            for word in self.history.iter().take(12).cloned() {
                let click_word = word.clone();
                chips = chips.child(
                    div()
                        .id(SharedString::from(format!("recent-{word}")))
                        .cursor_pointer()
                        .on_click({
                            let e = cx.entity();
                            move |_, window, cx: &mut App| {
                                let _ = e.update(cx, |this, cx| {
                                    this.run_search(click_word.clone(), None, window, cx)
                                });
                            }
                        })
                        .px(px(10.0))
                        .py(px(4.0))
                        .rounded(px(12.0))
                        .bg(c.elevated)
                        .border_1()
                        .border_color(c.border)
                        .child(word),
                );
            }
            column = column.child(chips);
        }
        column.into_any_element()
    }

    fn render_no_results(&self, theme: &Theme, cx: &mut Context<Self>) -> gpui::AnyElement {
        let c = theme.colors;
        div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(12.0))
            .p(px(40.0))
            .child(
                div()
                    .text_size(px(22.0))
                    .font_weight(FontWeight::BOLD)
                    .child("没有找到结果"),
            )
            .child(
                div()
                    .text_size(px(13.0))
                    .text_color(c.muted)
                    .child("试试其他拼写，或从最近搜索中选择"),
            )
            .child(Self::button("试试手气", theme, {
                let e = cx.entity();
                move |window, cx: &mut App| {
                    let _ = e.update(cx, |this, cx| this.lucky(window, cx));
                }
            }))
            .into_any_element()
    }

    fn render_nav(&self, theme: &Theme) -> gpui::AnyElement {
        let c = theme.colors;
        let mut pane = div()
            .w(px(176.0))
            .h_full()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .bg(c.panel)
            .border_l_1()
            .border_color(c.border)
            .p(px(10.0))
            .child(
                div()
                    .text_size(px(12.0))
                    .font_weight(FontWeight::BOLD)
                    .child("结果导航"),
            );
        for (i, hit) in self.hits.iter().enumerate() {
            pane = pane.child(
                div()
                    .id(SharedString::from(format!("nav-{i}")))
                    .cursor_pointer()
                    .px(px(8.0))
                    .py(px(5.0))
                    .rounded(px(6.0))
                    .when(i == self.active_suggestion, |el| el.bg(c.note_bg))
                    .child(hit.dict_name.clone()),
            );
        }
        pane.into_any_element()
    }

    fn render_status(&self, theme: &Theme) -> gpui::AnyElement {
        let c = theme.colors;
        let indexing = self.progress.len();
        div()
            .h(px(26.0))
            .flex()
            .flex_row()
            .items_center()
            .px(px(14.0))
            .border_t_1()
            .border_color(c.border)
            .bg(c.panel)
            .text_size(px(11.0))
            .text_color(c.muted)
            .child(if indexing > 0 {
                format!("索引中: {indexing} 本")
            } else {
                format!("{} 本词典", self.dicts.len())
            })
            .child(div().flex_1())
            .child(self.status.clone())
            .into_any_element()
    }

    fn render_toasts(&self, theme: &Theme) -> gpui::AnyElement {
        let c = theme.colors;
        let mut column = div()
            .absolute()
            .bottom(px(44.0))
            .right(px(20.0))
            .flex()
            .flex_col()
            .gap(px(8.0));
        for toast in &self.toasts {
            let color = match toast.kind {
                ToastKind::Info => c.link,
                ToastKind::Success => c.accent,
                ToastKind::Warn => c.note_border,
                ToastKind::Error => c.warning,
            };
            column = column.child(
                div()
                    .px(px(14.0))
                    .py(px(10.0))
                    .rounded(px(8.0))
                    .bg(c.elevated)
                    .border_l_3()
                    .border_color(color)
                    .child(toast.message.clone()),
            );
        }
        column.into_any_element()
    }

    fn render_menu(
        &self,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let c = theme.colors;
        let id = self.menu_for.clone().unwrap_or_default();
        let info = self.dicts.iter().find(|d| d.id == id);
        let pos = self.dicts.iter().position(|d| d.id == id).unwrap_or(0);
        let mut menu = div()
            .absolute()
            .top(px(120.0))
            .left(px(180.0))
            .w(px(180.0))
            .flex()
            .flex_col()
            .gap(px(2.0))
            .p(px(6.0))
            .rounded(px(10.0))
            .bg(c.elevated)
            .border_1()
            .border_color(c.border)
            .child(
                div()
                    .px(px(10.0))
                    .py(px(6.0))
                    .text_size(px(12.0))
                    .text_color(c.muted)
                    .child(info.map(|d| d.name.clone()).unwrap_or_default()),
            );
        menu = menu.child(Self::button("重命名…", theme, {
            let id = id.clone();
            let e = cx.entity();
            move |window, cx: &mut App| {
                let _ = e.update(cx, |this, cx| {
                    let input = cx.new(|cx| InputState::new(window, cx));
                    this.rename_input = Some(input.clone());
                    this.dialog = Dialog::Rename { id: id.clone() };
                    this.menu_for = None;
                    cx.notify();
                });
            }
        }));
        {
            menu = menu.child(Self::button("重建索引", theme, {
                let id = id.clone();
                let e = cx.entity();
                move |_, cx: &mut App| {
                    let _ = e.update(cx, |this, cx| {
                        let target = this.dicts.iter().find(|d| d.id == id).cloned();
                        if let Some(info) = target {
                            this.reindex(&info, cx);
                        }
                        this.menu_for = None;
                    });
                }
            }));
        }
        menu = menu.child(Self::button("上移", theme, {
            let e = cx.entity();
            move |_, cx: &mut App| {
                let _ = e.update(cx, |this, cx| {
                    if pos > 0 {
                        let mut ids: Vec<String> =
                            this.dicts.iter().map(|d| d.id.clone()).collect();
                        ids.swap(pos, pos - 1);
                        if let Err(err) = this.store.reorder(&ids) {
                            this.toast(format!("排序失败: {err}"), ToastKind::Error, cx);
                        }
                        this.refresh(cx);
                    }
                    this.menu_for = None;
                });
            }
        }));
        menu = menu.child(Self::button("下移", theme, {
            let e = cx.entity();
            move |_, cx: &mut App| {
                let _ = e.update(cx, |this, cx| {
                    if pos + 1 < this.dicts.len() {
                        let mut ids: Vec<String> =
                            this.dicts.iter().map(|d| d.id.clone()).collect();
                        ids.swap(pos, pos + 1);
                        if let Err(err) = this.store.reorder(&ids) {
                            this.toast(format!("排序失败: {err}"), ToastKind::Error, cx);
                        }
                        this.refresh(cx);
                    }
                    this.menu_for = None;
                });
            }
        }));
        menu = menu.child(Self::button("打开扢在文件夹", theme, {
            let id = id.clone();
            let e = cx.entity();
            move |_, cx: &mut App| {
                let _ = e.update(cx, |this, cx| {
                    if let Ok(path) = this.store.dictionary_dir(&id) {
                        cx.reveal_path(&path);
                    }
                    this.menu_for = None;
                });
            }
        }));
        menu = menu.child(Self::button("删除词典…", theme, {
            let id = id.clone();
            let e = cx.entity();
            move |_, cx: &mut App| {
                let _ = e.update(cx, |this, cx| {
                    let name = this
                        .dicts
                        .iter()
                        .find(|d| d.id == id)
                        .map(|d| d.name.clone())
                        .unwrap_or_default();
                    this.dialog = Dialog::Remove {
                        id: id.clone(),
                        name,
                    };
                    this.menu_for = None;
                    cx.notify();
                });
            }
        }));
        let _ = window;
        menu.into_any_element()
    }

    fn render_settings(&self, theme: &Theme, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Dialog::Settings {
            theme: selected,
            follow_system,
            font_size,
            suggestion_limit,
            default_expanded,
            auto_index,
            show_all_dicts,
        } = &self.dialog
        else {
            return div().into_any_element();
        };
        let c = theme.colors;
        let e = cx.entity();
        let mut body = div()
            .id("settings-scroll")
            .absolute()
            .left(px(320.0))
            .top(px(100.0))
            .w(px(460.0))
            .max_h(px(620.0))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .p(px(18.0))
            .rounded(px(12.0))
            .bg(c.elevated)
            .border_1()
            .border_color(c.border)
            .child(
                div()
                    .text_size(px(18.0))
                    .font_weight(FontWeight::BOLD)
                    .child("设置"),
            );
        let mut theme_row = div().flex().flex_row().gap(px(4.0));
        for id in ThemeId::all() {
            let active = selected == id;
            let label = id.label().to_string();
            let id = *id;
            theme_row = theme_row.child(
                div()
                    .id(SharedString::from(format!("theme-{id:?}")))
                    .cursor_pointer()
                    .on_click({
                        let e = e.clone();
                        move |_, _, cx: &mut App| {
                            let _ = e.update(cx, |this, cx| {
                                if let Dialog::Settings {
                                    theme,
                                    follow_system,
                                    ..
                                } = &mut this.dialog
                                {
                                    *theme = id;
                                    *follow_system = false;
                                }
                                cx.notify();
                            });
                        }
                    })
                    .px(px(10.0))
                    .py(px(4.0))
                    .rounded(px(6.0))
                    .bg(if active { c.accent } else { c.panel })
                    .text_color(if active { c.bg } else { c.text })
                    .child(label),
            );
        }
        body = body.child(setting_row("外观主题", theme_row));
        body = body.child(setting_row(
            "跟随系统",
            toggle(*follow_system, theme, {
                let e = e.clone();
                move |_, cx: &mut App| {
                    let _ = e.update(cx, |this, cx| {
                        if let Dialog::Settings { follow_system, .. } = &mut this.dialog {
                            *follow_system = !*follow_system;
                        }
                        cx.notify();
                    });
                }
            }),
        ));
        body = body.child(setting_row(
            format!("释义字号: {font_size}px"),
            stepper(
                theme,
                {
                    let e = e.clone();
                    move |_, cx: &mut App| {
                        let _ = e.update(cx, |this, cx| {
                            if let Dialog::Settings { font_size, .. } = &mut this.dialog {
                                *font_size = font_size.saturating_sub(1).max(10);
                            }
                            cx.notify();
                        });
                    }
                },
                {
                    let e = e.clone();
                    move |_, cx: &mut App| {
                        let _ = e.update(cx, |this, cx| {
                            if let Dialog::Settings { font_size, .. } = &mut this.dialog {
                                *font_size = (*font_size + 1).min(32);
                            }
                            cx.notify();
                        });
                    }
                },
            ),
        ));
        body = body.child(setting_row(
            format!("联想条数: {suggestion_limit}"),
            stepper(
                theme,
                {
                    let e = e.clone();
                    move |_, cx: &mut App| {
                        let _ = e.update(cx, |this, cx| {
                            if let Dialog::Settings {
                                suggestion_limit, ..
                            } = &mut this.dialog
                            {
                                *suggestion_limit = suggestion_limit.saturating_sub(1).max(1);
                            }
                            cx.notify();
                        });
                    }
                },
                {
                    let e = e.clone();
                    move |_, cx: &mut App| {
                        let _ = e.update(cx, |this, cx| {
                            if let Dialog::Settings {
                                suggestion_limit, ..
                            } = &mut this.dialog
                            {
                                *suggestion_limit = (*suggestion_limit + 1).min(50);
                            }
                            cx.notify();
                        });
                    }
                },
            ),
        ));
        body = body.child(setting_row(
            format!("默认展开: {default_expanded} 条"),
            stepper(
                theme,
                {
                    let e = e.clone();
                    move |_, cx: &mut App| {
                        let _ = e.update(cx, |this, cx| {
                            if let Dialog::Settings {
                                default_expanded, ..
                            } = &mut this.dialog
                            {
                                *default_expanded = default_expanded.saturating_sub(1);
                            }
                            cx.notify();
                        });
                    }
                },
                {
                    let e = e.clone();
                    move |_, cx: &mut App| {
                        let _ = e.update(cx, |this, cx| {
                            if let Dialog::Settings {
                                default_expanded, ..
                            } = &mut this.dialog
                            {
                                *default_expanded = (*default_expanded + 1).min(20);
                            }
                            cx.notify();
                        });
                    }
                },
            ),
        ));
        body = body.child(setting_row(
            "自动建立索引",
            toggle(*auto_index, theme, {
                let e = e.clone();
                move |_, cx: &mut App| {
                    let _ = e.update(cx, |this, cx| {
                        if let Dialog::Settings { auto_index, .. } = &mut this.dialog {
                            *auto_index = !*auto_index;
                        }
                        cx.notify();
                    });
                }
            }),
        ));
        body = body.child(setting_row(
            "显示未收录词典",
            toggle(*show_all_dicts, theme, {
                let e = e.clone();
                move |_, cx: &mut App| {
                    let _ = e.update(cx, |this, cx| {
                        if let Dialog::Settings { show_all_dicts, .. } = &mut this.dialog {
                            *show_all_dicts = !*show_all_dicts;
                        }
                        cx.notify();
                    });
                }
            }),
        ));
        body = body.child(
            div()
                .flex()
                .flex_row()
                .justify_end()
                .gap(px(8.0))
                .pt(px(10.0))
                .child(Self::button("取消", theme, {
                    let e = e.clone();
                    move |_, cx: &mut App| {
                        let _ = e.update(cx, |this, cx| {
                            this.dialog = Dialog::None;
                            cx.notify();
                        });
                    }
                }))
                .child(Self::button("保存", theme, {
                    let e = e.clone();
                    move |_, cx: &mut App| {
                        let _ = e.update(cx, |this, cx| this.save_settings(cx));
                    }
                })),
        );
        body.into_any_element()
    }

    fn render_rename(&self, theme: &Theme, id: &str, cx: &mut Context<Self>) -> gpui::AnyElement {
        let c = theme.colors;
        let e = cx.entity();
        let Some(input) = self.rename_input.clone() else {
            return div().into_any_element();
        };
        let id = id.to_string();
        dialog_frame(
            theme,
            "重命名词典",
            div()
                .flex()
                .flex_col()
                .gap(px(10.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(c.muted)
                        .child("仅修改显示名称，不会改动磁盘文件。"),
                )
                .child(Input::new(&input))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .justify_end()
                        .gap(px(8.0))
                        .child(Self::button("取消", theme, {
                            let e = e.clone();
                            move |_, cx: &mut App| {
                                let _ = e.update(cx, |this, cx| {
                                    this.dialog = Dialog::None;
                                    cx.notify();
                                });
                            }
                        }))
                        .child(Self::button("确定", theme, {
                            let e = e.clone();
                            let input = input.clone();
                            let id = id.clone();
                            move |_, cx: &mut App| {
                                let _ = e.update(cx, |this, cx| {
                                    let name = input.read(cx).value().to_string();
                                    if let Err(err) = this.store.rename(&id, &name) {
                                        this.toast(
                                            format!("重命名失败: {err}"),
                                            ToastKind::Error,
                                            cx,
                                        );
                                    } else {
                                        this.dialog = Dialog::None;
                                        this.refresh(cx);
                                        this.toast("已重命名", ToastKind::Success, cx);
                                    }
                                });
                            }
                        })),
                ),
        )
        .into_any_element()
    }

    fn render_remove(
        &self,
        theme: &Theme,
        id: &str,
        name: &str,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let c = theme.colors;
        let e = cx.entity();
        let id = id.to_string();
        dialog_frame(
            theme,
            &format!("删除词典「{name}」？"),
            div()
                .flex()
                .flex_col()
                .gap(px(10.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(c.muted)
                        .child("只删除配置与索引缓存，不会删除磁盘上的 .mdx 文件。"),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .justify_end()
                        .gap(px(8.0))
                        .child(Self::button("取消", theme, {
                            let e = e.clone();
                            move |_, cx: &mut App| {
                                let _ = e.update(cx, |this, cx| {
                                    this.dialog = Dialog::None;
                                    cx.notify();
                                });
                            }
                        }))
                        .child(Self::button("删除", theme, {
                            let e = e.clone();
                            let id = id.clone();
                            move |_, cx: &mut App| {
                                let _ = e.update(cx, |this, cx| {
                                    if let Err(err) = this.store.remove(&id) {
                                        this.toast(
                                            format!("删除失败: {err}"),
                                            ToastKind::Error,
                                            cx,
                                        );
                                    } else {
                                        this.dialog = Dialog::None;
                                        this.refresh(cx);
                                        this.toast("词典已移除", ToastKind::Success, cx);
                                    }
                                });
                            }
                        })),
                ),
        )
        .into_any_element()
    }
}

fn dialog_frame(theme: &Theme, title: &str, body: gpui::Div) -> gpui::Div {
    div()
        .absolute()
        .left(px(360.0))
        .top(px(180.0))
        .w(px(420.0))
        .flex()
        .flex_col()
        .gap(px(12.0))
        .p(px(18.0))
        .rounded(px(12.0))
        .bg(theme.colors.elevated)
        .border_1()
        .border_color(theme.colors.border)
        .child(div().font_weight(FontWeight::BOLD).child(title.to_string()))
        .child(body)
}

fn setting_row(label: impl Into<String>, control: impl IntoElement) -> gpui::Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .child(label.into())
        .child(control)
}

fn toggle(
    active: bool,
    theme: &Theme,
    action: impl Fn(&mut Window, &mut App) + 'static,
) -> Stateful<gpui::Div> {
    div()
        .id(if active { "toggle-on" } else { "toggle-off" })
        .cursor_pointer()
        .on_click(move |_, window, cx: &mut App| action(window, cx))
        .px(px(12.0))
        .py(px(4.0))
        .rounded(px(12.0))
        .bg(if active {
            theme.colors.accent
        } else {
            theme.colors.panel
        })
        .text_color(if active {
            theme.colors.bg
        } else {
            theme.colors.text
        })
        .child(if active { "开" } else { "关" })
}

fn stepper(
    theme: &Theme,
    minus: impl Fn(&mut Window, &mut App) + 'static,
    plus: impl Fn(&mut Window, &mut App) + 'static,
) -> gpui::Div {
    div()
        .flex()
        .flex_row()
        .gap(px(4.0))
        .child(AppView::button("−", theme, minus))
        .child(AppView::button("+", theme, plus))
}

fn status_label(info: &DictionaryInfo, progress: Option<&ProgressInfo>) -> String {
    if let Some(p) = progress {
        return format!("索引中 {}%", p.percent as u32);
    }
    match info.status {
        DictStatus::Ready => format!("已索引 · {} 词条", info.entry_count),
        DictStatus::Stale => "源文件已变化，需重建".to_string(),
        DictStatus::Unindexed => "未索引".to_string(),
        DictStatus::Missing => "文件缺失".to_string(),
        DictStatus::Error => info.error.clone().unwrap_or_else(|| "索引错误".to_string()),
    }
}

fn status_color(info: &DictionaryInfo, theme: &Theme) -> gpui::Hsla {
    match info.status {
        DictStatus::Ready | DictStatus::Unindexed => theme.colors.muted,
        DictStatus::Stale => theme.colors.note_border,
        DictStatus::Missing | DictStatus::Error => theme.colors.warning,
    }
}

#[allow(dead_code)]
fn _unused(_: TextStyle, _: FontStyle) {}
