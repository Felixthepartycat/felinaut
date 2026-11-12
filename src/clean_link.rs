//! "Copy Clean Link" (⌘⇧C): the page address without tracking parameters.
//!
//! Only parameters that exist to track clicks and campaigns are removed; everything that
//! selects content (a search query, a branch, a video time) stays. Kept parameters keep their
//! original order and percent-encoding, so the link opens exactly the same page.

use url::{Url, form_urlencoded};

/// Parameters removed on every site, compared case-insensitively.
const TRACKING: &[&str] = &[
    // Ad click identifiers.
    "fbclid",
    "gclid",
    "gclsrc",
    "dclid",
    "gbraid",
    "wbraid",
    "gad_source",
    "gad_campaignid",
    "msclkid",
    "twclid",
    "ttclid",
    "li_fat_id",
    "yclid",
    "ysclid",
    "rb_clickid",
    "srsltid",
    "epik",
    // Analytics and email campaigns.
    "_ga",
    "_gl",
    "mc_cid",
    "mc_eid",
    "_hsenc",
    "_hsmi",
    "__hstc",
    "__hssc",
    "__hsfp",
    "hsctatracking",
    "mkt_tok",
    "_kx",
    "vero_id",
    "vero_conv",
    "oly_anon_id",
    "oly_enc_id",
    "s_cid",
    "wickedid",
    // Social share identifiers.
    "igshid",
    "igsh",
];

/// Parameter name prefixes removed on every site.
const TRACKING_PREFIXES: &[&str] = &["utm_", "pk_", "mtm_", "hsa_"];

/// Parameters that only track on particular sites; elsewhere the same names can mean
/// something real, so they are kept.
fn site_tracking(host: &str, path: &str, name: &str) -> bool {
    let host = host.trim_end_matches('.');
    let on = |domain: &str| host == domain || host.ends_with(&format!(".{domain}"));
    if on("youtube.com") || on("youtu.be") {
        return matches!(name, "si" | "feature" | "pp");
    }
    if on("spotify.com") {
        return name == "si";
    }
    if is_google(host) && path == "/search" {
        return matches!(
            name,
            "ved"
                | "ei"
                | "sei"
                | "oq"
                | "gs_lcrp"
                | "gs_lp"
                | "gs_ssp"
                | "sclient"
                | "sourceid"
                | "uact"
                | "sca_esv"
                | "sca_upv"
                | "sxsrf"
                | "iflsig"
                | "rlz"
                | "biw"
                | "bih"
                | "aqs"
                | "client"
                | "sa"
                | "source"
        );
    }
    if is_amazon(host) {
        return matches!(
            name,
            "ref" | "ref_" | "qid" | "sr" | "crid" | "sprefix" | "content-id" | "_encoding"
        ) || name.starts_with("pd_rd_")
            || name.starts_with("pf_rd_");
    }
    false
}

/// `<brand>.<tld>` behind one of `prefixes` (or none), with one- or two-label endings such as
/// `google.de` or `google.co.uk`, but not `google.evil.example`.
fn is_brand_site(host: &str, brand: &str, prefixes: &[&str]) -> bool {
    let host = prefixes
        .iter()
        .find_map(|prefix| host.strip_prefix(prefix))
        .unwrap_or(host);
    host.strip_prefix(brand)
        .and_then(|rest| rest.strip_prefix('.'))
        .is_some_and(|tld| {
            let labels: Vec<_> = tld.split('.').collect();
            labels.len() <= 2
                && labels
                    .iter()
                    .all(|label| !label.is_empty() && label.chars().all(|c| c.is_ascii_alphabetic()))
                // Two-label endings are country forms such as co.uk or com.au.
                && (labels.len() == 1 || labels[1].len() == 2)
        })
}

fn is_google(host: &str) -> bool {
    is_brand_site(host, "google", &["www."])
}

fn is_amazon(host: &str) -> bool {
    is_brand_site(host, "amazon", &["www.", "smile."])
}

fn is_tracking(host: &str, path: &str, name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    TRACKING.contains(&name.as_str())
        || TRACKING_PREFIXES
            .iter()
            .any(|prefix| name.starts_with(prefix))
        || site_tracking(host, path, &name)
}

/// The cleaned link for a web page address, or `None` for anything that is not an
/// http(s) URL (the New Tab page, an empty tab, malformed input).
pub fn clean(address: &str) -> Option<String> {
    let mut url = Url::parse(address).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let host = url.host_str()?.to_ascii_lowercase();

    // Amazon also puts its tracking in a trailing `/ref=…` path segment.
    if is_amazon(&host)
        && let Some(index) = url.path().rfind("/ref=")
        && !url.path()[index + 1..].contains('/')
    {
        let path = url.path()[..index].to_owned();
        url.set_path(if path.is_empty() { "/" } else { &path });
    }

    let path = url.path().to_owned();
    if let Some(query) = url.query() {
        let kept: Vec<&str> = query
            .split('&')
            .filter(|part| !part.is_empty())
            .filter(|part| {
                // Decode only to compare the name; the original text is what is kept.
                let name = form_urlencoded::parse(part.as_bytes())
                    .next()
                    .map(|(name, _)| name.into_owned())
                    .unwrap_or_default();
                !is_tracking(&host, &path, &name)
            })
            .collect();
        let kept = kept.join("&");
        url.set_query((!kept.is_empty()).then_some(kept.as_str()));
    }

    // Text fragments (`#:~:text=…`) record what was highlighted, not where the page is.
    if let Some(fragment) = url.fragment()
        && let Some(index) = fragment.find(":~:")
    {
        let anchor = fragment[..index].to_owned();
        url.set_fragment((!anchor.is_empty()).then_some(anchor.as_str()));
    }
    Some(url.into())
}

#[cfg(test)]
mod tests {
    use super::clean;

    fn check(input: &str, expected: &str) {
        assert_eq!(clean(input).as_deref(), Some(expected), "{input}");
    }

    #[test]
    fn removes_campaign_and_click_parameters_everywhere() {
        check(
            "https://example.com/post?utm_source=x&utm_medium=email&utm_campaign=fall",
            "https://example.com/post",
        );
        check(
            "https://example.com/?fbclid=abc&gclid=1&msclkid=2&_ga=3&mc_eid=4",
            "https://example.com/",
        );
        check(
            "https://shop.example/item?id=7&utm_source=newsletter&color=red",
            "https://shop.example/item?id=7&color=red",
        );
        // Names are matched case-insensitively and after percent-decoding.
        check(
            "https://example.com/a?UTM_Source=x&utm%5Fmedium=y&FBCLID=z&b=1",
            "https://example.com/a?b=1",
        );
        check(
            "https://example.com/a?pk_campaign=x&mtm_kwd=y",
            "https://example.com/a",
        );
    }

    #[test]
    fn keeps_everything_that_selects_content_exactly_as_written() {
        for url in [
            "https://github.com/felinaut/felinaut/blob/main/README.md?ref=main",
            "https://example.com/search?q=caf%C3%A9+au+lait&page=2",
            "https://example.com/a?x=1+2&y=%20&z",
            "https://example.com/watch?v=abc&t=42",
            "https://example.com/?si=keep",
            "https://example.com/?feature=keep",
            "https://example.com/doc#section-2",
            "http://localhost:3000/dev?debug=1",
            "https://example.com/",
        ] {
            check(url, url);
        }
    }

    #[test]
    fn keeps_parameter_order_and_drops_an_empty_query() {
        check(
            "https://example.com/?b=2&utm_source=x&a=1&gclid=y&c=3",
            "https://example.com/?b=2&a=1&c=3",
        );
        check(
            "https://example.com/path?utm_source=x",
            "https://example.com/path",
        );
        check(
            "https://example.com/path?&&utm_source=x&&",
            "https://example.com/path",
        );
        check("https://example.com/path?", "https://example.com/path");
    }

    #[test]
    fn removes_site_specific_trackers_only_on_their_sites() {
        check(
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ&si=AbC123&feature=shared&t=10",
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=10",
        );
        check(
            "https://youtu.be/dQw4w9WgXcQ?si=AbC123",
            "https://youtu.be/dQw4w9WgXcQ",
        );
        check(
            "https://open.spotify.com/track/123?si=abcdef",
            "https://open.spotify.com/track/123",
        );
        check(
            "https://www.google.com/search?q=what&hl=en&sei=x&ved=y&oq=wha&gs_lcrp=z&sourceid=chrome&ie=UTF-8",
            "https://www.google.com/search?q=what&hl=en&ie=UTF-8",
        );
        check(
            "https://www.google.co.uk/search?q=tea&ei=x",
            "https://www.google.co.uk/search?q=tea",
        );
        // The same names elsewhere, or off Google's search page, are left alone.
        check(
            "https://example.com/search?q=x&ved=y",
            "https://example.com/search?q=x&ved=y",
        );
        check(
            "https://www.google.com/maps?q=x&ei=y",
            "https://www.google.com/maps?q=x&ei=y",
        );
        check(
            "https://notgoogle.com/search?ei=y",
            "https://notgoogle.com/search?ei=y",
        );
        check(
            "https://google.evil.example/search?ei=y",
            "https://google.evil.example/search?ei=y",
        );
    }

    #[test]
    fn removes_amazon_referral_paths_and_parameters() {
        check(
            "https://www.amazon.com/dp/B000123/ref=sr_1_1?crid=X&keywords=kettle&qid=1&sr=8-1&th=1",
            "https://www.amazon.com/dp/B000123?keywords=kettle&th=1",
        );
        check(
            "https://www.amazon.de/gp/product/B0001?pd_rd_w=a&pf_rd_p=b&psc=1",
            "https://www.amazon.de/gp/product/B0001?psc=1",
        );
        // A `ref=` that is not the last path segment is part of the address.
        check(
            "https://www.amazon.com/ref=nav_logo/deals",
            "https://www.amazon.com/ref=nav_logo/deals",
        );
        check(
            "https://example.com/dp/1/ref=sr_1_1?ref=x",
            "https://example.com/dp/1/ref=sr_1_1?ref=x",
        );
    }

    #[test]
    fn removes_text_fragments_but_keeps_anchors() {
        check(
            "https://example.com/article#:~:text=hello%20world",
            "https://example.com/article",
        );
        check(
            "https://example.com/article#intro:~:text=hello",
            "https://example.com/article#intro",
        );
        check(
            "https://example.com/article#intro",
            "https://example.com/article#intro",
        );
    }

    #[test]
    fn only_web_pages_have_a_link_to_copy() {
        for input in [
            "",
            "felinaut://newtab",
            "about:blank",
            "file:///Users/me/secret.txt",
            "javascript:alert(1)",
            "data:text/plain,hi",
            "not a url",
            "https://",
        ] {
            assert_eq!(clean(input), None, "{input:?}");
        }
    }
}
