//! Native pages live on GPUI's UI thread and must be dropped before their parent window.

use std::cell::RefCell;
use std::sync::mpsc::{self, Receiver};

use gpui::{Bounds, Pixels};
use wry::{NewWindowResponse, PageLoadEvent, WebView, WebViewBuilder};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Started(String),
    Finished(String),
    Title(String),
    Url(String),
    Open(String),
}

/// One persistent native child view per tab; hiding it does not discard its history or document.
/// The owner should poll `drain_events` on the UI thread, including for inactive tabs.
pub struct NativePage {
    webview: WebView,
    events: Receiver<Event>,
    last_url: RefCell<String>,
}

impl NativePage {
    /// Creates an initially hidden, unfocused child. Set its canvas bounds before showing it.
    pub fn new(window: &gpui::Window, url: &str) -> Result<Self, String> {
        check_url(url)?;
        let (sender, events) = mpsc::channel();
        let load_sender = sender.clone();
        let title_sender = sender.clone();
        let webview = WebViewBuilder::new()
            .with_visible(false)
            .with_focused(false)
            .with_navigation_handler(|url| allowed_url(&url))
            .with_new_window_req_handler(move |url, _| {
                if allowed_url(&url) {
                    let _ = sender.send(Event::Open(url));
                }
                // Main owns tab creation; never let WebKit spawn an external window.
                NewWindowResponse::Deny
            })
            .with_document_title_changed_handler(move |title| {
                let _ = title_sender.send(Event::Title(title));
            })
            .with_on_page_load_handler(move |event, url| {
                // Wry has no load-failure callback. Finished means an actual native finish,
                // not a timeout or a failed provisional navigation.
                let event = match event {
                    PageLoadEvent::Started => Event::Started(url),
                    PageLoadEvent::Finished => Event::Finished(url),
                };
                let _ = load_sender.send(event);
            })
            .with_url(url)
            .build_as_child(window)
            .map_err(|error| error.to_string())?;

        Ok(Self {
            webview,
            events,
            last_url: RefCell::new(String::new()),
        })
    }

    pub fn navigate(&self, url: &str) -> Result<(), String> {
        check_url(url)?;
        self.webview
            .load_url(url)
            .map_err(|error| error.to_string())
    }

    pub fn back(&self) -> Result<(), String> {
        self.webview.go_back().map_err(|error| error.to_string())
    }

    pub fn forward(&self) -> Result<(), String> {
        self.webview.go_forward().map_err(|error| error.to_string())
    }

    pub fn reload(&self) -> Result<(), String> {
        self.webview.reload().map_err(|error| error.to_string())
    }

    /// Uses WKWebView's native history, not a JavaScript or application-side history mirror.
    pub fn can_go_back(&self) -> bool {
        self.webview.can_go_back().unwrap_or(false)
    }

    pub fn can_go_forward(&self) -> bool {
        self.webview.can_go_forward().unwrap_or(false)
    }

    pub fn set_visible(&self, visible: bool) -> Result<(), String> {
        self.webview
            .set_visible(visible)
            .map_err(|error| error.to_string())
    }

    pub fn focus(&self) -> Result<(), String> {
        self.webview.focus().map_err(|error| error.to_string())
    }

    pub fn focus_parent(&self) -> Result<(), String> {
        self.webview
            .focus_parent()
            .map_err(|error| error.to_string())
    }

    /// Bounds are window-relative logical pixels, as supplied by the GPUI canvas.
    pub fn set_bounds(&self, bounds: Bounds<Pixels>) -> Result<(), String> {
        self.webview
            .set_bounds(wry::Rect {
                position: wry::dpi::LogicalPosition::new(
                    f64::from(bounds.origin.x),
                    f64::from(bounds.origin.y),
                )
                .into(),
                size: wry::dpi::LogicalSize::new(
                    f64::from(bounds.size.width),
                    f64::from(bounds.size.height),
                )
                .into(),
            })
            .map_err(|error| error.to_string())
    }

    pub fn drain_events(&self) -> Vec<Event> {
        let mut events: Vec<_> = self.events.try_iter().collect();
        // Poll the committed native URL: the navigation-policy callback also sees
        // subframes and attempted navigations, and misses history.pushState changes.
        if let Ok(url) = self.webview.url() {
            let mut last_url = self.last_url.borrow_mut();
            if !url.is_empty() && *last_url != url {
                last_url.clone_from(&url);
                events.push(Event::Url(url));
            }
        }
        events
    }
}

fn allowed_url(url: &str) -> bool {
    url.split_once(':').is_some_and(|(scheme, _)| {
        scheme.eq_ignore_ascii_case("http")
            || scheme.eq_ignore_ascii_case("https")
            || scheme.eq_ignore_ascii_case("about")
    })
}

fn check_url(url: &str) -> Result<(), String> {
    if allowed_url(url) {
        Ok(())
    } else {
        Err("Only http, https, and about navigation is allowed".into())
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn permits_only_web_and_about_schemes() {
        for url in ["http://example.com", "HTTPS://example.com", "about:blank"] {
            assert!(super::allowed_url(url), "{url}");
        }
        for url in [
            "javascript:alert(1)",
            "data:text/html,test",
            "file:///etc/passwd",
            "mailto:test@example.com",
            "custom:open",
            "example.com",
            "//example.com",
            " https://example.com",
            "ht\ntps://example.com",
            "",
        ] {
            assert!(!super::allowed_url(url), "{url}");
        }
    }
}
