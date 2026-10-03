//! Who may use the server. With no token (only on a loopback address), anyone on this machine: requests must name a
//! loopback host and come from a loopback page, so a web site cannot reach the server through the browser (no DNS
//! rebinding, no cross-site requests but a loopback page's: the UI in browser mode, on http://localhost:4200, asks
//! the server to download a link, and may read the answers: `loopback_caller`). With a token, every request carries it: `Authorization: Bearer <token>`, or the
//! cookie that visiting `/?token=<token>` once sets (SameSite=Strict, so other sites' requests do not carry it).

use std::net::{IpAddr, SocketAddr};

use axum::http::header::{AUTHORIZATION, COOKIE, HOST, ORIGIN};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri};

/// The cookie that holds the token.
pub const COOKIE_NAME: &str = "aimview_token";

/// What to do with a request.
#[derive(Debug, PartialEq)]
pub enum Verdict {
    Pass,
    /// The token came in the query: set the cookie and send the browser to `location` (the same page without it).
    SetCookie { location: String, cookie: String },
    Refuse { status: StatusCode, reason: &'static str },
}

pub struct Access {
    token: Option<String>,
}

/// Whether two byte strings are equal, in a time that does not depend on where they differ.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

/// A host name or address (with or without its port) that stays on this machine.
fn is_loopback_host(host: &str) -> bool {
    let host = host.trim().to_ascii_lowercase();
    // ::1, [::1]:8770, [::1], 127.0.0.1:8770, localhost
    let name = match host.strip_prefix('[') {
        _ if host.parse::<IpAddr>().is_ok() => host.as_str(),
        Some(rest) => rest.split(']').next().unwrap_or_default(),
        None => host.rsplit_once(':').map_or(host.as_str(), |(h, port)| {
            if port.bytes().all(|b| b.is_ascii_digit()) { h } else { host.as_str() }
        }),
    };
    name == "localhost"
        || name.ends_with(".localhost")
        || name.parse::<IpAddr>().is_ok_and(|ip| ip.to_canonical().is_loopback())
}

/// An Origin header's page is on this machine (http://localhost:4200, http://127.0.0.1:8770, ...).
fn is_loopback_origin(origin: &str) -> bool {
    origin
        .split_once("://")
        .is_some_and(|(scheme, host)| matches!(scheme, "http" | "https") && is_loopback_host(host))
}

/// The Origin of a page on this machine (http://localhost:4200, the UI in browser mode), which may read the API's
/// answers (Access-Control-Allow-Origin); None for any other page, and for a request that names none.
pub fn loopback_caller(headers: &HeaderMap) -> Option<HeaderValue> {
    headers.get(ORIGIN).filter(|o| o.to_str().is_ok_and(is_loopback_origin)).cloned()
}

/// Whether every address the server listens on stays on this machine.
pub fn all_loopback(addrs: &[SocketAddr]) -> bool {
    !addrs.is_empty() && addrs.iter().all(|a| a.ip().to_canonical().is_loopback())
}

/// The value of one header, when it is there and readable.
fn header(headers: &HeaderMap, name: impl axum::http::header::AsHeaderName) -> Option<&str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

/// The token in the query (`token=`), percent-decoded, and the query without it.
fn query_token(query: &str) -> (Option<String>, String) {
    let mut token = None;
    let mut rest = Vec::new();
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        match pair.split_once('=') {
            Some(("token", v)) => token = Some(percent_encoding::percent_decode_str(v).decode_utf8_lossy().into_owned()),
            _ => rest.push(pair),
        }
    }
    (token, rest.join("&"))
}

impl Access {
    /// The access a server on `addrs` gets: refused when other machines can reach it and it has no token.
    pub fn new(addrs: &[SocketAddr], token: Option<String>) -> Result<Access, String> {
        if token.is_none() && !all_loopback(addrs) {
            return Err("the server would be open to other machines: give it a token (--token, or token = \"...\" in \
                        the settings file), or listen on 127.0.0.1"
                .into());
        }
        Ok(Access { token })
    }

    pub fn has_token(&self) -> bool {
        self.token.is_some()
    }

    /// Whether a request may go on.
    pub fn check(&self, method: &Method, uri: &Uri, headers: &HeaderMap) -> Verdict {
        let host = header(headers, HOST).or_else(|| uri.authority().map(|a| a.as_str()));
        let origin = header(headers, ORIGIN);
        // a page of another site (a link, a form, a script) never gets in; a page on this machine may
        if header(headers, "sec-fetch-site") == Some("cross-site") && !origin.is_some_and(is_loopback_origin) {
            return Verdict::Refuse { status: StatusCode::FORBIDDEN, reason: "a request from another site" };
        }
        let Some(token) = &self.token else {
            if !host.is_some_and(is_loopback_host) {
                return Verdict::Refuse { status: StatusCode::FORBIDDEN, reason: "open the server as localhost or 127.0.0.1" };
            }
            if origin.is_some_and(|o| !is_loopback_origin(o)) {
                return Verdict::Refuse { status: StatusCode::FORBIDDEN, reason: "a request from another site's page" };
            }
            return Verdict::Pass;
        };
        let bearer = header(headers, AUTHORIZATION).and_then(|v| v.strip_prefix("Bearer ")).map(str::trim);
        if bearer.is_some_and(|b| same(b.as_bytes(), token.as_bytes())) {
            return Verdict::Pass;
        }
        let cookie = headers
            .get_all(COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(';'))
            .filter_map(|c| c.trim().split_once('='))
            .find(|(name, _)| *name == COOKIE_NAME)
            .map(|(_, value)| value);
        if cookie.is_some_and(|c| same(c.as_bytes(), token.as_bytes())) {
            // the browser sends the cookie by itself: only from the server's own pages
            let own = match (origin, host) {
                (Some(o), Some(h)) => o.split_once("://").is_some_and(|(_, oh)| oh.eq_ignore_ascii_case(h)),
                _ => true,
            };
            if !own {
                return Verdict::Refuse { status: StatusCode::FORBIDDEN, reason: "a request from another site" };
            }
            return Verdict::Pass;
        }
        if matches!(*method, Method::GET | Method::HEAD)
            && let (Some(given), rest) = query_token(uri.query().unwrap_or_default())
            && same(given.as_bytes(), token.as_bytes())
        {
            let path = uri.path();
            let location = if rest.is_empty() { path.to_string() } else { format!("{path}?{rest}") };
            let cookie = format!("{COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age=31536000");
            return Verdict::SetCookie { location, cookie };
        }
        Verdict::Refuse {
            status: StatusCode::UNAUTHORIZED,
            reason: "this server needs its token: open /?token=<token> once in this browser, or send the header \
                     Authorization: Bearer <token>",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "s3cret-token";

    fn loopback() -> Vec<SocketAddr> {
        vec!["127.0.0.1:8770".parse().unwrap()]
    }

    fn lan() -> Vec<SocketAddr> {
        vec!["0.0.0.0:8770".parse().unwrap()]
    }

    fn check(access: &Access, method: Method, uri: &str, headers: &[(&'static str, &str)]) -> Verdict {
        let mut map = HeaderMap::new();
        for (name, value) in headers {
            map.append(*name, value.parse().unwrap());
        }
        access.check(&method, &uri.parse().unwrap(), &map)
    }

    fn refused(v: &Verdict) -> Option<StatusCode> {
        match v {
            Verdict::Refuse { status, .. } => Some(*status),
            _ => None,
        }
    }

    #[test]
    fn another_machine_can_reach_the_server_only_with_a_token() {
        assert!(Access::new(&lan(), None).is_err());
        assert!(Access::new(&["192.168.1.20:8770".parse().unwrap()], None).is_err());
        assert!(Access::new(&lan(), Some(TOKEN.into())).is_ok());
        assert!(Access::new(&loopback(), None).is_ok());
        assert!(Access::new(&["[::1]:8770".parse().unwrap()], None).is_ok());
    }

    #[test]
    fn without_a_token_only_this_machine_gets_in() {
        let a = Access::new(&loopback(), None).unwrap();
        for host in ["127.0.0.1:8770", "localhost:4200", "[::1]:8770", "LOCALHOST", "app.localhost:8770"] {
            assert_eq!(check(&a, Method::GET, "/api/vods", &[("host", host)]), Verdict::Pass, "{host}");
        }
        // the UI's own pages, through the Angular dev server too
        let page = [("host", "127.0.0.1:8770"), ("origin", "http://localhost:4201")];
        assert_eq!(check(&a, Method::POST, "/api/analyse?id=x", &page), Verdict::Pass);
        // DNS rebinding: a name of another site that points at 127.0.0.1
        assert_eq!(refused(&check(&a, Method::GET, "/api/vods", &[("host", "evil.example:8770")])), Some(StatusCode::FORBIDDEN));
        assert!(refused(&check(&a, Method::GET, "/", &[])).is_some(), "no host");
        // another site's page posting to the server
        let cross = [("host", "127.0.0.1:8770"), ("origin", "https://evil.example")];
        assert_eq!(refused(&check(&a, Method::POST, "/api/analyse?id=x", &cross)), Some(StatusCode::FORBIDDEN));
        let cross = [("host", "127.0.0.1:8770"), ("sec-fetch-site", "cross-site")];
        assert_eq!(refused(&check(&a, Method::GET, "/video?id=x", &cross)), Some(StatusCode::FORBIDDEN));
        let null = [("host", "127.0.0.1:8770"), ("origin", "null")];
        assert!(refused(&check(&a, Method::POST, "/api/run?id=x", &null)).is_some());
        // the UI in browser mode, on another port of this machine: a cross-site request, from a loopback page
        let browser = [("host", "127.0.0.1:8770"), ("origin", "http://localhost:4200"), ("sec-fetch-site", "cross-site")];
        assert_eq!(check(&a, Method::POST, "/api/link", &browser), Verdict::Pass);
        assert_eq!(check(&a, Method::OPTIONS, "/api/link", &browser), Verdict::Pass);
        let cross = [("host", "127.0.0.1:8770"), ("origin", "https://evil.example"), ("sec-fetch-site", "cross-site")];
        assert_eq!(refused(&check(&a, Method::POST, "/api/link", &cross)), Some(StatusCode::FORBIDDEN));
    }

    #[test]
    fn only_loopback_pages_read_the_answers() {
        let caller = |origin: &str| {
            let mut map = HeaderMap::new();
            map.insert(ORIGIN, origin.parse().unwrap());
            loopback_caller(&map)
        };
        assert_eq!(caller("http://localhost:4200").unwrap(), "http://localhost:4200");
        assert_eq!(caller("http://127.0.0.1:4200").unwrap(), "http://127.0.0.1:4200");
        for origin in ["https://evil.example", "null", "http://localhost.evil.example"] {
            assert!(caller(origin).is_none(), "{origin}");
        }
        assert!(loopback_caller(&HeaderMap::new()).is_none());
    }

    #[test]
    fn the_token_in_the_header() {
        let a = Access::new(&lan(), Some(TOKEN.into())).unwrap();
        let host = ("host", "192.168.1.20:8770");
        let ok = format!("Bearer {TOKEN}");
        assert_eq!(check(&a, Method::POST, "/api/analyse?id=x", &[host, ("authorization", &ok)]), Verdict::Pass);
        let wrong = format!("Bearer {TOKEN}x");
        let v = check(&a, Method::GET, "/api/vods", &[host, ("authorization", &wrong)]);
        assert_eq!(refused(&v), Some(StatusCode::UNAUTHORIZED));
        assert_eq!(refused(&check(&a, Method::GET, "/api/vods", &[host])), Some(StatusCode::UNAUTHORIZED));
        assert_eq!(refused(&check(&a, Method::GET, "/", &[host, ("authorization", TOKEN)])), Some(StatusCode::UNAUTHORIZED));
    }

    #[test]
    fn the_token_in_the_query_sets_the_cookie_and_the_cookie_lets_the_browser_in() {
        let a = Access::new(&lan(), Some(TOKEN.into())).unwrap();
        let host = ("host", "192.168.1.20:8770");
        let v = check(&a, Method::GET, &format!("/?token={TOKEN}"), &[host]);
        let Verdict::SetCookie { location, cookie } = v else { panic!("no cookie: {v:?}") };
        assert_eq!(location, "/");
        assert!(cookie.starts_with(&format!("{COOKIE_NAME}={TOKEN};")));
        assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Strict"));
        // the rest of the query stays
        let v = check(&a, Method::GET, &format!("/run?a=1&token={TOKEN}&b=2"), &[host]);
        assert!(matches!(v, Verdict::SetCookie { location, .. } if location == "/run?a=1&b=2"));
        // a wrong token sets nothing
        let v = check(&a, Method::GET, "/?token=guess", &[host]);
        assert_eq!(refused(&v), Some(StatusCode::UNAUTHORIZED));

        let jar = format!("theme=dark; {COOKIE_NAME}={TOKEN}");
        assert_eq!(check(&a, Method::GET, "/api/vods", &[host, ("cookie", &jar)]), Verdict::Pass);
        let page = [host, ("cookie", &jar), ("origin", "http://192.168.1.20:8770")];
        assert_eq!(check(&a, Method::POST, "/api/analyse?id=x", &page), Verdict::Pass);
        // the cookie on a request another site's page made
        let other = [host, ("cookie", &jar), ("origin", "http://192.168.1.30:8000")];
        assert_eq!(refused(&check(&a, Method::POST, "/api/analyse?id=x", &other)), Some(StatusCode::FORBIDDEN));
        let stale = format!("{COOKIE_NAME}=old-token");
        assert_eq!(refused(&check(&a, Method::GET, "/api/vods", &[host, ("cookie", &stale)])), Some(StatusCode::UNAUTHORIZED));
    }

    #[test]
    fn loopback_names() {
        for h in ["localhost", "localhost:1", "127.0.0.1", "127.8.9.10:80", "[::1]", "[::1]:8770", "::1", "[::ffff:127.0.0.1]:1"] {
            assert!(is_loopback_host(h), "{h}");
        }
        for h in ["", "example.com", "localhost.example.com", "192.168.1.2:8770", "[::2]:1", "0.0.0.0", "localhost:abc"] {
            assert!(!is_loopback_host(h), "{h}");
        }
    }
}
