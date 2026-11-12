//! The user agent Felinaut's pages send: WebKit's own, plus the Safari version tokens.
//!
//! A bare `WKWebView` reports `… AppleWebKit/605.1.15 (KHTML, like Gecko)` with no `Version/…
//! Safari/…` suffix. Sites read that as an unknown embedded view: Google, for one, serves a
//! stripped-down legacy results page without its Sign in button. Felinaut renders with the
//! system WebKit that the installed Safari also uses, so it reports that Safari's version.

use std::sync::OnceLock;

/// WebKit's own user agent on macOS. Apple froze the OS and WebKit tokens for privacy, so they
/// no longer change between releases.
const WEBKIT: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)";
/// Safari's own build token, frozen the same way.
const SAFARI_TOKEN: &str = "605.1.15";
/// Used when the installed Safari's version cannot be read.
const FALLBACK_VERSION: &str = "26.0";
const SAFARI_INFO: &str = "/Applications/Safari.app/Contents/Info.plist";

/// The user agent for `version` (`major.minor`).
pub fn user_agent_for(version: &str) -> String {
    format!("{WEBKIT} Version/{version} Safari/{SAFARI_TOKEN}")
}

/// Safari's `major.minor` from its XML `Info.plist`, or `None` if it is missing or malformed.
pub fn safari_version(info_plist: &str) -> Option<String> {
    let after_key = info_plist
        .split("<key>CFBundleShortVersionString</key>")
        .nth(1)?;
    let value = after_key
        .trim_start()
        .strip_prefix("<string>")?
        .split("</string>")
        .next()?
        .trim();
    let mut parts = value.split('.');
    let major = parts.next()?;
    let minor = parts.next().unwrap_or("0");
    let number =
        |part: &str| (1..=3).contains(&part.len()) && part.bytes().all(|b| b.is_ascii_digit());
    (number(major) && number(minor) && parts.all(number)).then(|| format!("{major}.{minor}"))
}

/// The user agent every Felinaut page uses. Reads Safari's version once, on first use.
pub fn user_agent() -> &'static str {
    static AGENT: OnceLock<String> = OnceLock::new();
    AGENT.get_or_init(|| {
        let version = std::fs::read_to_string(SAFARI_INFO)
            .ok()
            .and_then(|plist| safari_version(&plist))
            .unwrap_or_else(|| FALLBACK_VERSION.to_owned());
        user_agent_for(&version)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plist(version: &str) -> String {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\">\n<dict>\n\
             \t<key>CFBundleName</key>\n\t<string>Safari</string>\n\
             \t<key>CFBundleShortVersionString</key>\n\t<string>{version}</string>\n\
             \t<key>CFBundleVersion</key>\n\t<string>21624.5.1.11.3</string>\n</dict>\n</plist>\n"
        )
    }

    #[test]
    fn matches_safaris_own_user_agent_shape() {
        assert_eq!(
            user_agent_for("26.6"),
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 \
             (KHTML, like Gecko) Version/26.6 Safari/605.1.15"
        );
        // What sites look for: WebKit first, then Safari's version and build tokens last.
        let agent = user_agent_for("18.3");
        assert!(agent.starts_with(WEBKIT));
        assert!(agent.ends_with(" Version/18.3 Safari/605.1.15"));
        assert!(!agent.contains("Chrome") && !agent.contains("Felinaut"));
    }

    #[test]
    fn reads_major_and_minor_from_safaris_info_plist() {
        for (version, expected) in [
            ("26.6.2", "26.6"),
            ("26.6", "26.6"),
            ("18.3.1", "18.3"),
            ("26", "26.0"),
            (" 26.1 ", "26.1"),
        ] {
            assert_eq!(
                safari_version(&plist(version)).as_deref(),
                Some(expected),
                "{version}"
            );
        }
    }

    #[test]
    fn rejects_missing_or_malformed_versions() {
        for version in [
            "",
            "beta",
            "26.x",
            "26..1",
            ".6",
            "26.6 beta",
            "26.6.2.1.9-rc",
            "1234.5",
        ] {
            assert_eq!(safari_version(&plist(version)), None, "{version:?}");
        }
        for text in [
            "",
            "not a plist",
            "<key>CFBundleShortVersionString</key>",
            "<key>CFBundleShortVersionString</key><integer>26</integer>",
            "bplist00\u{0}\u{1}binary",
        ] {
            assert_eq!(safari_version(text), None, "{text:?}");
        }
    }

    #[test]
    fn the_shared_user_agent_is_always_well_formed() {
        // Whatever this machine has installed, the result has Safari's shape.
        let agent = user_agent();
        let version = agent
            .strip_prefix(&format!("{WEBKIT} Version/"))
            .and_then(|rest| rest.strip_suffix(&format!(" Safari/{SAFARI_TOKEN}")))
            .expect("Safari-shaped user agent");
        let (major, minor) = version.split_once('.').expect("major.minor");
        assert!(
            major.parse::<u16>().is_ok() && minor.parse::<u16>().is_ok(),
            "{version}"
        );
        assert!(std::ptr::eq(agent, user_agent()), "read once and shared");
    }
}
