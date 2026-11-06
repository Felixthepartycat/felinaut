mod engine;
mod input;
mod webview;

use gpui::{prelude::*, *};
use input::{AddressInput, Submit};
use std::{rc::Rc, time::Duration};
use webview::{Event, NativePage};

const HOME: &str = "felifox://newtab";
pub const DISPLAY_VERSION: &str = "0.1";

actions!(
    felifox,
    [
        NewTab,
        CloseTab,
        FocusAddress,
        Back,
        Forward,
        Reload,
        Home,
        Quit
    ]
);

struct Tab {
    id: u64,
    title: String,
    url: String,
    home: bool,
    loading: bool,
    error: Option<String>,
    page: Option<Rc<NativePage>>,
}
impl Tab {
    fn new(id: u64) -> Self {
        Self {
            id,
            title: "New Tab".into(),
            url: HOME.into(),
            home: true,
            loading: false,
            error: None,
            page: None,
        }
    }
    fn url(&self) -> &str {
        if self.home { HOME } else { &self.url }
    }
    fn can_go_back(&self) -> bool {
        !self.home && self.page.is_some()
    }
    fn can_go_forward(&self) -> bool {
        self.page
            .as_ref()
            .is_some_and(|page| self.home || page.can_go_forward())
    }
}

struct Browser {
    tabs: Vec<Tab>,
    active: usize,
    next_id: u64,
    address: Entity<AddressInput>,
    focus: FocusHandle,
    _submit: Subscription,
}
impl Browser {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let address = cx.new(AddressInput::new);
        let subscription =
            cx.subscribe_in(&address, window, |this, _, event: &Submit, window, cx| {
                this.navigate(&event.0, window, cx);
                let tab = &this.tabs[this.active];
                if !tab.home
                    && tab.error.is_none()
                    && let Some(page) = &tab.page
                {
                    window.focus(&this.focus);
                    let _ = page.focus();
                }
            });
        // Native callbacks enqueue events, avoiding reentrant updates while GPUI renders.
        cx.spawn_in(window, async move |this, cx| {
            loop {
                Timer::after(Duration::from_millis(100)).await;
                if this
                    .update_in(cx, |this, window, cx| this.poll_pages(window, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        let weak = cx.weak_entity();
        window.on_window_should_close(cx, move |_, cx| {
            let _ = weak.update(cx, |this, _| {
                for tab in &mut this.tabs {
                    tab.page.take();
                }
            });
            true
        });
        let focus = cx.focus_handle();
        window.focus(&focus);
        Self {
            tabs: vec![Tab::new(1)],
            active: 0,
            next_id: 2,
            address,
            focus,
            _submit: subscription,
        }
    }
    fn sync_address(&self, cx: &mut Context<Self>) {
        let url = self.tabs[self.active].url();
        let text = if url == HOME {
            String::new()
        } else {
            url.into()
        };
        self.address
            .update(cx, |input, cx| input.set_text(text, cx));
    }
    fn poll_pages(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut changed = false;
        let mut address_changed = false;
        let mut open = Vec::new();
        for (index, tab) in self.tabs.iter_mut().enumerate() {
            let Some(page) = &tab.page else {
                continue;
            };
            for event in page.drain_events() {
                changed = true;
                match event {
                    Event::Started(url) => {
                        tab.loading = true;
                        tab.error = None;
                        tab.url = url;
                    }
                    Event::Finished(url) => {
                        tab.loading = false;
                        tab.url = url;
                    }
                    Event::Title(title) => {
                        tab.title = title;
                    }
                    Event::Url(url) => {
                        tab.url = url;
                        address_changed |= index == self.active;
                    }
                    Event::Open(url) => open.push(url),
                }
            }
        }
        if address_changed && !self.address.read(cx).focus_handle(cx).is_focused(window) {
            self.sync_address(cx);
        }
        for url in open {
            self.new_tab(&NewTab, window, cx);
            self.navigate(&url, window, cx);
        }
        if changed {
            cx.notify();
        }
    }
    fn navigate(&mut self, input: &str, window: &mut Window, cx: &mut Context<Self>) {
        let url = match engine::resolve_input(input) {
            Ok(url) => url,
            Err(error) => {
                self.tabs[self.active].error = Some(error);
                cx.notify();
                return;
            }
        };
        if url == HOME {
            self.home(&Home, window, cx);
            return;
        }
        let tab = &mut self.tabs[self.active];
        let result = if let Some(page) = &tab.page {
            page.navigate(&url)
        } else {
            NativePage::new(window, &url).map(|page| {
                tab.page = Some(Rc::new(page));
            })
        };
        match result {
            Ok(()) => {
                tab.url = url;
                tab.home = false;
                tab.loading = true;
                tab.error = None;
                tab.title = url::Url::parse(&tab.url)
                    .ok()
                    .and_then(|u| u.host_str().map(str::to_owned))
                    .unwrap_or_else(|| "Loading…".into());
            }
            Err(error) => tab.error = Some(error),
        }
        self.sync_address(cx);
        cx.notify();
    }
    fn select_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(page) = &self.tabs[self.active].page {
            let _ = page.set_visible(false);
            let _ = page.focus_parent();
        }
        self.active = index;
        window.focus(&self.focus);
        self.sync_address(cx);
        cx.notify();
    }
    fn new_tab(&mut self, _: &NewTab, window: &mut Window, cx: &mut Context<Self>) {
        self.tabs.push(Tab::new(self.next_id));
        self.next_id += 1;
        self.select_tab(self.tabs.len() - 1, window, cx);
        self.focus_address(&FocusAddress, window, cx);
    }
    fn close_tab(&mut self, _: &CloseTab, window: &mut Window, cx: &mut Context<Self>) {
        self.close_tab_at(self.active, window, cx);
    }
    fn close_tab_at(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index == self.active {
            if let Some(page) = &self.tabs[index].page {
                let _ = page.focus_parent();
            }
            window.focus(&self.focus);
        }
        self.tabs.remove(index);
        if self.tabs.is_empty() {
            self.tabs.push(Tab::new(self.next_id));
            self.next_id += 1;
        }
        if index < self.active {
            self.active -= 1;
        }
        self.active = self.active.min(self.tabs.len() - 1);
        self.sync_address(cx);
        cx.notify();
    }
    fn focus_address(&mut self, _: &FocusAddress, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(page) = &self.tabs[self.active].page {
            let _ = page.focus_parent();
        }
        self.address.update(cx, |input, cx| {
            window.focus(&input.focus_handle(cx));
            input.select_all_text(cx);
        });
    }
    fn back(&mut self, _: &Back, window: &mut Window, cx: &mut Context<Self>) {
        let tab = &mut self.tabs[self.active];
        if tab.home {
            return;
        }
        if let Some(page) = &tab.page {
            if page.can_go_back() {
                tab.error = page.back().err();
            } else {
                self.home(&Home, window, cx);
                return;
            }
        }
        cx.notify();
    }
    fn forward(&mut self, _: &Forward, _: &mut Window, cx: &mut Context<Self>) {
        let tab = &mut self.tabs[self.active];
        if let Some(page) = &tab.page {
            if tab.home {
                tab.home = false;
                tab.error = None;
            } else if page.can_go_forward() {
                tab.error = page.forward().err();
            }
        }
        self.sync_address(cx);
        cx.notify();
    }
    fn reload(&mut self, _: &Reload, _: &mut Window, cx: &mut Context<Self>) {
        let tab = &mut self.tabs[self.active];
        if !tab.home
            && let Some(page) = &tab.page
        {
            tab.error = page.reload().err();
        }
        cx.notify();
    }
    fn home(&mut self, _: &Home, window: &mut Window, cx: &mut Context<Self>) {
        let tab = &mut self.tabs[self.active];
        if let Some(page) = &tab.page {
            let _ = page.set_visible(false);
            let _ = page.focus_parent();
        }
        tab.home = true;
        tab.error = None;
        window.focus(&self.focus);
        self.sync_address(cx);
        cx.notify();
    }

    fn button(&self, id: &'static str, label: &'static str) -> Stateful<Div> {
        div()
            .id(id)
            .h(px(34.))
            .min_w(px(34.))
            .px_2()
            .rounded_lg()
            .flex()
            .items_center()
            .justify_center()
            .text_color(rgb(0x969fbe))
            .cursor_pointer()
            .hover(move |s| s.bg(rgb(0x2b3251)).text_color(rgb(0xeff1ff)))
            .child(label)
    }
    fn home_page(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_6()
            .child(
                div()
                    .text_size(px(54.))
                    .font_weight(FontWeight::BOLD)
                    .child("Felifox"),
            )
            .child(
                div()
                    .text_color(rgb(0x969fbe))
                    .child("A small browser. A world to discover."),
            )
            .child(
                div()
                    .id("home-search")
                    .w(px(520.))
                    .h(px(48.))
                    .px_5()
                    .rounded_xl()
                    .bg(rgb(0x191e35))
                    .flex()
                    .items_center()
                    .cursor_text()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.focus_address(&FocusAddress, window, cx)
                    }))
                    .child("Search the web or enter an address     ⌘ L"),
            )
            .child(
                div().flex().gap_4().children(
                    [
                        ("Wikipedia", "https://en.wikipedia.org/wiki/Main_Page"),
                        ("Rust", "https://www.rust-lang.org"),
                        ("Hacker News", "https://news.ycombinator.com"),
                        ("GitHub", "https://github.com"),
                    ]
                    .into_iter()
                    .map(|(name, url)| {
                        div()
                            .id(name)
                            .px_4()
                            .py_3()
                            .rounded_lg()
                            .bg(rgb(0x191e35))
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.navigate(url, window, cx)
                            }))
                            .child(name)
                    }),
                ),
            )
            .into_any_element()
    }
    fn content(&self, cx: &mut Context<Self>) -> AnyElement {
        let tab = &self.tabs[self.active];
        if tab.home {
            return self.home_page(cx);
        }
        if let Some(page) = &tab.page {
            let page = page.clone();
            return canvas(
                move |bounds, _, _| {
                    if let Err(error) = page.set_bounds(bounds).and_then(|_| page.set_visible(true))
                    {
                        eprintln!("Could not position native webview: {error}");
                    }
                },
                |_, _, _, _| {},
            )
            .size_full()
            .into_any_element();
        }
        div().size_full().into_any_element()
    }
}

impl Render for Browser {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = &self.tabs[self.active];
        let back_enabled = tab.can_go_back();
        let forward_enabled = tab.can_go_forward();
        let status = if tab.loading && !tab.home {
            "Loading · WebKit"
        } else {
            "WebKit · JavaScript enabled"
        };
        div()
            .id("felifox")
            .track_focus(&self.focus)
            .key_context("Browser")
            .size_full()
            .bg(rgb(0x101324))
            .text_color(rgb(0xeff1ff))
            .font_family(".AppleSystemUIFont")
            .flex()
            .flex_col()
            .on_action(cx.listener(Self::new_tab))
            .on_action(cx.listener(Self::close_tab))
            .on_action(cx.listener(Self::focus_address))
            .on_action(cx.listener(Self::back))
            .on_action(cx.listener(Self::forward))
            .on_action(cx.listener(Self::reload))
            .on_action(cx.listener(Self::home))
            .child(
                div()
                    .h(px(52.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .pl(px(88.))
                    .pr_4()
                    .children(self.tabs.iter().enumerate().map(|(index, tab)| {
                        let selected = self.active == index;
                        div()
                            .id(("tab", tab.id))
                            .h(px(34.))
                            .w(px(195.))
                            .min_w(px(70.))
                            .px_3()
                            .rounded_lg()
                            .bg(rgb(if selected { 0x191e35 } else { 0x101324 }))
                            .flex()
                            .items_center()
                            .gap_2()
                            .cursor_pointer()
                            .hover(move |s| s.bg(rgb(0x2b3251)))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.select_tab(index, window, cx);
                            }))
                            .child(
                                div()
                                    .text_color(rgb(if selected { 0xff966f } else { 0x969fbe }))
                                    .child(if tab.loading && !tab.home {
                                        div().child("◌").into_any_element()
                                    } else {
                                        div().child("◆").into_any_element()
                                    }),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .text_sm()
                                    .child(if tab.home {
                                        "New Tab".into()
                                    } else {
                                        tab.title.clone()
                                    }),
                            )
                            .child(
                                div()
                                    .id(("close", tab.id))
                                    .text_color(rgb(0x969fbe))
                                    .px_1()
                                    .rounded_md()
                                    .hover(move |s| s.bg(rgb(0x2b3251)).text_color(rgb(0xff966f)))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.close_tab_at(index, window, cx);
                                    }))
                                    .child("×"),
                            )
                    }))
                    .child(self.button("new-tab", "+").on_click(
                        cx.listener(|this, _, window, cx| this.new_tab(&NewTab, window, cx)),
                    )),
            )
            .child(
                div()
                    .h(px(64.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .px_4()
                    .gap_2()
                    .bg(rgb(0x191e35))
                    .child(
                        self.button("back", "←")
                            .opacity(if back_enabled { 1. } else { 0.35 })
                            .on_click(
                                cx.listener(|this, _, window, cx| this.back(&Back, window, cx)),
                            ),
                    )
                    .child(
                        self.button("forward", "→")
                            .opacity(if forward_enabled { 1. } else { 0.35 })
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.forward(&Forward, window, cx)
                            })),
                    )
                    .child(self.button("reload", "↻").on_click(
                        cx.listener(|this, _, window, cx| this.reload(&Reload, window, cx)),
                    ))
                    .child(
                        self.button("home", "⌂").on_click(
                            cx.listener(|this, _, window, cx| this.home(&Home, window, cx)),
                        ),
                    )
                    .child(div().flex_1().min_w_0().child(self.address.clone())),
            )
            .child(div().h(px(2.)).flex_shrink_0().bg(rgb(if tab.loading {
                0xff966f
            } else {
                0x38304f
            })))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(self.content(cx)),
            )
            .child(
                div()
                    .h(px(30.))
                    .flex_shrink_0()
                    .px_4()
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_xs()
                    .text_color(rgb(0x969fbe))
                    .child(tab.error.as_deref().unwrap_or(status).to_owned())
                    .child(format!("FELIFOX  /  {DISPLAY_VERSION}")),
            )
    }
}
fn main() {
    Application::new().run(|cx: &mut App| {
        input::init(cx);
        cx.bind_keys([
            KeyBinding::new("cmd-t", NewTab, None),
            KeyBinding::new("cmd-w", CloseTab, None),
            KeyBinding::new("cmd-l", FocusAddress, None),
            KeyBinding::new("cmd-r", Reload, None),
            KeyBinding::new("cmd-[", Back, None),
            KeyBinding::new("cmd-]", Forward, None),
            KeyBinding::new("cmd-shift-h", Home, None),
            KeyBinding::new("cmd-q", Quit, None),
        ]);
        // Native menu equivalents remain available while WKWebView owns keyboard focus.
        cx.set_menus(vec![
            Menu {
                name: "Felifox".into(),
                items: vec![MenuItem::action("Quit Felifox", Quit)],
            },
            Menu {
                name: "File".into(),
                items: vec![
                    MenuItem::action("New Tab", NewTab),
                    MenuItem::action("Close Tab", CloseTab),
                ],
            },
            Menu {
                name: "Edit".into(),
                items: vec![
                    MenuItem::os_action("Cut", input::Cut, OsAction::Cut),
                    MenuItem::os_action("Copy", input::Copy, OsAction::Copy),
                    MenuItem::os_action("Paste", input::Paste, OsAction::Paste),
                    MenuItem::os_action("Select All", input::SelectAll, OsAction::SelectAll),
                ],
            },
            Menu {
                name: "Navigate".into(),
                items: vec![
                    MenuItem::action("Open Location", FocusAddress),
                    MenuItem::action("Back", Back),
                    MenuItem::action("Forward", Forward),
                    MenuItem::action("Reload", Reload),
                    MenuItem::action("Home", Home),
                ],
            },
        ]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let bounds = Bounds::centered(None, size(px(1280.), px(900.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(1000.), px(700.))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Felifox".into()),
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(16.), px(21.))),
                }),
                ..Default::default()
            },
            |window, cx| cx.new(|cx| Browser::new(window, cx)),
        )
        .expect("Could not open Felifox window");
        cx.activate(true);
    });
}
