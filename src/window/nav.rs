//! 014 (research R-4, contracts/window.md §3), 016 (research M-5, FR-034, contracts/window-macos.md
//! §3): what the window does with a navigation or a request for a new window. The Studio stays in
//! the window; only an `http(s)` address elsewhere, on the engineer's click, goes to the default
//! browser; anything else is refused. The Studio's origin is the platform's (`ORIGIN`); the
//! decision takes it as a parameter so both platforms' rules are tested on every OS.

/// The window's answer to a navigation or a new-window request.
#[derive(Debug, PartialEq, Eq)]
pub enum Nav {
    /// Load it in the window: the Studio's own address.
    Load,
    /// Cancel it in the window and open this address in the default browser.
    Browser(String),
    /// Cancel it; nothing opens.
    Refuse,
}

/// The Studio's origin on Windows: the custom scheme `fallow` as `wry` serves it to WebView2.
pub const WINDOWS_ORIGIN: &str = "http://fallow.localhost";
/// The Studio's origin on macOS: the custom scheme `fallow` as WebKit serves it (research M-4).
pub const MACOS_ORIGIN: &str = "fallow://localhost";

/// The Studio's origin on this platform.
#[cfg(target_os = "macos")]
pub const ORIGIN: &str = MACOS_ORIGIN;
/// The Studio's origin on this platform.
#[cfg(not(target_os = "macos"))]
pub const ORIGIN: &str = WINDOWS_ORIGIN;

/// The Studio's first address in the window.
#[cfg(target_os = "macos")]
pub const HOME: &str = "fallow://localhost/";
/// The Studio's first address in the window.
#[cfg(not(target_os = "macos"))]
pub const HOME: &str = "http://fallow.localhost/";

/// A URL's scheme (lowercased), host (lowercased, without user information or port) and whether
/// the address carried user information or a port. `None` without `scheme://`.
fn parts(url: &str) -> Option<(String, String, bool)> {
    let (scheme, rest) = url.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    let authority = rest.split(['/', '?', '#', '\\']).next().unwrap_or_default();
    let (user, host_port) = match authority.rsplit_once('@') {
        Some((_, h)) => (true, h),
        None => (false, authority),
    };
    let (host, port) = if host_port.starts_with('[') {
        // An IPv6 address: its port, if any, follows the bracket.
        match host_port.find(']') {
            Some(i) => (&host_port[..=i], host_port.len() > i + 1),
            None => (host_port, false),
        }
    } else {
        match host_port.split_once(':') {
            Some((h, _)) => (h, true),
            None => (host_port, false),
        }
    };
    Some((scheme, host.to_ascii_lowercase(), user || port))
}

/// Nothing with a control character, a space or a quote goes anywhere.
fn odd(url: &str) -> bool {
    url.chars().any(|c| c.is_control() || c == ' ' || c == '"')
}

/// An address the default browser may be given: `http(s)`, a host, and not the Studio's own
/// Windows host (`fallow.localhost` in a browser is the machine's port 80, which is not Fallow).
fn for_browser(url: &str) -> bool {
    if odd(url) {
        return false;
    }
    match parts(url) {
        Some((scheme, host, _)) => {
            (scheme == "http" || scheme == "https")
                && !host.is_empty()
                && host != "fallow.localhost"
        }
        None => false,
    }
}

/// The decision of 014 research R-4 for `url` in a window whose Studio is at `origin`
/// (`ORIGIN`); `new_window` for a request to open a window, `user_initiated` when the engineer's
/// click (or key) started it, as far as the web view can tell (WebView2 can; WebKit through `wry`
/// cannot, so on macOS it is always false and links leave through `open_link`).
pub fn decide(origin: &str, url: &str, new_window: bool, user_initiated: bool) -> Nav {
    if odd(url) {
        return Nav::Refuse;
    }
    if !new_window && url == "about:blank" {
        return Nav::Load;
    }
    let Some((scheme, host, port_or_user)) = parts(url) else {
        return Nav::Refuse;
    };
    let ours = parts(origin).is_some_and(|(s, h, _)| s == scheme && h == host) && !port_or_user;
    if ours {
        return if new_window { Nav::Refuse } else { Nav::Load };
    }
    if !user_initiated || !for_browser(url) {
        return Nav::Refuse;
    }
    Nav::Browser(url.to_string())
}

/// 016 (FR-034, contracts/window-macos.md §4): the page's `open:<url>` message, sent only for
/// the engineer's click on a link outside the Studio: the address to open in the default browser
/// (`http`/`https` only), or `None`.
pub fn open_link(url: &str) -> Option<String> {
    for_browser(url).then(|| url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: &str = WINDOWS_ORIGIN;
    const M: &str = MACOS_ORIGIN;

    #[test]
    fn the_studio_loads_in_the_window_and_never_as_a_new_one() {
        assert_eq!(
            decide(W, "http://fallow.localhost/", false, false),
            Nav::Load
        );
        assert_eq!(
            decide(W, "http://fallow.localhost/#/loops/3", false, true),
            Nav::Load
        );
        assert_eq!(
            decide(W, "HTTP://Fallow.LocalHost/app.js", false, false),
            Nav::Load
        );
        assert_eq!(decide(W, "about:blank", false, false), Nav::Load);
        // As a new window: never a window, never the browser (port 80 there is not Fallow).
        assert_eq!(
            decide(W, "http://fallow.localhost/api/x", true, true),
            Nav::Refuse
        );
        assert_eq!(decide(W, "about:blank", true, true), Nav::Refuse);
    }

    #[test]
    fn an_address_elsewhere_goes_to_the_browser_only_on_a_click() {
        let pr = "https://github.com/o/r/pull/7";
        assert_eq!(decide(W, pr, false, true), Nav::Browser(pr.into()));
        assert_eq!(decide(W, pr, true, true), Nav::Browser(pr.into()));
        assert_eq!(decide(W, pr, false, false), Nav::Refuse);
        assert_eq!(decide(W, pr, true, false), Nav::Refuse);
        assert_eq!(
            decide(W, "http://example.com/x", false, true),
            Nav::Browser("http://example.com/x".into())
        );
    }

    #[test]
    fn look_alikes_of_the_studio_are_not_it() {
        for url in [
            "http://fallow.localhost.evil.example/",
            "http://fallow.localhost@evil.example/",
            "http://evil.example@fallow.localhost/",
            "http://fallow.localhost:8080/",
            "https://fallow.localhost/",
        ] {
            assert_ne!(decide(W, url, false, true), Nav::Load, "{url}");
            if let Nav::Browser(u) = decide(W, url, false, true) {
                assert!(
                    !u.contains("fallow.localhost/") || u.contains("evil"),
                    "{url}"
                );
            }
        }
        // Any form of the Studio's host in a browser is refused.
        assert_eq!(
            decide(W, "https://fallow.localhost/", false, true),
            Nav::Refuse
        );
        assert_eq!(
            decide(W, "http://fallow.localhost:8080/", false, true),
            Nav::Refuse
        );
    }

    #[test]
    fn every_other_scheme_is_refused() {
        for url in [
            "file:///C:/Windows/System32/calc.exe",
            "file://server/share/x.exe",
            "\\\\server\\share\\x.exe",
            "ms-settings:privacy",
            "search-ms:query=x",
            "javascript:alert(1)",
            "data:text/html,hi",
            "about:settings",
            "mailto:a@b.c",
            "ftp://example.com/",
            "",
        ] {
            assert_eq!(decide(W, url, false, true), Nav::Refuse, "{url}");
            assert_eq!(decide(W, url, true, true), Nav::Refuse, "{url}");
        }
    }

    #[test]
    fn odd_characters_go_nowhere() {
        assert_eq!(
            decide(W, "https://example.com/a b", false, true),
            Nav::Refuse
        );
        assert_eq!(
            decide(W, "https://example.com/\"x", false, true),
            Nav::Refuse
        );
        assert_eq!(
            decide(W, "https://example.com/\nx", false, true),
            Nav::Refuse
        );
    }

    #[test]
    fn on_macos_the_studio_is_fallow_localhost_and_nothing_else_loads() {
        // §3 with macOS's origin: within the Studio loads, never as a new window.
        assert_eq!(decide(M, "fallow://localhost/", false, false), Nav::Load);
        assert_eq!(
            decide(M, "fallow://localhost/#/loops/3", false, false),
            Nav::Load
        );
        assert_eq!(
            decide(M, "FALLOW://LocalHost/app.js", false, false),
            Nav::Load
        );
        assert_eq!(decide(M, "fallow://localhost/x", true, false), Nav::Refuse);
        assert_eq!(decide(M, "about:blank", false, false), Nav::Load);
        assert_eq!(decide(M, "about:blank", true, false), Nav::Refuse);
        // Windows' Studio address is not the Studio on macOS.
        assert_eq!(
            decide(M, "http://fallow.localhost/", false, false),
            Nav::Refuse
        );
        for url in [
            "fallow://localhost:80/",
            "fallow://user@localhost/",
            "fallow://localhost.evil.example/",
            "fallow://other/",
        ] {
            assert_eq!(decide(M, url, false, false), Nav::Refuse, "{url}");
        }
        // And macOS's is not the Studio on Windows.
        assert_eq!(decide(W, "fallow://localhost/", false, true), Nav::Refuse);
    }

    #[test]
    fn on_macos_an_address_elsewhere_is_refused_in_the_window() {
        // WebKit through `wry` never says a navigation was the engineer's: refused, nothing
        // opened; links leave through the page's `open:` message (§4).
        let pr = "https://github.com/o/r/pull/7";
        assert_eq!(decide(M, pr, false, false), Nav::Refuse);
        assert_eq!(decide(M, pr, true, false), Nav::Refuse);
        for url in [
            "file:///etc/passwd",
            "x-apple.systempreferences:com.apple.preference.security",
            "javascript:alert(1)",
            "data:text/html,hi",
            "mailto:a@b.c",
            "",
        ] {
            assert_eq!(decide(M, url, false, false), Nav::Refuse, "{url}");
            assert_eq!(decide(M, url, true, true), Nav::Refuse, "{url}");
        }
    }

    #[test]
    fn the_pages_open_message_opens_only_http_addresses_elsewhere() {
        let pr = "https://github.com/o/r/pull/7";
        assert_eq!(open_link(pr), Some(pr.to_string()));
        assert_eq!(
            open_link("http://example.com/x"),
            Some("http://example.com/x".to_string())
        );
        assert_eq!(
            open_link("http://localhost:3000/"),
            Some("http://localhost:3000/".to_string())
        );
        for url in [
            "fallow://localhost/",
            "http://fallow.localhost/",
            "https://fallow.localhost:8080/",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "mailto:a@b.c",
            "ftp://example.com/",
            "https:///nohost",
            "https://example.com/a b",
            "https://example.com/\nx",
            "",
        ] {
            assert_eq!(open_link(url), None, "{url}");
        }
    }
}
