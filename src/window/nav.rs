//! 014 (research R-4, contracts/window.md §3): what the window does with a navigation or a
//! request for a new window. The Studio stays in the window; only an `http(s)` address elsewhere,
//! on the engineer's click, goes to the default browser; anything else is refused.

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

/// The Studio's address in the window (the custom scheme `fallow`, as `wry` serves it on
/// Windows).
pub const HOME: &str = "http://fallow.localhost/";

/// The scheme and host of `url` when its scheme is `http` or `https`: the host lowercased,
/// without user information or port, and whether the address carried either.
fn http_host(url: &str) -> Option<(String, String, bool)> {
    let (scheme, rest) = url.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return None;
    }
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

/// The decision of research R-4 for `url`; `new_window` for a request to open a window,
/// `user_initiated` when the engineer's click (or key) started it.
pub fn decide(url: &str, new_window: bool, user_initiated: bool) -> Nav {
    // Nothing with a control character, a space or a quote goes anywhere.
    if url.chars().any(|c| c.is_control() || c == ' ' || c == '"') {
        return Nav::Refuse;
    }
    if !new_window && url == "about:blank" {
        return Nav::Load;
    }
    let Some((scheme, host, port_or_user)) = http_host(url) else {
        return Nav::Refuse;
    };
    let ours = scheme == "http" && host == "fallow.localhost" && !port_or_user;
    if ours {
        return if new_window { Nav::Refuse } else { Nav::Load };
    }
    // `fallow.localhost` in a browser is the machine's own port 80, which is not Fallow.
    if host == "fallow.localhost" || host.is_empty() || !user_initiated {
        return Nav::Refuse;
    }
    Nav::Browser(url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_studio_loads_in_the_window_and_never_as_a_new_one() {
        assert_eq!(decide(HOME, false, false), Nav::Load);
        assert_eq!(
            decide("http://fallow.localhost/#/loops/3", false, true),
            Nav::Load
        );
        assert_eq!(
            decide("HTTP://Fallow.LocalHost/app.js", false, false),
            Nav::Load
        );
        assert_eq!(decide("about:blank", false, false), Nav::Load);
        // As a new window: never a window, never the browser (port 80 there is not Fallow).
        assert_eq!(
            decide("http://fallow.localhost/api/x", true, true),
            Nav::Refuse
        );
        assert_eq!(decide("about:blank", true, true), Nav::Refuse);
    }

    #[test]
    fn an_address_elsewhere_goes_to_the_browser_only_on_a_click() {
        let pr = "https://github.com/o/r/pull/7";
        assert_eq!(decide(pr, false, true), Nav::Browser(pr.into()));
        assert_eq!(decide(pr, true, true), Nav::Browser(pr.into()));
        assert_eq!(decide(pr, false, false), Nav::Refuse);
        assert_eq!(decide(pr, true, false), Nav::Refuse);
        assert_eq!(
            decide("http://example.com/x", false, true),
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
            assert_ne!(decide(url, false, true), Nav::Load, "{url}");
            if let Nav::Browser(u) = decide(url, false, true) {
                assert!(
                    !u.contains("fallow.localhost/") || u.contains("evil"),
                    "{url}"
                );
            }
        }
        // Any form of the Studio's host in a browser is refused.
        assert_eq!(
            decide("https://fallow.localhost/", false, true),
            Nav::Refuse
        );
        assert_eq!(
            decide("http://fallow.localhost:8080/", false, true),
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
            assert_eq!(decide(url, false, true), Nav::Refuse, "{url}");
            assert_eq!(decide(url, true, true), Nav::Refuse, "{url}");
        }
    }

    #[test]
    fn odd_characters_go_nowhere() {
        assert_eq!(decide("https://example.com/a b", false, true), Nav::Refuse);
        assert_eq!(decide("https://example.com/\"x", false, true), Nav::Refuse);
        assert_eq!(decide("https://example.com/\nx", false, true), Nav::Refuse);
    }
}
