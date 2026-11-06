# Felifox · 0.1 beta

A simple Rust desktop browser: **GPUI for the interface, Wry / WKWebView for the web**. On macOS, WebKit handles HTML, CSS, JavaScript, images, forms, networking and web storage. There is no bundled browser engine and no custom page renderer.

## Run

Currently targets **macOS**. Install a recent Rust toolchain (Rust 1.94 or later recommended) and Xcode command-line tools. GPUI's Metal shader build may require full Xcode with its Metal toolchain installed.

```sh
cargo run
```

For an optimized executable:

```sh
cargo build --release
./target/release/felifox
```

The first build downloads dependencies from crates.io. The new-tab page contains no remote assets or telemetry. Search queries go to DuckDuckGo. Websites control their own requests once loaded.

## Controls

- Native address input with Unicode, IME, selection and clipboard.
- Tabs retain their own live webview and native navigation history when switched.
- Back, forward, reload and home; native page titles and URLs update the chrome.
- New-window links open as tabs instead of unmanaged native windows.
- Native menus keep browser shortcuts available while the webview has focus.

| Shortcut    | Action                       |
| ----------- | ---------------------------- |
| `⌘T`        | New tab                      |
| `⌘W`        | Close active tab             |
| `⌘L`        | Focus address and select all |
| `⌘R`        | Reload                       |
| `⌘[` / `⌘]` | Back / forward               |
| `⌘⇧H`       | Home                         |
| `⌘Q`        | Quit                         |

Home is a GPUI page outside WebKit's history. Going back from the first web page shows Home; Forward restores the retained web page. Closing a tab destroys its webview. Opening a new tab does not allocate a webview until you navigate.

## Artwork and appearance

The toolbar's **Theme ▾** button opens five curated palettes: Midnight Party (default), Confetti Sunset, Neon Wool, Arctic Pop, and Forest Carnival. Themes recolor Felifox's chrome, address field, New Tab page and About window—not websites. Theme and animation choices currently last for the session only.

- **Original embedded SVG artwork:** a mischievous midnight-purple cat playing with a flaming wool ball, plus a simplified flaming-yarn symbol. No fox emoji, external assets or large background bitmaps.
- **Procedural New Tab and About backgrounds:** responsive gradient ribbons, cloud scallops, orbital yarn trails and four-point sparkles drawn with GPUI paths. Their geometry moves slowly rather than cycling through image frames.
- **Motion:** the dropdown includes an Animate artwork checkbox. Animation stops while the window is inactive or web content replaces New Tab, and follows the live macOS Reduce Motion setting. Active animation is limited to about 30 fps; pausing does not affect website animation.
- **About:** open **Felifox → About Felifox**, click the toolbar's **ⓘ**, or use the About link on New Tab. The independently closable window displays **0.1 beta**, the mascot, and Rust / GPUI / WebKit credits. Escape or Cmd-W closes About, not the browser.

The theme chooser is a small native popup, so it appears correctly above the embedded WebKit view without hiding or reloading the page. The artwork is original vector work inspired by the supplied design direction, not a copy of the reference illustrations.

## Architecture

```text
GPUI: tabs, address bar, controls, new-tab page
                     │
              Wry native child view
                     │
         macOS WebKit + JavaScriptCore
```

A GPUI canvas supplies the webview's window-relative logical bounds so it stays below the toolbar and above the status bar during resizing. Hidden tabs keep their documents and JavaScript state. Native callbacks enqueue events, which the UI drains every 100 ms; they never re-enter GPUI while it is rendering.

The previous WebRender/SWGL pipeline, HTML extraction, custom HTTP loader and `--render-check` command have been removed. WebKit now owns the entire web-content pipeline.

## Scope and limitations

Keep it small: no bookmarks manager, extensions, download UI, permission UI, session restoration, or private-mode promise. WebKit's normal website storage is enabled; cookies and local storage may persist independently of the in-memory tab list.

- Compatibility is **WebKit compatibility**, not Chromium compatibility. Websites requiring specific browser integrations, DRM, permissions, or custom URL schemes may still fail.
- Top-level navigation permits HTTP(S) and `about:` pages; file, data, JavaScript and external-app schemes are blocked. This is not a general request filter: WebKit manages subresources and web APIs.
- Wry's page-load callback does not expose native load failures. Some failed loads may leave the loading status displayed; Felifox does not yet provide a custom network-error page.
- This is an early browser shell, not a security-audited Safari replacement. WebKit's engine security updates come from macOS updates.
- Only macOS is configured and tested. Windows and Linux backends are not configured.

## Validate

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
# Opt-in native integration test (opens a temporary window, loopback traffic only):
FELIFOX_NATIVE_TEST=1 cargo test --test native_webview
# Native artwork, theme-handler and About-window checks:
FELIFOX_NATIVE_TEST=1 cargo test --test brand_ui
```

The native test verifies actual WebKit JavaScript execution, DOM title changes, localStorage, two native webviews, native back/forward, reload, bounds, visibility and focus operations. It is skipped by default because WebKit needs a macOS desktop session and main-thread event loop.

## Source

- `src/main.rs`: GPUI window, tabs and navigation controls.
- `src/input.rs`: address input adapted from Zed's Apache-2.0 GPUI input example.
- `src/engine.rs`: address-to-URL/search resolution only.
- `src/webview.rs`: Wry child views, native callbacks, history, bounds and focus.
- `src/theme.rs`: shared palettes, contrast checks and native Reduce Motion support.
- `src/art.rs`: procedural background geometry; no textures or animation framework.
- `src/brand.rs` and `assets/`: embedded original SVG mascot and symbol.
- `src/appearance.rs`: native theme dropdown and animation control.
- `src/about.rs`: themed About window showing 0.1 beta.
- `tests/native_webview.rs`: opt-in native JavaScript integration test.
- `tests/brand_ui.rs`: opt-in SVG decode, theme-handler, motion-toggle and About lifecycle checks.

Felifox source is MIT licensed. Dependencies retain their own licenses, including GPUI (Apache-2.0) and Wry (Apache-2.0 OR MIT).
