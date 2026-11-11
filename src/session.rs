//! Open tabs, kept across launches in `Felifox/tabs.tsv` under Application Support: one
//! `flags \t url \t title` line per tab, in sidebar order. Flags: `p` pinned, `s` shown in
//! split view with the line before, `a` the active tab. Blank New Tabs are not kept. Pinned
//! tabs store their pinned URL, the others the page they were showing.

use std::path::PathBuf;

const HEADER: &str =
    "# Felifox tabs: flags (p pinned, s split with previous, a active)\turl\ttitle";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SavedTab {
    pub url: String,
    pub title: String,
    pub pinned: bool,
    pub split: bool,
    pub active: bool,
}

/// `~/Library/Application Support/Felifox/tabs.tsv`, beside the `history` folder.
pub fn default_path() -> Option<PathBuf> {
    let history = crate::history::default_path()?;
    Some(history.parent()?.parent()?.join("tabs.tsv"))
}

/// Saved tabs; lines whose URL is not HTTP(S) are skipped.
pub fn parse(text: &str) -> Vec<SavedTab> {
    text.lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let mut fields = line.splitn(3, '\t');
            let flags = fields.next()?;
            let url = fields.next()?.trim();
            let title = fields.next().unwrap_or("").trim();
            (url.starts_with("https://") || url.starts_with("http://")).then(|| SavedTab {
                url: url.to_owned(),
                title: title.to_owned(),
                pinned: flags.contains('p'),
                split: flags.contains('s'),
                active: flags.contains('a'),
            })
        })
        .collect()
}

pub fn serialize(tabs: &[SavedTab]) -> String {
    let mut out = String::from(HEADER);
    for tab in tabs {
        let flags: String = [(tab.pinned, 'p'), (tab.split, 's'), (tab.active, 'a')]
            .into_iter()
            .filter_map(|(set, flag)| set.then_some(flag))
            .collect();
        let title = tab.title.replace(['\t', '\n', '\r'], " ");
        out.push('\n');
        out.push_str(&format!("{flags}\t{}\t{title}", tab.url));
    }
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(url: &str, title: &str, pinned: bool, split: bool, active: bool) -> SavedTab {
        SavedTab {
            url: url.into(),
            title: title.into(),
            pinned,
            split,
            active,
        }
    }

    #[test]
    fn round_trips_and_sanitizes() {
        let tabs = vec![
            tab(
                "https://github.com/pulls",
                "Pull\trequests\n",
                true,
                false,
                false,
            ),
            tab("https://a.b/", "A", false, false, false),
            tab("http://localhost:3000/", "", false, true, true),
        ];
        let parsed = parse(&serialize(&tabs));
        assert_eq!(parsed[0].title, "Pull requests");
        assert_eq!(parsed[1..], tabs[1..]);
        assert!(parsed[0].pinned && !parsed[0].split && !parsed[0].active);
    }

    #[test]
    fn skips_comments_and_non_web_urls() {
        let text = "# header\np\tfile:///etc/passwd\tx\n\tjavascript:alert(1)\n\n\thttps://a.b/\n";
        assert_eq!(
            parse(text),
            vec![tab("https://a.b/", "", false, false, false)]
        );
    }

    #[test]
    fn lives_beside_history() {
        if let (Some(tabs), Some(history)) = (default_path(), crate::history::default_path()) {
            assert_eq!(tabs.parent(), history.parent().and_then(|dir| dir.parent()));
        }
    }
}
