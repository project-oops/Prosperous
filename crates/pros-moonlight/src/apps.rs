//! The apps a client sees, one per target.
//!
//! A Moonlight client picks what to stream from an app list. The bridge maps that onto the thing a
//! person actually chooses - **which console** - by offering one app per registered target
//! (`docs/VIDEO.md` part four): choosing the app in the client is choosing the target, and nothing
//! new is invented for selection.

/// One entry in the app list: a target, as the client sees it.
#[derive(Debug, Clone)]
pub struct App {
    /// The numeric id the client launches by. Stable within a run.
    pub id: u32,
    /// The name shown in the client.
    pub title: String,
}

/// The apps on offer.
#[derive(Debug, Clone, Default)]
pub struct Apps {
    /// The entries, in the order they are shown.
    apps: Vec<App>,
}

impl Apps {
    /// Build an app list from titles, numbering them from one.
    #[must_use]
    pub fn from_titles<I, S>(titles: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let apps = titles
            .into_iter()
            .enumerate()
            .map(|(index, title)| App {
                id: u32::try_from(index).unwrap_or(0) + 1,
                title: title.into(),
            })
            .collect();
        Self { apps }
    }

    /// The `applist` document a client reads to populate its grid.
    #[must_use]
    pub(crate) fn applist(&self) -> String {
        use std::fmt::Write as _;
        let mut xml = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n<root status_code=\"200\">",
        );
        for app in &self.apps {
            let _ = write!(
                xml,
                "<App><IsHdrSupported>0</IsHdrSupported><AppTitle>{}</AppTitle><ID>{}</ID></App>",
                escape(&app.title),
                app.id,
            );
        }
        xml.push_str("</root>");
        xml
    }
}

/// Escape the five XML characters, so a target named with an ampersand does not break the document.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::Apps;

    #[test]
    fn one_app_per_title_numbered_from_one() {
        let apps = Apps::from_titles(["ps5 in the lounge", "ps5 on the bench"]);
        let xml = apps.applist();
        assert!(xml.contains("<AppTitle>ps5 in the lounge</AppTitle>"));
        assert!(xml.contains("<AppTitle>ps5 on the bench</AppTitle>"));
        assert!(xml.contains("<ID>1</ID>"));
        assert!(xml.contains("<ID>2</ID>"));
    }

    #[test]
    fn a_title_with_an_ampersand_is_escaped() {
        let apps = Apps::from_titles(["tom & jerry"]);
        assert!(apps.applist().contains("tom &amp; jerry"));
    }
}
