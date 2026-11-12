//! A single, independently closable Settings window (⌘,). The host registers `open`.

use gpui::{
    App, Bounds, Context, FocusHandle, Global, KeyBinding, Subscription, TitlebarOptions, Window,
    WindowBounds, WindowHandle, WindowOptions, actions, div, prelude::*, px, rgb, size,
};

use crate::{
    engine::SearchEngine,
    theme::{self, ThemeSettings},
};

actions!(settings, [CloseSettings]);

#[derive(Default)]
struct SettingsWindow {
    // GPUI window handles do not keep their window (or root entity) alive.
    handle: Option<WindowHandle<Settings>>,
}
impl Global for SettingsWindow {}

/// Open Settings, or bring its existing window forward.
pub fn open(cx: &mut App) {
    if !cx.has_global::<SettingsWindow>() {
        cx.set_global(SettingsWindow::default());
        cx.bind_keys([
            KeyBinding::new("escape", CloseSettings, Some("FelinautSettings")),
            KeyBinding::new("cmd-w", CloseSettings, Some("FelinautSettings")),
        ]);
    }

    if let Some(handle) = cx.global::<SettingsWindow>().handle {
        if handle
            .update(cx, |settings, window, _| {
                window.activate_window();
                window.focus(&settings.focus);
            })
            .is_ok()
        {
            cx.activate(true);
            return;
        }
        cx.global_mut::<SettingsWindow>().handle = None;
    }

    let bounds = Bounds::centered(None, size(px(460.), px(420.)), cx);
    match cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(400.), px(420.))),
            titlebar: Some(TitlebarOptions {
                title: Some("Settings".into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        |window, cx| cx.new(|cx| Settings::new(window, cx)),
    ) {
        Ok(handle) => {
            cx.global_mut::<SettingsWindow>().handle = Some(handle);
            cx.activate(true);
        }
        Err(error) => eprintln!("Could not open Settings: {error}"),
    }
}

/// The search engine rows in display order, each with whether it is the current choice.
fn search_rows(current: SearchEngine) -> Vec<(SearchEngine, bool)> {
    SearchEngine::ALL
        .into_iter()
        .map(|engine| (engine, engine == current))
        .collect()
}

/// Make `engine` the search engine for address-bar searches and save it.
fn choose_search(engine: SearchEngine, cx: &mut App) {
    cx.update_global::<ThemeSettings, _>(|settings, _| {
        if settings.search != engine {
            settings.search = engine;
            settings.save();
        }
    });
}

struct Settings {
    focus: FocusHandle,
    _theme_observer: Subscription,
}

impl Settings {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus);
        // Redraw for theme changes and for choices made elsewhere.
        let theme_observer = cx.observe_global::<ThemeSettings>(|_, cx| cx.notify());
        Self {
            focus,
            _theme_observer: theme_observer,
        }
    }

    fn close(&mut self, _: &CloseSettings, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        window.remove_window();
    }
}

impl Render for Settings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let current = cx.global::<ThemeSettings>().search;
        div()
            .id("felinaut-settings")
            .key_context("FelinautSettings")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::close))
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_6()
            .bg(rgb(p.background))
            .text_color(rgb(p.text))
            .font_family(".AppleSystemUIFont")
            .text_size(px(13.))
            .child(
                div()
                    .text_size(px(20.))
                    .font_weight(gpui::FontWeight::BOLD)
                    .child("Settings"),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .p_3()
                    .gap_1()
                    .rounded_xl()
                    .bg(rgb(p.surface))
                    .border_1()
                    .border_color(rgb(p.border))
                    .child(
                        div()
                            .h(px(22.))
                            .px_2()
                            .text_size(px(11.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgb(p.muted))
                            .child("SEARCH ENGINE"),
                    )
                    .child(
                        div()
                            .px_2()
                            .pb_1()
                            .text_size(px(12.))
                            .text_color(rgb(p.muted))
                            .child("Used when you type a search in the address bar."),
                    )
                    .children(search_rows(current).into_iter().enumerate().map(
                        |(index, (engine, active))| {
                            div()
                                .id(("search-engine", index))
                                .flex()
                                .items_center()
                                .gap_2()
                                .px_2()
                                .h(px(30.))
                                .rounded_lg()
                                .cursor_pointer()
                                .when(active, |row| row.bg(rgb(p.elevated)))
                                .hover(move |style| style.bg(rgb(p.elevated)))
                                .on_click(move |_, _, cx| choose_search(engine, cx))
                                .child(
                                    div()
                                        .size(px(16.))
                                        .rounded_full()
                                        .border_1()
                                        .border_color(rgb(p.primary))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            div().size(px(8.)).rounded_full().bg(rgb(if active {
                                                p.primary
                                            } else {
                                                p.surface
                                            })),
                                        ),
                                )
                                .child(engine.name())
                        },
                    )),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_rows_list_every_engine_once_and_mark_only_the_current_one() {
        for current in SearchEngine::ALL {
            let rows = search_rows(current);
            assert_eq!(
                rows.iter().map(|(engine, _)| *engine).collect::<Vec<_>>(),
                SearchEngine::ALL.to_vec()
            );
            assert_eq!(
                rows.iter()
                    .filter(|(_, active)| *active)
                    .map(|(engine, _)| *engine)
                    .collect::<Vec<_>>(),
                vec![current]
            );
        }
    }

    #[test]
    fn search_engine_names_are_distinct_and_readable() {
        let mut names: Vec<_> = SearchEngine::ALL.iter().map(|e| e.name()).collect();
        assert!(names.iter().all(|name| !name.trim().is_empty()));
        assert_eq!(SearchEngine::DuckDuckGo.name(), "DuckDuckGo");
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), SearchEngine::ALL.len());
    }
}
