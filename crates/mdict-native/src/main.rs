use std::sync::Arc;

use gpui::{App, AppContext, Application, KeyBinding, WindowBounds, WindowOptions, px, size};
use gpui_component::Root;
use mdict_core::DictionaryStore;

use moedict_native::{
    app::{
        AddFiles, AddFolder, AppView, CloseOverlay, FocusSearch, GoBack, GoForward, Lucky,
        OpenSettings, ToggleNav,
    },
    paths,
    theme::ThemeId,
};

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let options = paths::parse_args();
    let store = Arc::new(
        DictionaryStore::new(&paths::config_dir(), &paths::data_dir())
            .expect("failed to initialize dictionary store"),
    );

    if !options.mdx_dirs.is_empty() {
        match store.add_paths(&options.mdx_dirs) {
            Ok(outcome) => log::info!("startup added {} dictionaries", outcome.added.len()),
            Err(e) => log::warn!("startup add failed: {e}"),
        }
    }

    let app = Application::new();
    if let Err(e) = app.text_system().add_fonts(vec![std::borrow::Cow::Borrowed(
        &include_bytes!("../../../ui/fonts/KingsoftPhoneticPlain.ttf")[..],
    )]) {
        log::warn!("failed to register phonetic font: {e}");
    }

    let settings = store.settings();
    let system_dark = false;
    let theme_id = ThemeId::from_settings(&settings.theme, system_dark);
    let follow_system = settings.theme == "system";

    app.run(move |cx: &mut App| {
        gpui_component::init(cx);
        cx.bind_keys([
            KeyBinding::new("ctrl-l", FocusSearch, None),
            KeyBinding::new("ctrl-k", FocusSearch, None),
            KeyBinding::new("cmd-l", FocusSearch, None),
            KeyBinding::new("cmd-k", FocusSearch, None),
            KeyBinding::new("ctrl-comma", OpenSettings, None),
            KeyBinding::new("cmd-comma", OpenSettings, None),
            KeyBinding::new("ctrl-o", AddFiles, None),
            KeyBinding::new("cmd-o", AddFiles, None),
            KeyBinding::new("ctrl-shift-o", AddFolder, None),
            KeyBinding::new("cmd-shift-o", AddFolder, None),
            KeyBinding::new("ctrl-shift-n", ToggleNav, None),
            KeyBinding::new("cmd-shift-n", ToggleNav, None),
            KeyBinding::new("alt-left", GoBack, None),
            KeyBinding::new("alt-right", GoForward, None),
            KeyBinding::new("escape", CloseOverlay, None),
            KeyBinding::new("f2", Lucky, None),
        ]);

        let store_for_window = store.clone();
        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(1180.0), px(760.0)), cx)),
            titlebar: Some(gpui::TitlebarOptions {
                title: Some("Moe 词典 · Native".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        cx.open_window(window_options, |window, cx| {
            let view = cx.new(|cx| {
                AppView::new(
                    window,
                    cx,
                    store_for_window.clone(),
                    theme_id,
                    follow_system,
                )
            });
            view.update(cx, |view, cx| view.initialize(cx));
            cx.new(|cx| Root::new(view, window, cx))
        })
        .expect("failed to open main window");
    });
}
