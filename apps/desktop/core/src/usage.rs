//! App and URL usage (E09-S12): what the member had in front of them while the timer ran,
//! reduced to what the organization's policy allows before anything is kept.
//!
//! **Levels** (`apps.tracking_level`, from the agent's configuration, E09-S06):
//!
//! - `off`: nothing.
//! - `apps`: the foreground application.
//! - `domains`: also the website in a browser, as its registrable domain (eTLD+1, from the
//!   Public Suffix List): `mail.example.co.uk` is kept as `example.co.uk`.
//! - `paths`: also the URL's path.
//!
//! **Never kept**, at any level: query strings, fragments, user names and passwords in URLs,
//! addresses that aren't web pages (`file:`, `chrome:`, `about:` and the like), and the title of
//! a private window. Window titles are kept only when the organization turns them on.
//!
//! **Private windows** keep their domain only, or nothing, by policy (the acceptance criterion).
//!
//! The sensors are asked only for what the policy keeps: no title unless titles are on, and no
//! browser tab below `domains` unless a title needs checking (a private window's title is never
//! kept, and a tab that cannot be read counts as private).

use serde::{Deserialize, Serialize};
use url::{Host, Url};
use worklume_platform::{ActiveWindow, BrowserTab, BrowserUrl, ForegroundApp};

/// Longest window title kept, in characters.
pub const MAX_TITLE_CHARS: usize = 256;
/// Longest URL path kept, in characters.
pub const MAX_PATH_CHARS: usize = 512;
/// Longest app identifier, app name, domain or browser name kept, in characters.
pub const MAX_NAME_CHARS: usize = 256;

/// How much app and URL detail the organization keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum UsageLevel {
    Off,
    Apps,
    Domains,
    Paths,
}

/// What a private browser window keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateWindows {
    /// The domain, never the path.
    DomainOnly,
    /// No website at all. The browser still counts as the app in front.
    Nothing,
}

/// The organization's app and URL policy for the member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsagePolicy {
    pub level: UsageLevel,
    /// Keep window titles (off by default, ADR-014).
    pub window_titles: bool,
    pub private_windows: PrivateWindows,
}

impl UsagePolicy {
    /// Keeps nothing: the policy until the agent's configuration says otherwise.
    pub const OFF: Self = Self {
        level: UsageLevel::Off,
        window_titles: false,
        private_windows: PrivateWindows::Nothing,
    };
}

/// An application in use, as far as the policy allows.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AppUse {
    pub identifier: String,
    pub name: String,
    pub title: Option<String>,
}

/// A website in use, as far as the policy allows.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SiteUse {
    pub domain: String,
    pub path: Option<String>,
    pub browser: String,
    pub private_window: bool,
}

/// What was in front during one tick.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsageSample {
    pub app: Option<AppUse>,
    pub site: Option<SiteUse>,
}

impl UsageSample {
    pub const NONE: Self = Self {
        app: None,
        site: None,
    };
}

impl UsagePolicy {
    /// Reads the foreground app and, from `domains` up, its browser tab, then reduces both to
    /// what the policy keeps. A sensor that cannot answer gives nothing for this tick.
    #[must_use]
    pub fn observe(self, window: &dyn ActiveWindow, browser: &dyn BrowserUrl) -> UsageSample {
        if self.level == UsageLevel::Off {
            return UsageSample::NONE;
        }
        let Ok(Some(mut app)) = window.foreground(self.window_titles) else {
            return UsageSample::NONE;
        };
        // The tab is needed for its website, and also to know whether a title belongs to a
        // private window. A tab that cannot be read might be private, so its title goes.
        let tab = if self.level >= UsageLevel::Domains || app.title.is_some() {
            browser.active_tab(&app).unwrap_or_else(|_| {
                app.title = None;
                None
            })
        } else {
            None
        };
        self.reduce(app, tab.as_ref())
    }

    /// Keeps of `app` and `tab` what the policy allows.
    #[must_use]
    pub fn reduce(self, app: ForegroundApp, tab: Option<&BrowserTab>) -> UsageSample {
        if self.level == UsageLevel::Off {
            return UsageSample::NONE;
        }
        let private = tab.is_some_and(|tab| tab.private);
        let title = app
            .title
            .filter(|_| self.window_titles && !private)
            .map(|title| truncate(title.trim(), MAX_TITLE_CHARS))
            .filter(|title| !title.is_empty());
        let site = tab.and_then(|tab| self.site(tab));
        UsageSample {
            app: Some(AppUse {
                identifier: truncate(&app.identifier, MAX_NAME_CHARS),
                name: truncate(&app.name, MAX_NAME_CHARS),
                title,
            }),
            site,
        }
    }

    fn site(self, tab: &BrowserTab) -> Option<SiteUse> {
        if self.level < UsageLevel::Domains
            || (tab.private && self.private_windows == PrivateWindows::Nothing)
        {
            return None;
        }
        let with_path = self.level == UsageLevel::Paths && !tab.private;
        let (domain, path) = reduce_url(&tab.url, with_path)?;
        Some(SiteUse {
            domain,
            path,
            browser: truncate(&tab.browser.to_lowercase(), MAX_NAME_CHARS),
            private_window: tab.private,
        })
    }
}

/// The website a URL belongs to: its registrable domain and, when asked for, its path. Only
/// `http` and `https` addresses count. Queries, fragments and credentials are never returned.
/// A host with no public suffix (`localhost`, an intranet name) or an IP address is kept whole.
#[must_use]
pub fn reduce_url(url: &str, with_path: bool) -> Option<(String, Option<String>)> {
    let parsed = Url::parse(url.trim()).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let domain = match parsed.host()? {
        Host::Domain(host) => {
            let host = host.trim_end_matches('.');
            if host.is_empty() {
                return None;
            }
            psl::domain_str(host).unwrap_or(host).to_owned()
        }
        Host::Ipv4(address) => address.to_string(),
        Host::Ipv6(address) => address.to_string(),
    };
    let path = with_path.then(|| truncate(parsed.path(), MAX_PATH_CHARS));
    Some((truncate(&domain, MAX_NAME_CHARS), path))
}

fn truncate(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::reduce_url;

    fn domain(url: &str) -> Option<String> {
        reduce_url(url, false).map(|(domain, _)| domain)
    }

    #[test]
    fn keeps_the_registrable_domain() {
        for (url, expected) in [
            ("https://mail.example.co.uk/inbox", "example.co.uk"),
            ("https://docs.github.com/en", "github.com"),
            ("http://EXAMPLE.com.", "example.com"),
            ("https://user:secret@www.example.org:8443/a", "example.org"),
            ("https://bücher.de/katalog", "xn--bcher-kva.de"),
            ("https://foo.github.io/page", "foo.github.io"),
            ("http://localhost:3000/", "localhost"),
            ("http://192.168.1.10/admin", "192.168.1.10"),
            ("http://[::1]:8080/", "::1"),
        ] {
            assert_eq!(domain(url).as_deref(), Some(expected), "{url}");
        }
    }

    #[test]
    fn ignores_what_is_not_a_web_page() {
        for url in [
            "file:///Users/someone/salary.xlsx",
            "chrome://settings",
            "about:blank",
            "mailto:someone@example.com",
            "not a url",
            "",
        ] {
            assert_eq!(domain(url), None, "{url}");
        }
    }

    #[test]
    fn paths_lose_queries_fragments_and_length() {
        let (_, path) =
            reduce_url("https://example.com/search/results?q=salary#top", true).unwrap();
        assert_eq!(path.as_deref(), Some("/search/results"));
        let long = format!("https://example.com/{}", "a".repeat(2_000));
        let (_, path) = reduce_url(&long, true).unwrap();
        assert_eq!(path.unwrap().chars().count(), super::MAX_PATH_CHARS);
        assert_eq!(
            reduce_url("https://example.com/a?b", false).unwrap().1,
            None
        );
    }
}
