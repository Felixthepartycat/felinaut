//! Pinned tabs: one `url \t title` line each in `Felifox/pinned.tsv` under Application Support.
//! A pinned tab reopens its pinned URL on launch, whatever it showed when the app quit.

use std::path::PathBuf;

const HEADER: &str = "# Felifox pinned tabs: url\ttitle";

/// `~/Library/Application Support/Felifox/pinned.tsv`, beside the `history` folder.
pub fn default_path() -> Option<PathBuf> {
    let history = crate::history::default_path()?;
    Some(history.parent()?.parent()?.join("pinned.tsv"))
}

/// Pinned `(url, title)` pairs; lines that are not HTTP(S) URLs are skipped.
pub fn parse(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let (url, title) = line.split_once('\t').unwrap_or((line, ""));
            let url = url.trim();
            (url.starts_with("https://") || url.starts_with("http://"))
                .then(|| (url.to_owned(), title.trim().to_owned()))
        })
        .collect()
}

pub fn serialize<'a>(tabs: impl IntoIterator<Item = (&'a str, &'a str)>) -> String {
    let mut out = String::from(HEADER);
    for (url, title) in tabs {
        let title = title.replace(['\t', '\n', '\r'], " ");
        out.push('\n');
        out.push_str(&format!("{url}\t{title}"));
    }
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_sanitizes() {
        let text = serialize([
            ("https://github.com/pulls", "Pull\trequests\n"),
            ("http://localhost:3000/", ""),
        ]);
        assert_eq!(
            parse(&text),
            vec![
                ("https://github.com/pulls".into(), "Pull requests".into()),
                ("http://localhost:3000/".into(), String::new()),
            ]
        );
    }

    #[test]
    fn skips_comments_and_non_web_urls() {
        let text = "# header\nfile:///etc/passwd\tx\njavascript:alert(1)\n\nhttps://a.b/\n";
        assert_eq!(parse(text), vec![("https://a.b/".into(), String::new())]);
    }

    #[test]
    fn lives_beside_history() {
        if let (Some(pinned), Some(history)) = (default_path(), crate::history::default_path()) {
            assert_eq!(
                pinned.parent(),
                history.parent().and_then(|dir| dir.parent())
            );
        }
    }
}
