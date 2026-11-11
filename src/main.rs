mod about;
mod agent;
mod appearance;
mod art;
mod audio;
mod brand;
mod engine;
mod history;
mod icons;
mod input;
mod pinned;
mod suggestions;
mod theme;
mod volume;
mod webview;

use audio::{AudioReport, PageSound, TabAudio};
use gpui::{prelude::*, *};
use history::{History, Suggestion};
use input::{AddressEvent, AddressInput, Submit};
use std::{
    cell::RefCell,
    collections::HashMap,
    fs,
    path::PathBuf,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};
use theme::{AddressDisplay, Palette, ThemeSettings};
use webview::{Event, NativePage};

const HOME: &str = "felifox://newtab";
const SIDEBAR_WIDTH: f32 = 240.;
const SIDEBAR_SLIDE: Duration = Duration::from_millis(200);
/// The pointer within this many px of the window's left edge peeks a hidden sidebar…
const PEEK_EDGE: f32 = 6.;
/// …which stays until the pointer is this far past its right edge.
const PEEK_MARGIN: f32 = 24.;
/// Three tiles and two 8 px gaps fill the sidebar's 223 px list (minus its border and padding).
const PINNED_TILE: f32 = 69.;

/// The host of a web URL, which keys site icons and labels the title chip.
pub fn host(url: &str) -> Option<String> {
    url::Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
}
pub const DISPLAY_VERSION: &str = "0.1 beta";

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
        Quit,
        AboutFelifox,
        VolumeControls,
        MuteTab,
        ToggleSidebar,
        PinTab,
        NextTab,
        PreviousTab
    ]
);

/// ⌘1–⌘8 select that tab in sidebar order; `LAST_TAB` (⌘9) selects the last one.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = felifox, no_json)]
struct ActivateTab(usize);
const LAST_TAB: usize = usize::MAX;

struct Tab {
    id: u64,
    title: String,
    url: String,
    home: bool,
    loading: bool,
    error: Option<String>,
    hovered_link: Option<String>,
    page: Option<Rc<NativePage>>,
    /// The user's settings for this tab; they persist across its navigations.
    audio: TabAudio,
    /// What the current document reports about its playback.
    sound: PageSound,
    /// The last URL this tab added to history, so reloads and the URL poll don't count twice.
    recorded: String,
    /// The URL a pinned tab reopens on launch. Pinned tabs come first in `Browser::tabs`.
    pinned: Option<String>,
}
impl Tab {
    /// A saved pinned tab; its page is created when it is first selected.
    fn pinned(id: u64, url: String, title: String) -> Self {
        Self {
            title: if title.is_empty() { url.clone() } else { title },
            url: url.clone(),
            home: false,
            pinned: Some(url),
            ..Self::new(id)
        }
    }
    fn new(id: u64) -> Self {
        Self {
            id,
            title: "New Tab".into(),
            url: HOME.into(),
            home: true,
            loading: false,
            error: None,
            hovered_link: None,
            page: None,
            audio: TabAudio::default(),
            sound: PageSound::default(),
            recorded: String::new(),
            pinned: None,
        }
    }
    /// Adds the current page to history once per URL per tab.
    fn record_visit(&mut self, history: &mut History) {
        if self.recorded != self.url && history.record(&self.url, &self.title, history::now()) {
            self.recorded.clone_from(&self.url);
        }
    }
    fn display_title(&self) -> String {
        if self.home {
            "New Tab".into()
        } else {
            self.title.clone()
        }
    }
    fn sound(&self, active: bool) -> volume::TabSound {
        volume::TabSound {
            id: self.id,
            title: self.display_title().into(),
            audio: self.audio,
            audible: self.sound.audible(),
            limited: self.sound.limited(),
            active,
        }
    }
    fn apply_audio(&self) {
        if let Some(page) = &self.page
            && let Err(error) = page.apply_audio(self.audio)
        {
            eprintln!("Could not apply tab audio: {error}");
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
    _theme_observer: Subscription,
    art_phase: f32,
    history: History,
    last_save: Instant,
    /// History rows matching the address text, shown by `suggestions` while typing.
    suggestions: Vec<Suggestion>,
    selected: Option<usize>,
    /// What the user typed, restored when stepping off the last suggestion or pressing Esc.
    typed: String,
    _address_events: Subscription,
    _address_focus: Subscription,
    _address_blur: Subscription,
    _activation_observer: Subscription,
    _bounds_observer: Subscription,
    /// `history/icons/`, beside the visits file.
    icon_dir: Option<PathBuf>,
    /// Site icons by host; `None` records a host without one so disk is read once.
    icons: RefCell<HashMap<String, Option<Arc<Image>>>>,
    /// Commands from the control socket; `None` unless started with `FELIFOX_AGENT=1`.
    agent: Option<agent::Queue>,
    /// What the hovered sidebar control does, shown in the status bar. GPUI tooltips would
    /// draw underneath the native page, so the status bar carries them instead.
    hint: Option<&'static str>,
    /// The sidebar row under the pointer; it reveals the pin and fills the status bar.
    hovered_tab: Option<u64>,
    tab_scroll: ScrollHandle,
    /// When the sidebar last started sliding; `None` once settled or with motion reduced.
    sidebar_anim: Option<Instant>,
    /// A hidden sidebar shown because the pointer touched the window's left edge, like Arc.
    peek: bool,
    /// `None` without a home, or when the file was unreadable so it is never overwritten.
    pinned_path: Option<PathBuf>,
}
impl Browser {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let theme_observer = cx.observe_global::<ThemeSettings>(|_, cx| cx.notify());
        let address = cx.new(AddressInput::new);
        cx.spawn_in(window, async move |this, cx| {
            let mut previous = Instant::now();
            let mut was_running = false;
            let mut delay = Duration::from_millis(200);
            loop {
                Timer::after(delay).await;
                let now = Instant::now();
                let elapsed = now.duration_since(previous).as_secs_f32();
                previous = now;
                let result = this.update_in(cx, |this, window, cx| {
                    let running = this.tabs[this.active].home
                        && window.is_window_active()
                        && theme::motion_allowed(cx);
                    if running && was_running {
                        this.art_phase = (this.art_phase + elapsed) % 3600.;
                        cx.notify();
                    }
                    running
                });
                let Ok(running) = result else {
                    break;
                };
                was_running = running;
                delay = Duration::from_millis(if running { 33 } else { 200 });
            }
        })
        .detach();
        let subscription =
            cx.subscribe_in(&address, window, |this, _, event: &Submit, window, cx| {
                this.submit(&event.0, window, cx);
            });
        let address_events = cx.subscribe_in(
            &address,
            window,
            |this, _, event: &AddressEvent, window, cx| match event {
                AddressEvent::Changed => this.update_suggestions(window, cx),
                AddressEvent::Up => this.move_selection(false, cx),
                AddressEvent::Down => this.move_selection(true, cx),
                AddressEvent::Escape => this.close_suggestions(true, window, cx),
            },
        );
        let address_focus = address.read(cx).focus_handle(cx);
        let address_blur = cx.on_blur(&address_focus, window, |this, window, cx| {
            // GPUI also blurs when the window deactivates, as it does while a row or its × is
            // clicked in the dropdown; the activation observer below handles that case.
            if window.is_window_active() {
                this.close_suggestions(false, window, cx);
            }
            cx.notify();
        });
        // An empty address bar offers recent pages as soon as it is focused.
        let address_focus_observer = cx.on_focus(&address_focus, window, |this, window, cx| {
            if this.address.read(cx).text().is_empty() {
                this.update_suggestions(window, cx);
            }
            cx.notify();
        });
        // The dropdown floats above other apps, so it must go when the window loses focus;
        // clicking a suggestion also deactivates us briefly, hence the short grace period.
        let activation_observer = cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() || this.suggestions.is_empty() {
                return;
            }
            cx.spawn_in(window, async move |this, cx| {
                Timer::after(Duration::from_millis(80)).await;
                let _ = this.update_in(cx, |this, window, cx| {
                    if !window.is_window_active() && !suggestions::is_active(cx) {
                        this.close_suggestions(false, window, cx);
                    }
                });
            })
            .detach();
        });
        // GPUI also reports bounds on activation changes (clicking a row makes the dropdown
        // key); only a real move or resize should close the list.
        let mut last_bounds = window.bounds();
        let bounds_observer = cx.observe_window_bounds(window, move |this, window, cx| {
            let bounds = window.bounds();
            if bounds != last_bounds {
                last_bounds = bounds;
                this.close_suggestions(false, window, cx);
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
            // Otherwise the dropdown outlives us and the app never quits.
            suggestions::close(cx);
            let _ = weak.update(cx, |this, _| {
                for tab in &mut this.tabs {
                    tab.page.take();
                }
                // The debounced save may still be pending; this is the last chance to write.
                if let Some((path, contents)) = this.history.take_snapshot()
                    && let Err(error) = history::write(&path, &contents)
                {
                    eprintln!("Could not save browsing history: {error}");
                }
                // Titles may have changed since the last pin edit.
                this.save_pinned();
            });
            true
        });
        let focus = cx.focus_handle();
        window.focus(&focus);
        let mut pinned_path = pinned::default_path();
        let saved = match pinned_path.as_deref().map(fs::read_to_string) {
            Some(Ok(text)) => pinned::parse(&text),
            Some(Err(error)) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Some(Err(error)) => {
                eprintln!("Could not read pinned tabs: {error}");
                pinned_path = None;
                Vec::new()
            }
            None => Vec::new(),
        };
        let mut tabs: Vec<Tab> = saved
            .into_iter()
            .zip(1..)
            .map(|((url, title), id)| Tab::pinned(id, url, title))
            .collect();
        let next_id = tabs.len() as u64 + 1;
        tabs.push(Tab::new(next_id));
        Self {
            active: tabs.len() - 1,
            tabs,
            next_id: next_id + 1,
            address,
            focus,
            _submit: subscription,
            _theme_observer: theme_observer,
            art_phase: 0.,
            history: History::load(history::default_path()),
            last_save: Instant::now(),
            suggestions: Vec::new(),
            selected: None,
            typed: String::new(),
            _address_events: address_events,
            _address_focus: address_focus_observer,
            _address_blur: address_blur,
            _activation_observer: activation_observer,
            _bounds_observer: bounds_observer,
            icon_dir: history::default_path()
                .and_then(|path| path.parent().map(|dir| dir.join("icons"))),
            icons: RefCell::new(HashMap::new()),
            agent: agent::start(),
            hint: None,
            hovered_tab: None,
            tab_scroll: ScrollHandle::new(),
            sidebar_anim: None,
            peek: false,
            pinned_path,
        }
    }
    fn pinned_count(&self) -> usize {
        self.tabs
            .iter()
            .take_while(|tab| tab.pinned.is_some())
            .count()
    }
    /// A tiny file written only on pin edits and quit, so it is written synchronously.
    fn save_pinned(&self) {
        let Some(path) = &self.pinned_path else {
            return;
        };
        let contents = pinned::serialize(
            self.tabs
                .iter()
                .filter_map(|tab| Some((tab.pinned.as_deref()?, tab.title.as_str()))),
        );
        if let Err(error) = history::write(path, &contents) {
            eprintln!("Could not save pinned tabs: {error}");
        }
    }
    /// Pins at the end of the pinned group, or unpins to the top of the other tabs.
    fn toggle_pin_at(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.tabs[index].pinned.is_none() && self.tabs[index].url() == HOME {
            return;
        }
        self.hint = None;
        let active_id = self.tabs[self.active].id;
        let mut tab = self.tabs.remove(index);
        tab.pinned = match tab.pinned {
            Some(_) => None,
            None => Some(tab.url.clone()),
        };
        self.tabs.insert(self.pinned_count(), tab);
        self.active = self
            .tabs
            .iter()
            .position(|tab| tab.id == active_id)
            .unwrap_or(0);
        self.save_pinned();
        cx.notify();
    }
    fn pin_tab(&mut self, _: &PinTab, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle_pin_at(self.active, cx);
    }
    /// Saved with the other settings, so the sidebar stays collapsed across launches.
    fn sidebar_shown(&self, cx: &App) -> bool {
        cx.global::<ThemeSettings>().sidebar || self.peek
    }
    /// Starts a slide toward the opposite of the current state; call before flipping it.
    fn start_slide(&mut self, cx: &App) {
        self.sidebar_anim = theme::motion_allowed(cx).then(|| {
            // Reversing mid-slide continues from the current width instead of jumping.
            let left = SIDEBAR_SLIDE.saturating_sub(
                self.sidebar_anim
                    .map_or(SIDEBAR_SLIDE, |start| start.elapsed()),
            );
            Instant::now() - left
        });
    }
    /// Peeks the hidden sidebar while the pointer is at the window's left edge, and hides it
    /// again once the pointer moves past it or leaves the window.
    fn update_peek(&mut self, window: &Window, cx: &mut Context<Self>) {
        if cx.global::<ThemeSettings>().sidebar {
            self.peek = false;
            return;
        }
        let pointer = window
            .is_window_active()
            .then(|| webview::pointer(window))
            .flatten();
        let peek = pointer.is_some_and(|at| {
            at.x <= if self.peek {
                SIDEBAR_WIDTH + PEEK_MARGIN
            } else {
                PEEK_EDGE
            }
        });
        if peek != self.peek {
            self.start_slide(cx);
            self.peek = peek;
            cx.notify();
        }
    }
    fn toggle_sidebar(&mut self, _: &ToggleSidebar, _: &mut Window, cx: &mut Context<Self>) {
        self.hint = None;
        // ⌘S during a peek keeps the already visible sidebar open: no slide.
        if !self.peek {
            self.start_slide(cx);
        }
        self.peek = false;
        cx.update_global::<ThemeSettings, _>(|settings, _| {
            settings.sidebar = !settings.sidebar;
            settings.save();
        });
        cx.notify();
    }
    fn activate_tab(&mut self, action: &ActivateTab, window: &mut Window, cx: &mut Context<Self>) {
        let index = action.0.min(self.tabs.len() - 1);
        self.select_tab(index, window, cx);
    }
    /// Cycles through tabs in sidebar order, wrapping at either end.
    fn step_tab(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.tabs.len();
        let index = if forward {
            (self.active + 1) % count
        } else {
            (self.active + count - 1) % count
        };
        self.select_tab(index, window, cx);
    }
    fn next_tab(&mut self, _: &NextTab, window: &mut Window, cx: &mut Context<Self>) {
        self.step_tab(true, window, cx);
    }
    fn previous_tab(&mut self, _: &PreviousTab, window: &mut Window, cx: &mut Context<Self>) {
        self.step_tab(false, window, cx);
    }
    /// Pinned tabs restored at launch have no page until they are first shown.
    fn load_if_needed(&mut self, index: usize, window: &mut Window) {
        let tab = &mut self.tabs[index];
        if tab.page.is_some() || tab.home {
            return;
        }
        match NativePage::new(window, &tab.url) {
            Ok(page) => {
                tab.page = Some(Rc::new(page));
                tab.loading = true;
            }
            Err(error) => tab.error = Some(error),
        }
    }
    /// Runs one command from the control socket against the active tab.
    fn handle_agent(
        &mut self,
        request: agent::Request,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use serde_json::{Value, json};
        let command = &request.command;
        let result = match command["cmd"].as_str().unwrap_or_default() {
            "tabs" => Ok(self
                .tabs
                .iter()
                .enumerate()
                .map(|(index, tab)| {
                    json!({
                        "index": index,
                        "title": tab.display_title(),
                        "url": tab.url,
                        "active": index == self.active,
                        "loading": tab.loading,
                    })
                })
                .collect()),
            "tab" => match command["index"].as_u64().map(|index| index as usize) {
                Some(index) if index < self.tabs.len() => {
                    self.select_tab(index, window, cx);
                    Ok(Value::Null)
                }
                _ => Err("No such tab".into()),
            },
            "navigate" => {
                self.navigate(command["url"].as_str().unwrap_or_default(), window, cx);
                let tab = &self.tabs[self.active];
                tab.error.clone().map_or_else(|| Ok(json!(tab.url)), Err)
            }
            _ => {
                let tab = &self.tabs[self.active];
                match (&tab.page, &self.agent) {
                    (Some(page), Some((_, queue))) if !tab.home => {
                        return agent::run(page, request, queue);
                    }
                    _ => Err("The active tab has no page. Navigate first.".into()),
                }
            }
        };
        let _ = request.reply.send(agent::response(result));
    }
    fn icon_for(&self, host: &str) -> Option<Arc<Image>> {
        let mut icons = self.icons.borrow_mut();
        if let Some(icon) = icons.get(host) {
            return icon.clone();
        }
        // One small read per host per session; misses are remembered too.
        let icon = self
            .icon_dir
            .as_ref()
            .and_then(|dir| fs::read(icons::path(dir, host)).ok())
            .filter(|bytes| icons::valid_png(bytes))
            .map(|bytes| Arc::new(Image::from_bytes(ImageFormat::Png, bytes)));
        icons.insert(host.to_owned(), icon.clone());
        icon
    }
    /// The site's icon, or a themed monogram of its host.
    fn site_icon(&self, host: &str, size: f32, p: Palette) -> AnyElement {
        if let Some(image) = self.icon_for(host) {
            return img(image).size(px(size)).rounded_sm().into_any_element();
        }
        let (letter, slot) = icons::monogram(host);
        let color = [p.primary, p.secondary, p.accent, p.highlight][slot];
        div()
            .size(px(size))
            .flex_shrink_0()
            .rounded_md()
            .bg(rgb(color))
            .text_color(rgb(p.background))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(size * 0.6))
            .font_weight(FontWeight::BOLD)
            .child(letter.to_string())
            .into_any_element()
    }
    fn store_icon(&mut self, url: &str, png: Vec<u8>, cx: &mut Context<Self>) {
        let Some(host) = host(url) else {
            return;
        };
        let image = Arc::new(Image::from_bytes(ImageFormat::Png, png.clone()));
        let unchanged = self.icons.borrow().get(&host).is_some_and(|cached| {
            cached
                .as_ref()
                .is_some_and(|cached| cached.id() == image.id())
        });
        if unchanged {
            return;
        }
        if let Some(dir) = &self.icon_dir {
            let path = icons::path(dir, &host);
            cx.background_executor()
                .spawn(async move {
                    if let Err(error) = icons::write(&path, &png) {
                        eprintln!("Could not save site icon: {error}");
                    }
                })
                .detach();
        }
        self.icons.borrow_mut().insert(host, Some(image));
        cx.notify();
    }
    /// The idle address field: icon plus page title and domain, or the domain alone.
    fn title_chip(
        &self,
        display: AddressDisplay,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tab = &self.tabs[self.active];
        let host = host(&tab.url).unwrap_or_default();
        let (label, detail) = if display == AddressDisplay::Domain {
            (host.clone(), None)
        } else {
            (tab.display_title(), Some(host.clone()))
        };
        div()
            .id("title-chip")
            .h(px(42.))
            .w_full()
            .min_w_0()
            .rounded_lg()
            .bg(rgb(p.input))
            .border_1()
            .border_color(rgb(p.border))
            .px_3()
            .flex()
            .items_center()
            .gap_2()
            .cursor_text()
            .hover(move |s| s.border_color(rgb(p.primary)))
            .on_click(
                cx.listener(|this, _, window, cx| this.focus_address(&FocusAddress, window, cx)),
            )
            .child(self.site_icon(&host, 16., p))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .text_size(px(14.))
                    .child(label),
            )
            .when_some(detail, |chip, detail| {
                chip.child(
                    div()
                        .flex_shrink_0()
                        .text_size(px(12.))
                        .text_color(rgb(p.muted))
                        .child(detail),
                )
            })
            .into_any_element()
    }
    fn submit(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        // A selected or autocompleted row navigates to its exact URL, not the shown text.
        let target = self
            .selected
            .and_then(|index| self.suggestions.get(index))
            .map_or_else(|| text.to_owned(), |row| row.url.clone());
        self.close_suggestions(false, window, cx);
        self.navigate(&target, window, cx);
        let tab = &self.tabs[self.active];
        if !tab.home
            && tab.error.is_none()
            && let Some(page) = &tab.page
        {
            window.focus(&self.focus);
            let _ = page.focus();
        }
    }
    /// The user edited the address: refresh the rows and, with the caret at the end, complete
    /// the first row whose URL contains the text (`le` → `goog·le·.com`, dimmed around it).
    fn update_suggestions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = self.address.read(cx);
        let text = input.text().to_owned();
        let at_end = input.caret_at_end();
        self.typed = text;
        self.refresh_suggestions(window, cx);
        if at_end && let Some((index, start)) = history::completion(&self.typed, &self.suggestions)
        {
            let row = self.suggestions.remove(index);
            self.suggestions.insert(0, row);
            self.selected = Some(0);
            let display = &self.suggestions[0].display_url;
            let ghost = (
                display[..start].to_owned(),
                display[start + self.typed.len()..].to_owned(),
            );
            self.address
                .update(cx, |input, cx| input.set_completion(Some(ghost), cx));
        }
    }
    /// Recomputes the rows for the typed text and opens or closes the dropdown accordingly.
    fn refresh_suggestions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let anchor = self.address.read(cx).bounds();
        self.selected = None;
        self.suggestions = if self.typed.trim().is_empty() {
            self.history.recent(history::now(), suggestions::MAX_ROWS)
        } else {
            self.history
                .suggest(&self.typed, history::now(), suggestions::MAX_ROWS)
        };
        match anchor {
            Some(anchor) if !self.suggestions.is_empty() => {
                let browser = cx.weak_entity();
                // Opening a window from inside this update would re-enter it; see `open_volume`.
                window.defer(cx, move |window, cx| {
                    suggestions::open(browser, anchor, window, cx)
                });
            }
            _ => window.defer(cx, |_, cx| suggestions::close(cx)),
        }
        cx.notify();
    }
    /// ↓ walks from the typed text through the rows and back; ↑ the other way round.
    fn move_selection(&mut self, down: bool, cx: &mut Context<Self>) {
        let count = self.suggestions.len();
        if count == 0 {
            return;
        }
        self.selected = match (self.selected, down) {
            (None, true) => Some(0),
            (None, false) => Some(count - 1),
            (Some(index), true) => (index + 1 < count).then_some(index + 1),
            (Some(index), false) => index.checked_sub(1),
        };
        let text = match self.selected {
            Some(index) => self.suggestions[index].url.clone(),
            None => self.typed.clone(),
        };
        self.address
            .update(cx, |input, cx| input.set_text(text, cx));
        cx.notify();
    }
    fn close_suggestions(&mut self, restore: bool, window: &mut Window, cx: &mut Context<Self>) {
        if restore && self.selected.is_some() {
            let typed = self.typed.clone();
            self.address
                .update(cx, |input, cx| input.set_text(typed, cx));
        }
        self.address
            .update(cx, |input, cx| input.set_completion(None, cx));
        if self.suggestions.is_empty() && self.selected.is_none() {
            return;
        }
        self.suggestions.clear();
        self.selected = None;
        window.defer(cx, |_, cx| suggestions::close(cx));
        cx.notify();
    }
    /// Called by the dropdown when a row is clicked.
    fn open_suggestion(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(url) = self.suggestions.get(index).map(|s| s.url.clone()) {
            self.selected = Some(index);
            self.submit(&url, window, cx);
        }
    }
    /// Called by a row's × button: forgets the page and refills the list from the typed text.
    fn remove_suggestion(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(url) = self.suggestions.get(index).map(|s| s.url.clone()) else {
            return;
        };
        self.history.remove(&url);
        // Refresh first: `set_text` drops the painted bounds the dropdown is anchored to.
        self.refresh_suggestions(window, cx);
        let typed = self.typed.clone();
        self.address
            .update(cx, |input, cx| input.set_text(typed, cx));
    }
    /// Writes history at most once a second, off the UI thread.
    fn flush_history(&mut self, cx: &mut Context<Self>) {
        if self.last_save.elapsed() < Duration::from_secs(1) {
            return;
        }
        if let Some((path, contents)) = self.history.take_snapshot() {
            self.last_save = Instant::now();
            cx.background_executor()
                .spawn(async move {
                    if let Err(error) = history::write(&path, &contents) {
                        eprintln!("Could not save browsing history: {error}");
                    }
                })
                .detach();
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
        let requests: Vec<_> = self
            .agent
            .iter()
            .flat_map(|(requests, _)| requests.try_iter())
            .collect();
        for request in requests {
            self.handle_agent(request, window, cx);
        }
        let mut changed = false;
        let mut address_changed = false;
        let mut open = Vec::new();
        let mut icons = Vec::new();
        let mut page_focused = false;
        for (index, tab) in self.tabs.iter_mut().enumerate() {
            let Some(page) = &tab.page else {
                continue;
            };
            for event in page.drain_events() {
                changed = true;
                match event {
                    Event::Started(url) => {
                        tab.sound.clear();
                        tab.hovered_link = None;
                        tab.loading = true;
                        tab.error = None;
                        tab.url = url;
                    }
                    Event::Finished(url) => {
                        tab.loading = false;
                        tab.url = url;
                        tab.record_visit(&mut self.history);
                        if !tab.audio.is_default() {
                            tab.apply_audio();
                        }
                    }
                    Event::Title(title) => {
                        tab.title = title;
                        if !tab.loading && tab.recorded == tab.url {
                            self.history.set_title(&tab.url, &tab.title);
                        }
                    }
                    Event::Url(url) => {
                        tab.url = url;
                        // Same-document navigations (pushState) never finish a load.
                        if !tab.loading {
                            tab.record_visit(&mut self.history);
                        }
                        address_changed |= index == self.active;
                    }
                    Event::Icon(png) => icons.push((tab.url.clone(), png)),
                    Event::Open(url) => open.push((url, true)),
                    Event::OpenInBackground(url) => open.push((url, false)),
                    Event::Hover(url) => tab.hovered_link = url,
                    Event::Focused => page_focused |= index == self.active,
                    // A new document asks for the tab's settings; default ones need no script.
                    Event::Audio(AudioReport::Hello) => {
                        if !tab.audio.is_default() {
                            tab.apply_audio();
                        }
                    }
                    Event::Audio(AudioReport::Frame {
                        id,
                        audible,
                        limited,
                    }) => tab.sound.report(&id, audible, limited),
                }
            }
        }
        let address_focused = self.address.read(cx).focus_handle(cx).is_focused(window);
        if address_changed && !address_focused {
            self.sync_address(cx);
        }
        // Clicking the page while editing the address ends the edit, like other browsers.
        if page_focused && address_focused {
            window.focus(&self.focus);
            self.sync_address(cx);
        }
        for (url, activate) in open {
            self.open_link_tab(&url, activate, window, cx);
        }
        for (url, png) in icons {
            self.store_icon(&url, png, cx);
        }
        self.update_peek(window, cx);
        self.flush_history(cx);
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
        tab.hovered_link = None;
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
    fn open_link_tab(
        &mut self,
        url: &str,
        activate: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match NativePage::new(window, url) {
            Ok(page) => {
                let page = Rc::new(page);
                let mut tab = Tab::new(self.next_id);
                self.next_id += 1;
                tab.url = url.into();
                tab.home = false;
                tab.loading = true;
                tab.title = url::Url::parse(url)
                    .ok()
                    .and_then(|u| u.host_str().map(str::to_owned))
                    .unwrap_or_else(|| "Loading…".into());
                tab.page = Some(page.clone());
                let index = self.insert_tab(tab);
                if activate {
                    self.select_tab(index, window, cx);
                    window.on_next_frame(move |_, _| {
                        let _ = page.focus();
                    });
                }
            }
            Err(error) => self.tabs[self.active].error = Some(error),
        }
        cx.notify();
    }
    /// Called by the volume popover and the tab strip.
    fn set_audio(&mut self, id: u64, audio: TabAudio, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id)
            && tab.audio != audio
        {
            tab.audio = audio;
            tab.apply_audio();
            cx.notify();
        }
    }
    fn toggle_mute(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.iter().find(|tab| tab.id == id) {
            let audio = TabAudio {
                muted: !tab.audio.muted,
                ..tab.audio
            };
            self.set_audio(id, audio, cx);
        }
    }
    fn mute_tab(&mut self, _: &MuteTab, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle_mute(self.tabs[self.active].id, cx);
    }
    fn volume_controls(&mut self, _: &VolumeControls, window: &mut Window, cx: &mut Context<Self>) {
        open_volume(cx.weak_entity(), None, window, cx);
    }
    fn activate_tab_id(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.tabs.iter().position(|tab| tab.id == id) {
            self.select_tab(index, window, cx);
        }
    }
    fn sound_overview(&self) -> volume::Overview {
        let mut tabs: Vec<_> = self
            .tabs
            .iter()
            .enumerate()
            .filter(|(_, tab)| tab.sound.audible() || !tab.audio.is_default())
            .map(|(index, tab)| tab.sound(index == self.active))
            .collect();
        tabs.sort_by_key(|tab| (!tab.audible, !tab.active));
        volume::Overview {
            current: self.tabs[self.active].sound(true),
            tabs,
        }
    }
    fn select_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.tabs[self.active].hovered_link = None;
        if let Some(page) = &self.tabs[self.active].page {
            let _ = page.set_visible(false);
            let _ = page.focus_parent();
        }
        self.active = index;
        self.tabs[index].hovered_link = None;
        self.load_if_needed(index, window);
        self.reveal_active();
        window.focus(&self.focus);
        self.sync_address(cx);
        cx.notify();
    }
    /// New tabs open at the top of the unpinned list, right under the New tab row.
    fn insert_tab(&mut self, tab: Tab) -> usize {
        let at = self.pinned_count();
        self.tabs.insert(at, tab);
        if self.active >= at {
            self.active += 1;
        }
        at
    }
    /// Scrolls the sidebar just enough to show the active row.
    fn reveal_active(&self) {
        let pinned = self.pinned_count();
        // List children: the pinned grid (when pins exist), the New tab row, then the tabs.
        let row = if self.active < pinned {
            0
        } else {
            self.active - pinned + usize::from(pinned > 0) + 1
        };
        self.tab_scroll.scroll_to_item(row);
    }
    fn new_tab(&mut self, _: &NewTab, window: &mut Window, cx: &mut Context<Self>) {
        let tab = Tab::new(self.next_id);
        self.next_id += 1;
        let index = self.insert_tab(tab);
        self.select_tab(index, window, cx);
        self.focus_address(&FocusAddress, window, cx);
    }
    fn close_tab(&mut self, _: &CloseTab, window: &mut Window, cx: &mut Context<Self>) {
        self.close_tab_at(self.active, window, cx);
    }
    fn close_tab_at(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.hint = None;
        self.hovered_tab = None;
        if index == self.active {
            if let Some(page) = &self.tabs[index].page {
                let _ = page.focus_parent();
            }
            window.focus(&self.focus);
        }
        if let Some(url) = self.tabs[index].pinned.clone() {
            self.unload_pinned(index, url, window, cx);
            return;
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
        self.load_if_needed(self.active, window);
        self.reveal_active();
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
        tab.hovered_link = None;
        window.focus(&self.focus);
        self.sync_address(cx);
        cx.notify();
    }

    fn button(&self, id: &'static str, label: &'static str, p: Palette) -> Stateful<Div> {
        div()
            .id(id)
            .h(px(34.))
            .min_w(px(34.))
            .px_2()
            .rounded_lg()
            .flex()
            .items_center()
            .justify_center()
            .text_color(rgb(p.muted))
            .cursor_pointer()
            .hover(move |s| s.bg(rgb(p.elevated)).text_color(rgb(p.text)))
            .child(label)
    }
    fn volume_button(&self, p: Palette, cx: &mut Context<Self>) -> Stateful<Div> {
        let tab = &self.tabs[self.active];
        let audio = tab.audio;
        let color = if audio.muted {
            p.muted
        } else if audio.boosted() || tab.sound.audible() {
            p.primary
        } else {
            p.muted
        };
        div()
            .id("volume")
            .h(px(34.))
            .min_w(px(34.))
            .px_2()
            .gap_1()
            .rounded_lg()
            .flex()
            .items_center()
            .justify_center()
            .text_color(rgb(color))
            .text_size(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .cursor_pointer()
            .when(!audio.is_default(), |button| button.bg(rgb(p.elevated)))
            .hover(move |s| s.bg(rgb(p.elevated)).text_color(rgb(p.text)))
            .on_click(cx.listener(|_, event: &ClickEvent, window, cx| {
                let anchor = (!event.is_keyboard()).then(|| event.position().x);
                open_volume(cx.weak_entity(), anchor, window, cx);
            }))
            .child(
                brand::icon(volume::speaker_icon(audio))
                    .size(px(18.))
                    .text_color(rgb(color)),
            )
            .when_some(audio.badge(), |button, badge| button.child(badge))
    }
    fn home_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = theme::palette(cx);
        let phase = if theme::motion_allowed(cx) {
            self.art_phase
        } else {
            0.
        };
        let links = [
            (
                "W",
                "Wikipedia",
                "https://en.wikipedia.org/wiki/Main_Page",
                p.highlight,
            ),
            ("R", "Rust", "https://www.rust-lang.org", p.accent),
            (
                "Y",
                "Hacker News",
                "https://news.ycombinator.com",
                p.primary,
            ),
            ("G", "GitHub", "https://github.com", p.secondary),
        ];
        div()
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(rgb(p.background))
            .child(art::backdrop(p, phase).absolute().inset_0())
            .child(
                div()
                    .id("home-content")
                    .relative()
                    .size_full()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .min_h_full()
                            .w_full()
                            .flex()
                            .flex_col()
                            .items_center()
                            .justify_center()
                            .gap_3()
                            .py_5()
                            .px_6()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(p.muted))
                                    .child("SMALL BROWSER. BIG CURIOSITY."),
                            )
                            .child(
                                brand::mascot()
                                    .w(px(280.))
                                    .h(px(210.))
                                    .flex_shrink_0()
                                    .relative()
                                    .top(px((phase * 1.2).sin() * 6.)),
                            )
                            .child(
                                div()
                                    .font_family("Georgia")
                                    .italic()
                                    .text_size(px(48.))
                                    .line_height(px(52.))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(p.text))
                                    .child("Felifox"),
                            )
                            .child(
                                div()
                                    .text_size(px(17.))
                                    .text_color(rgb(p.muted))
                                    .child("Follow the thread. Find something wonderful."),
                            )
                            .child(
                                div()
                                    .id("home-search")
                                    .w(px(520.))
                                    .max_w_full()
                                    .h(px(48.))
                                    .flex_shrink_0()
                                    .px_5()
                                    .rounded_2xl()
                                    .bg(rgb(p.surface))
                                    .border_1()
                                    .border_color(rgb(p.border))
                                    .cursor_text()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .text_color(rgb(p.muted))
                                    .hover(move |s| s.border_color(rgb(p.primary)))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.focus_address(&FocusAddress, window, cx)
                                    }))
                                    .child("Search the web or enter an address")
                                    .child(div().text_xs().child("⌘ L")),
                            )
                            .child(div().flex().gap_3().mt_1().children(links.into_iter().map(
                                |(icon, name, url, color)| {
                                    div()
                                        .id(name)
                                        .w(px(120.))
                                        .h(px(84.))
                                        .flex_shrink_0()
                                        .rounded_xl()
                                        .bg(rgb(p.surface))
                                        .border_1()
                                        .border_color(rgb(p.border))
                                        .cursor_pointer()
                                        .hover(move |s| {
                                            s.bg(rgb(p.elevated)).border_color(rgb(color))
                                        })
                                        .flex()
                                        .flex_col()
                                        .items_center()
                                        .justify_center()
                                        .gap_2()
                                        .on_hover(cx.listener(
                                            move |this, hovered: &bool, _, cx| {
                                                this.tabs[this.active].hovered_link =
                                                    hovered.then(|| url.to_owned());
                                                cx.notify();
                                            },
                                        ))
                                        .on_click(cx.listener(
                                            move |this, event: &ClickEvent, window, cx| {
                                                let modifiers = event.modifiers();
                                                if modifiers.platform {
                                                    this.open_link_tab(
                                                        url,
                                                        modifiers.shift,
                                                        window,
                                                        cx,
                                                    );
                                                } else {
                                                    this.navigate(url, window, cx);
                                                }
                                            },
                                        ))
                                        .child(
                                            div()
                                                .size(px(30.))
                                                .rounded_lg()
                                                .bg(rgb(p.elevated))
                                                .text_color(rgb(p.text))
                                                .border_1()
                                                .border_color(rgb(color))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .font_weight(FontWeight::BOLD)
                                                .child(icon),
                                        )
                                        .child(div().text_xs().text_color(rgb(p.text)).child(name))
                                },
                            )))
                            .child(
                                div()
                                    .mt_2()
                                    .flex()
                                    .items_center()
                                    .gap_4()
                                    .text_xs()
                                    .text_color(rgb(p.muted))
                                    .child("⌘ T  New tab   ·   ⌘ L  Search")
                                    .child(
                                        div()
                                            .id("home-about")
                                            .cursor_pointer()
                                            .text_color(rgb(p.primary))
                                            .on_click(|_, _, cx| about::open(cx))
                                            .child(format!("About Felifox · {DISPLAY_VERSION}")),
                                    ),
                            ),
                    ),
            )
            .into_any_element()
    }
    /// Closing a pinned tab frees its page and resets it to the pinned URL; the pin stays.
    fn unload_pinned(
        &mut self,
        index: usize,
        url: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tab = &mut self.tabs[index];
        if let Some(page) = tab.page.take() {
            let _ = page.set_visible(false);
        }
        *tab = Tab::pinned(tab.id, url, std::mem::take(&mut tab.title));
        if index == self.active {
            if self.tabs.len() == 1 {
                self.tabs.push(Tab::new(self.next_id));
                self.next_id += 1;
            }
            let next = if index + 1 < self.tabs.len() {
                index + 1
            } else {
                index - 1
            };
            self.select_tab(next, window, cx);
        }
        cx.notify();
    }
    /// Shows `text` in the status bar while the element is hovered.
    fn hint_on_hover(
        text: &'static str,
        cx: &mut Context<Self>,
    ) -> impl Fn(&bool, &mut Window, &mut App) + 'static {
        cx.listener(move |this, hovered: &bool, _, cx| {
            if *hovered {
                this.hint = Some(text);
            } else if this.hint == Some(text) {
                this.hint = None;
            }
            cx.notify();
        })
    }
    /// A 24 × 24 icon control inside a tab row.
    fn row_control(id: (&'static str, u64), p: Palette) -> Stateful<Div> {
        div()
            .id(id)
            .size(px(24.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded_md()
            .text_color(rgb(p.muted))
            // `border` differs from both the selected and the hovered row fill in every theme.
            .hover(move |s| s.bg(rgb(p.border)).text_color(rgb(p.text)))
    }
    fn sidebar_button(&self, p: Palette, cx: &mut Context<Self>) -> Stateful<Div> {
        self.button("sidebar-toggle", "", p)
            .on_click(
                cx.listener(|this, _, window, cx| this.toggle_sidebar(&ToggleSidebar, window, cx)),
            )
            .on_hover(Self::hint_on_hover(
                if cx.global::<ThemeSettings>().sidebar {
                    "Hide sidebar · ⌘S"
                } else if self.peek {
                    "Keep sidebar open · ⌘S"
                } else {
                    "Show sidebar · ⌘S"
                },
                cx,
            ))
            .child(
                brand::icon(brand::icon::SIDEBAR)
                    .size(px(18.))
                    .text_color(rgb(p.muted)),
            )
    }
    /// An unpinned tab; pinned ones are `pinned_tile`s.
    fn tab_row(&self, index: usize, p: Palette, cx: &mut Context<Self>) -> Stateful<Div> {
        let tab = &self.tabs[index];
        let selected = self.active == index;
        let id = tab.id;
        let hovered = self.hovered_tab == Some(id);
        div()
            .id(("tab", id))
            .h(px(34.))
            .w_full()
            .flex_shrink_0()
            .px_2()
            .rounded_lg()
            // Selection is the strongest state: fill plus an outline, which light themes need
            // because their `elevated` is close to `background`. Hover only hints.
            .border_1()
            .border_color(rgb(if selected { p.border } else { p.background }))
            .bg(rgb(if selected { p.elevated } else { p.background }))
            .flex()
            .items_center()
            .gap_2()
            .cursor_pointer()
            .when(!selected, |row| {
                row.hover(move |s| s.bg(rgb(p.surface)).border_color(rgb(p.surface)))
            })
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.hovered_tab = Some(id);
                } else if this.hovered_tab == Some(id) {
                    this.hovered_tab = None;
                }
                cx.notify();
            }))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.select_tab(index, window, cx);
            }))
            .child(
                div()
                    .text_color(rgb(if selected { p.primary } else { p.muted }))
                    .child(if tab.loading && !tab.home {
                        div().child("◌").into_any_element()
                    } else if let Some(host) = (!tab.home).then(|| host(&tab.url)).flatten() {
                        self.site_icon(&host, 16., p)
                    } else {
                        brand::symbol().size(px(18.)).into_any_element()
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .text_sm()
                    .child(tab.display_title()),
            )
            .when(tab.sound.audible() || tab.audio.muted, |row| {
                let muted = tab.audio.muted;
                row.child(
                    Self::row_control(("tab-sound", id), p)
                        .on_hover(Self::hint_on_hover(
                            if muted {
                                "Unmute tab · ⌘⇧M"
                            } else {
                                "Mute tab · ⌘⇧M"
                            },
                            cx,
                        ))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.toggle_mute(id, cx);
                        }))
                        .child(
                            brand::icon(volume::speaker_icon(tab.audio))
                                .size(px(14.))
                                .text_color(rgb(if muted { p.muted } else { p.primary })),
                        ),
                )
            })
            // Only rendered while hovered, so a hidden pin can never catch a click.
            .when(hovered && tab.url() != HOME, |row| {
                row.child(
                    Self::row_control(("pin", id), p)
                        .on_hover(Self::hint_on_hover("Pin tab · ⌘D", cx))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.toggle_pin_at(index, cx);
                        }))
                        .child(
                            brand::icon(brand::icon::PIN)
                                .size(px(14.))
                                .text_color(rgb(p.muted)),
                        ),
                )
            })
            // × only on the selected or hovered row.
            .when(hovered || selected, |row| {
                row.child(
                    Self::row_control(("close", id), p)
                        .on_hover(Self::hint_on_hover("Close tab · ⌘W", cx))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.close_tab_at(index, window, cx);
                        }))
                        .child("×"),
                )
            })
    }
    /// Arc-style vertical tabs: pinned tabs, then a divider, New Tab and the other tabs.
    /// The sidebar's current width: `SIDEBAR_WIDTH` or 0, eased in between while it slides.
    fn sidebar_width(&self, cx: &App) -> f32 {
        let shown = self.sidebar_shown(cx);
        let t = self.sidebar_anim.map_or(1., |start| {
            (start.elapsed().as_secs_f32() / SIDEBAR_SLIDE.as_secs_f32()).min(1.)
        });
        let eased = 1. - (1. - t).powi(3);
        SIDEBAR_WIDTH * if shown { eased } else { 1. - eased }
    }
    /// Arc-style pinned tile: the site icon centred in a square-ish card, three per row.
    fn pinned_tile(&self, index: usize, p: Palette, cx: &mut Context<Self>) -> Stateful<Div> {
        let tab = &self.tabs[index];
        let selected = self.active == index;
        let id = tab.id;
        let hovered = self.hovered_tab == Some(id);
        div()
            .id(("pinned", id))
            .relative()
            .w(px(PINNED_TILE))
            .h(px(46.))
            .flex_shrink_0()
            .rounded_xl()
            .border_1()
            .border_color(rgb(if selected { p.border } else { p.surface }))
            .bg(rgb(if selected { p.elevated } else { p.surface }))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .when(!selected, |tile| {
                tile.hover(move |s| s.bg(rgb(p.elevated)).border_color(rgb(p.elevated)))
            })
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.hovered_tab = Some(id);
                } else if this.hovered_tab == Some(id) {
                    this.hovered_tab = None;
                }
                cx.notify();
            }))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.select_tab(index, window, cx);
            }))
            .child(
                div()
                    .when(tab.loading, |icon| icon.opacity(0.5))
                    .child(match host(&tab.url) {
                        Some(host) => self.site_icon(&host, 20., p),
                        None => brand::symbol().size(px(20.)).into_any_element(),
                    }),
            )
            // Unpinning lives on hover; ⌘W on a selected tile frees its page, keeping the pin.
            .when(hovered, |tile| {
                tile.child(
                    Self::row_control(("unpin", id), p)
                        .absolute()
                        .top(px(1.))
                        .right(px(1.))
                        .size(px(20.))
                        .on_hover(Self::hint_on_hover("Unpin tab · ⌘D", cx))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.toggle_pin_at(index, cx);
                        }))
                        .child(
                            brand::icon(brand::icon::PIN)
                                .size(px(12.))
                                .text_color(rgb(p.primary)),
                        ),
                )
            })
    }
    fn sidebar_view(&self, p: Palette, cx: &mut Context<Self>) -> Stateful<Div> {
        let pinned = self.pinned_count();
        div()
            .id("sidebar")
            .w(px(SIDEBAR_WIDTH))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(rgb(p.border))
            .child(
                // Leaves room for the traffic lights, level with the toolbar.
                div()
                    .h(px(64.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_end()
                    .px_3()
                    .child(self.sidebar_button(p, cx)),
            )
            .child(
                div()
                    .id("tab-list")
                    .track_scroll(&self.tab_scroll)
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_2()
                    .pb_2()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .when(pinned > 0, |list| {
                        list.child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap_2()
                                .pb_2()
                                .children((0..pinned).map(|index| self.pinned_tile(index, p, cx))),
                        )
                    })
                    .child(
                        // An action, not a tab: no fill or icon tile, quieter text, its shortcut.
                        div()
                            .id("sidebar-new-tab")
                            .h(px(30.))
                            .flex_shrink_0()
                            .px_2()
                            .rounded_lg()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_xs()
                            .text_color(rgb(p.muted))
                            .cursor_pointer()
                            .hover(move |s| s.bg(rgb(p.surface)).text_color(rgb(p.text)))
                            .on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.new_tab(&NewTab, window, cx)
                                }),
                            )
                            .child(
                                div()
                                    .w(px(18.))
                                    .flex()
                                    .justify_center()
                                    .text_sm()
                                    .child("+"),
                            )
                            .child(div().flex_1().child("New tab"))
                            .child("⌘T"),
                    )
                    .children((pinned..self.tabs.len()).map(|index| self.tab_row(index, p, cx))),
            )
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let address_focused = self.address.read(cx).focus_handle(cx).is_focused(window);
        let display = cx.global::<ThemeSettings>().address;
        let sidebar_width = self.sidebar_width(cx);
        if self
            .sidebar_anim
            .is_some_and(|start| start.elapsed() < SIDEBAR_SLIDE)
        {
            window.request_animation_frame();
        } else {
            self.sidebar_anim = None;
        }
        // The traffic lights cover the window's first 88 px; once the sidebar is narrower than
        // that, the toolbar carries the toggle just right of them.
        let toolbar_toggle = sidebar_width < 88.;
        let tab = &self.tabs[self.active];
        let show_chip = !tab.home && !address_focused && display != AddressDisplay::Url;
        let back_enabled = tab.can_go_back();
        let forward_enabled = tab.can_go_forward();
        // A hovered sidebar row shows its full title and address, which the row truncates.
        let hovered_row = self
            .hovered_tab
            .and_then(|id| self.tabs.iter().find(|tab| tab.id == id))
            .filter(|tab| !tab.home)
            .map(|tab| {
                format!(
                    "{} · {}",
                    tab.display_title(),
                    history::display_url(&tab.url)
                )
            });
        let status = self
            .hint
            .map(str::to_owned)
            .or(hovered_row)
            .unwrap_or_else(|| {
                tab.hovered_link
                    .as_deref()
                    .or(tab.error.as_deref())
                    .unwrap_or(if tab.loading && !tab.home {
                        "Loading · WebKit"
                    } else {
                        "WebKit · JavaScript enabled"
                    })
                    .to_owned()
            });
        div()
            .id("felifox")
            .track_focus(&self.focus)
            .key_context("Browser")
            .size_full()
            .bg(rgb(p.background))
            .text_color(rgb(p.text))
            .font_family(".AppleSystemUIFont")
            .flex()
            .on_action(cx.listener(Self::new_tab))
            .on_action(cx.listener(Self::close_tab))
            .on_action(cx.listener(Self::focus_address))
            .on_action(cx.listener(Self::back))
            .on_action(cx.listener(Self::forward))
            .on_action(cx.listener(Self::reload))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::mute_tab))
            .on_action(cx.listener(Self::volume_controls))
            .on_action(cx.listener(Self::toggle_sidebar))
            .on_action(cx.listener(Self::pin_tab))
            .on_action(cx.listener(Self::activate_tab))
            .on_action(cx.listener(Self::next_tab))
            .on_action(cx.listener(Self::previous_tab))
            .when(sidebar_width > 0., |root| {
                // A clipping frame slides; the sidebar inside keeps its width, so nothing reflows.
                root.child(
                    div()
                        .w(px(sidebar_width))
                        .h_full()
                        .flex_shrink_0()
                        .overflow_hidden()
                        .child(self.sidebar_view(p, cx)),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .h(px(64.))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .px_4()
                            .gap_2()
                            .bg(rgb(p.surface))
                            .when(toolbar_toggle, |bar| {
                                bar.pl(px((88. - sidebar_width).max(16.)))
                                    .child(self.sidebar_button(p, cx))
                            })
                            .child(
                                self.button("back", "←", p)
                                    .opacity(if back_enabled { 1. } else { 0.35 })
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.back(&Back, window, cx)
                                    })),
                            )
                            .child(
                                self.button("forward", "→", p)
                                    .opacity(if forward_enabled { 1. } else { 0.35 })
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.forward(&Forward, window, cx)
                                    })),
                            )
                            .child(self.button("reload", "↻", p).on_click(
                                cx.listener(|this, _, window, cx| this.reload(&Reload, window, cx)),
                            ))
                            .child(self.button("home", "⌂", p).on_click(
                                cx.listener(|this, _, window, cx| this.home(&Home, window, cx)),
                            ))
                            .child(div().flex_1().min_w_0().child(if show_chip {
                                self.title_chip(display, p, cx)
                            } else {
                                self.address.clone().into_any_element()
                            }))
                            .child(self.volume_button(p, cx))
                            .child(
                                self.button("appearance", "◈ Theme ▾", p)
                                    .text_size(px(12.))
                                    .on_click(|_, window, cx| appearance::open(window, cx)),
                            )
                            .child(
                                self.button("about", "ⓘ", p)
                                    .on_click(|_, _, cx| about::open(cx)),
                            ),
                    )
                    .child(div().h(px(2.)).flex_shrink_0().bg(rgb(if tab.loading {
                        p.primary
                    } else {
                        p.border
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
                            .text_color(rgb(p.muted))
                            .gap_4()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .child(status),
                            )
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .child(format!("FELIFOX  /  {DISPLAY_VERSION}")),
                            ),
                    ),
            )
    }
}
/// The popover draws as soon as its window opens and reads the browser while doing so, so it
/// must not be opened from inside one of the browser's own updates (click or action handlers).
fn open_volume(
    browser: WeakEntity<Browser>,
    anchor: Option<Pixels>,
    window: &mut Window,
    cx: &mut Context<Browser>,
) {
    window.defer(cx, move |window, cx| {
        volume::toggle(browser, anchor, window, cx)
    });
}

fn main() {
    Application::new()
        .with_assets(brand::BrandAssets)
        .run(|cx: &mut App| {
            cx.set_global(ThemeSettings::load());
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
                KeyBinding::new("cmd-shift-m", MuteTab, None),
                KeyBinding::new("cmd-s", ToggleSidebar, None),
                KeyBinding::new("cmd-d", PinTab, None),
                KeyBinding::new("ctrl-tab", NextTab, None),
                KeyBinding::new("ctrl-shift-tab", PreviousTab, None),
                KeyBinding::new("cmd-shift-]", NextTab, None),
                KeyBinding::new("cmd-shift-[", PreviousTab, None),
                KeyBinding::new("cmd-9", ActivateTab(LAST_TAB), None),
            ]);
            cx.bind_keys(
                (1..=8).map(|n| KeyBinding::new(&format!("cmd-{n}"), ActivateTab(n - 1), None)),
            );
            // Native menu equivalents remain available while WKWebView owns keyboard focus.
            cx.set_menus(vec![
                Menu {
                    name: "Felifox".into(),
                    items: vec![
                        MenuItem::action("About Felifox", AboutFelifox),
                        MenuItem::separator(),
                        MenuItem::action("Quit Felifox", Quit),
                    ],
                },
                Menu {
                    name: "File".into(),
                    items: vec![
                        MenuItem::action("New Tab", NewTab),
                        MenuItem::action("Close Tab", CloseTab),
                        MenuItem::action("Pin / Unpin Tab", PinTab),
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
                    name: "View".into(),
                    // Menu items keep these working while the page has keyboard focus.
                    items: [
                        MenuItem::action("Toggle Sidebar", ToggleSidebar),
                        MenuItem::separator(),
                        MenuItem::action("Show Next Tab", NextTab),
                        MenuItem::action("Show Previous Tab", PreviousTab),
                        MenuItem::separator(),
                    ]
                    .into_iter()
                    .chain(
                        (1..=8).map(|n| MenuItem::action(format!("Tab {n}"), ActivateTab(n - 1))),
                    )
                    .chain([MenuItem::action("Last Tab", ActivateTab(LAST_TAB))])
                    .collect(),
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
                Menu {
                    name: "Sound".into(),
                    items: vec![
                        MenuItem::action("Volume Controls…", VolumeControls),
                        MenuItem::action("Mute / Unmute Tab", MuteTab),
                    ],
                },
            ]);
            cx.on_action(|_: &AboutFelifox, cx| about::open(cx));
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
                        // Centred on the 64 px sidebar header and toolbar rows.
                        traffic_light_position: Some(point(px(16.), px(25.))),
                    }),
                    ..Default::default()
                },
                |window, cx| cx.new(|cx| Browser::new(window, cx)),
            )
            .expect("Could not open Felifox window");
            cx.activate(true);
        });
}
