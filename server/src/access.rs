//! Who may use the server. With no token (only on a loopback address), anyone on this machine: requests must name a
//! loopback host and come from a loopback page, so a web site cannot reach the server through the browser (no DNS
//! rebinding, no cross-site requests but a loopback page's: the UI in browser mode, on http://localhost:4200, asks
//! the server to download a link, and may read the answers: `loopback_caller`). With a token, every request carries
//! it: `Authorization: Bearer <token>`, or the cookie that visiting `/?token=<token>` once sets (SameSite=Strict, so
//! other sites' requests do not carry it). In dev mode no token is used: on a network address, any device on the
//! local network gets in, by an address or a machine's name (still no rebinding, and no other site's page).
//!
//! In: the addresses the server listens on and its token (main.rs), then each request's method, URI and headers
//! (http.rs). Out: a `Verdict` per request, which http.rs acts on.

use std::net::{IpAddr, SocketAddr};

use axum::http::header::{AUTHORIZATION, COOKIE, HOST, ORIGIN};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri};

/// The cookie that holds the token.
pub const COOKIE_NAME: &str = "aimview_token";
/// How long the browser keeps the cookie, seconds: a year.
const COOKIE_MAX_AGE_S: u32 = 31_536_000;

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
    /// Dev mode on an address other machines reach: they get in without a token.
    open_network: bool,
}

/// Whether two byte strings are equal, in a time that does not depend on where they differ.
fn equal_in_constant_time(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |difference, (x, y)| difference | (x ^ y)) == 0
}

/// A Host header's name or address without its port or brackets, lower case: ::1, [::1]:8770, [::1], 127.0.0.1:8770
/// and localhost:8770 give ::1, ::1, ::1, 127.0.0.1 and localhost.
fn host_name(host: &str) -> String {
    let host = host.trim().to_ascii_lowercase();
    match host.strip_prefix('[') {
        _ if host.parse::<IpAddr>().is_ok() => host,
        Some(rest) => rest.split(']').next().unwrap_or_default().to_string(),
        None => host.rsplit_once(':').map_or(host.clone(), |(name, port)| {
            if port.bytes().all(|b| b.is_ascii_digit()) { name.to_string() } else { host.clone() }
        }),
    }
}

/// A host name or address (with or without its port) that stays on this machine.
fn is_loopback_host(host: &str) -> bool {
    let name = host_name(host);
    name == "localhost"
        || name.ends_with(".localhost")
        || name.parse::<IpAddr>().is_ok_and(|ip| ip.to_canonical().is_loopback())
}

/// A host a device on the local network opens the server by: this machine's, an address (192.168.1.111:8770), a
/// machine's name (my-pc) or its name on the local network (my-pc.local). A web site's name (evil.example) is none,
/// so a site whose name points at this machine (DNS rebinding) is still refused.
fn is_local_network_host(host: &str) -> bool {
    let name = host_name(host);
    is_loopback_host(host)
        || name.parse::<IpAddr>().is_ok()
        || (!name.is_empty() && (!name.contains('.') || name.ends_with(".local")))
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
    headers.get(ORIGIN).filter(|origin| origin.to_str().is_ok_and(is_loopback_origin)).cloned()
}

/// Whether every address the server listens on stays on this machine.
pub fn all_loopback(addrs: &[SocketAddr]) -> bool {
    !addrs.is_empty() && addrs.iter().all(|address| address.ip().to_canonical().is_loopback())
}

/// The value of one header, when it is there and readable.
fn header(headers: &HeaderMap, name: impl axum::http::header::AsHeaderName) -> Option<&str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

/// The token in the query (`token=`), percent-decoded, and the query without it.
fn query_token(query: &str) -> (Option<String>, String) {
    let mut token = None;
    let mut rest = Vec::new();
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        match pair.split_once('=') {
            Some(("token", value)) => {
                token = Some(percent_encoding::percent_decode_str(value).decode_utf8_lossy().into_owned());
            }
            _ => rest.push(pair),
        }
    }
    (token, rest.join("&"))
}

/// The token in an `Authorization: Bearer <token>` header.
fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    header(headers, AUTHORIZATION).and_then(|value| value.strip_prefix("Bearer ")).map(str::trim)
}

/// The token in the cookie named COOKIE_NAME, from any Cookie header.
fn cookie_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|cookie| cookie.trim().split_once('='))
        .find(|(name, _)| *name == COOKIE_NAME)
        .map(|(_, value)| value)
}

/// Without a token: a request that names a loopback host, from no page or a page on this machine.
fn check_this_machine(host: Option<&str>, origin: Option<&str>) -> Verdict {
    if !host.is_some_and(is_loopback_host) {
        return Verdict::Refuse { status: StatusCode::FORBIDDEN, reason: "open the server as localhost or 127.0.0.1" };
    }
    if origin.is_some_and(|origin| !is_loopback_origin(origin)) {
        return Verdict::Refuse { status: StatusCode::FORBIDDEN, reason: "a request from another site's page" };
    }
    Verdict::Pass
}

/// Dev mode, open to the local network without a token: a request that names a local network host, from no page,
/// a page on this machine or the server's own page.
fn check_local_network(host: Option<&str>, origin: Option<&str>) -> Verdict {
    if !host.is_some_and(is_local_network_host) {
        return Verdict::Refuse {
            status: StatusCode::FORBIDDEN,
            reason: "open the server by this machine's address or name on the local network",
        };
    }
    if origin.is_some_and(is_loopback_origin) {
        return Verdict::Pass;
    }
    check_own_page(host, origin)
}

/// With the token's cookie: the browser sends the cookie by itself, so only the server's own pages (or no page) pass.
fn check_own_page(host: Option<&str>, origin: Option<&str>) -> Verdict {
    let own = match (origin, host) {
        (Some(origin), Some(host)) => {
            origin.split_once("://").is_some_and(|(_, origin_host)| origin_host.eq_ignore_ascii_case(host))
        }
        _ => true,
    };
    if !own {
        return Verdict::Refuse { status: StatusCode::FORBIDDEN, reason: "a request from another site" };
    }
    Verdict::Pass
}

/// The answer to the token in the query: the cookie, and the same page (`path`) with the rest of its query.
fn set_cookie(path: &str, rest: &str, token: &str) -> Verdict {
    let location = if rest.is_empty() { path.to_string() } else { format!("{path}?{rest}") };
    let cookie = format!("{COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={COOKIE_MAX_AGE_S}");
    Verdict::SetCookie { location, cookie }
}

impl Access {
    /// The access a server on `addrs` gets: refused when other machines can reach it and it has no token, unless in
    /// dev mode (`dev`), which uses no token at all.
    pub fn new(addrs: &[SocketAddr], token: Option<String>, dev: bool) -> Result<Access, String> {
        if dev {
            return Ok(Access { token: None, open_network: !all_loopback(addrs) });
        }
        if token.is_none() && !all_loopback(addrs) {
            return Err("the server would be open to other machines: give it a token (--token, or token = \"...\" in \
                        the settings file), listen on 127.0.0.1, or run it in dev mode (--dev, or dev = true)"
                .into());
        }
        Ok(Access { token, open_network: false })
    }

    pub fn has_token(&self) -> bool {
        self.token.is_some()
    }

    /// Whether other machines get in without a token (dev mode on a network address).
    pub fn is_open_network(&self) -> bool {
        self.open_network
    }

    /// Whether a request may go on.
    pub fn check(&self, method: &Method, uri: &Uri, headers: &HeaderMap) -> Verdict {
        let host = header(headers, HOST).or_else(|| uri.authority().map(|authority| authority.as_str()));
        let origin = header(headers, ORIGIN);
        // a page of another site (a link, a form, a script) never gets in; a page on this machine may
        if header(headers, "sec-fetch-site") == Some("cross-site") && !origin.is_some_and(is_loopback_origin) {
            return Verdict::Refuse { status: StatusCode::FORBIDDEN, reason: "a request from another site" };
        }
        let Some(token) = &self.token else {
            return if self.open_network { check_local_network(host, origin) } else { check_this_machine(host, origin) };
        };
        let is_token = |given: &str| equal_in_constant_time(given.as_bytes(), token.as_bytes());
        if bearer_token(headers).is_some_and(is_token) {
            return Verdict::Pass;
        }
        if cookie_token(headers).is_some_and(is_token) {
            return check_own_page(host, origin);
        }
        if matches!(*method, Method::GET | Method::HEAD)
            && let (Some(given), rest) = query_token(uri.query().unwrap_or_default())
            && is_token(given.as_str())
        {
            return set_cookie(uri.path(), &rest, token);
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

    fn refused(verdict: &Verdict) -> Option<StatusCode> {
        match verdict {
            Verdict::Refuse { status, .. } => Some(*status),
            _ => None,
        }
    }

    #[test]
    fn another_machine_can_reach_the_server_only_with_a_token() {
        assert!(Access::new(&lan(), None, false).is_err());
        assert!(Access::new(&["192.168.1.20:8770".parse().unwrap()], None, false).is_err());
        assert!(Access::new(&lan(), Some(TOKEN.into()), false).is_ok());
        assert!(Access::new(&loopback(), None, false).is_ok());
        assert!(Access::new(&["[::1]:8770".parse().unwrap()], None, false).is_ok());
    }

    #[test]
    fn without_a_token_only_this_machine_gets_in() {
        let a = Access::new(&loopback(), None, false).unwrap();
        for host in ["127.0.0.1:8770", "localhost:4200", "[::1]:8770", "LOCALHOST", "app.localhost:8770"] {
            assert_eq!(check(&a, Method::GET, "/api/vods", &[("host", host)]), Verdict::Pass, "{host}");
        }
        // the UI's own pages, through the Angular dev server too
        let page = [("host", "127.0.0.1:8770"), ("origin", "http://localhost:4201")];
        assert_eq!(check(&a, Method::POST, "/api/analyse?id=x", &page), Verdict::Pass);
        // DNS rebinding: a name of another site that points at 127.0.0.1
        let rebound = [("host", "evil.example:8770")];
        assert_eq!(refused(&check(&a, Method::GET, "/api/vods", &rebound)), Some(StatusCode::FORBIDDEN));
        assert!(refused(&check(&a, Method::GET, "/", &[])).is_some(), "no host");
        // another site's page posting to the server
        let cross = [("host", "127.0.0.1:8770"), ("origin", "https://evil.example")];
        assert_eq!(refused(&check(&a, Method::POST, "/api/analyse?id=x", &cross)), Some(StatusCode::FORBIDDEN));
        let cross = [("host", "127.0.0.1:8770"), ("sec-fetch-site", "cross-site")];
        assert_eq!(refused(&check(&a, Method::GET, "/video?id=x", &cross)), Some(StatusCode::FORBIDDEN));
        let null = [("host", "127.0.0.1:8770"), ("origin", "null")];
        assert!(refused(&check(&a, Method::POST, "/api/run?id=x", &null)).is_some());
        // the UI in browser mode, on another port of this machine: a cross-site request, from a loopback page
        let browser =
            [("host", "127.0.0.1:8770"), ("origin", "http://localhost:4200"), ("sec-fetch-site", "cross-site")];
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
        let a = Access::new(&lan(), Some(TOKEN.into()), false).unwrap();
        let host = ("host", "192.168.1.20:8770");
        let ok = format!("Bearer {TOKEN}");
        assert_eq!(check(&a, Method::POST, "/api/analyse?id=x", &[host, ("authorization", &ok)]), Verdict::Pass);
        let wrong = format!("Bearer {TOKEN}x");
        let verdict = check(&a, Method::GET, "/api/vods", &[host, ("authorization", &wrong)]);
        assert_eq!(refused(&verdict), Some(StatusCode::UNAUTHORIZED));
        assert_eq!(refused(&check(&a, Method::GET, "/api/vods", &[host])), Some(StatusCode::UNAUTHORIZED));
        let no_bearer = [host, ("authorization", TOKEN)];
        assert_eq!(refused(&check(&a, Method::GET, "/", &no_bearer)), Some(StatusCode::UNAUTHORIZED));
    }

    #[test]
    fn the_token_in_the_query_sets_the_cookie_and_the_cookie_lets_the_browser_in() {
        let a = Access::new(&lan(), Some(TOKEN.into()), false).unwrap();
        let host = ("host", "192.168.1.20:8770");
        let verdict = check(&a, Method::GET, &format!("/?token={TOKEN}"), &[host]);
        let Verdict::SetCookie { location, cookie } = verdict else { panic!("no cookie: {verdict:?}") };
        assert_eq!(location, "/");
        assert!(cookie.starts_with(&format!("{COOKIE_NAME}={TOKEN};")));
        assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Strict"));
        // the rest of the query stays
        let verdict = check(&a, Method::GET, &format!("/run?a=1&token={TOKEN}&b=2"), &[host]);
        assert!(matches!(verdict, Verdict::SetCookie { location, .. } if location == "/run?a=1&b=2"));
        // a wrong token sets nothing
        let verdict = check(&a, Method::GET, "/?token=guess", &[host]);
        assert_eq!(refused(&verdict), Some(StatusCode::UNAUTHORIZED));

        let jar = format!("theme=dark; {COOKIE_NAME}={TOKEN}");
        assert_eq!(check(&a, Method::GET, "/api/vods", &[host, ("cookie", &jar)]), Verdict::Pass);
        let page = [host, ("cookie", &jar), ("origin", "http://192.168.1.20:8770")];
        assert_eq!(check(&a, Method::POST, "/api/analyse?id=x", &page), Verdict::Pass);
        // the cookie on a request another site's page made
        let other = [host, ("cookie", &jar), ("origin", "http://192.168.1.30:8000")];
        assert_eq!(refused(&check(&a, Method::POST, "/api/analyse?id=x", &other)), Some(StatusCode::FORBIDDEN));
        let stale = format!("{COOKIE_NAME}=old-token");
        let verdict = check(&a, Method::GET, "/api/vods", &[host, ("cookie", &stale)]);
        assert_eq!(refused(&verdict), Some(StatusCode::UNAUTHORIZED));
    }

    #[test]
    fn dev_mode_lets_the_local_network_in_without_a_token() {
        let a = Access::new(&lan(), Some(TOKEN.into()), true).unwrap();
        assert!(a.is_open_network() && !a.has_token(), "dev mode uses no token, even when one is set");
        for host in ["192.168.1.111:8770", "10.0.0.5", "[fe80::1]:8770", "my-pc:8770", "my-pc.local:8770", "localhost"] {
            assert_eq!(check(&a, Method::GET, "/api/vods", &[("host", host)]), Verdict::Pass, "{host}");
        }
        // the server's own page, and the UI in browser mode on this machine
        let own = [("host", "192.168.1.111:8770"), ("origin", "http://192.168.1.111:8770")];
        assert_eq!(check(&a, Method::POST, "/api/analyse?id=x", &own), Verdict::Pass);
        let browser = [("host", "127.0.0.1:8770"), ("origin", "http://localhost:4200"), ("sec-fetch-site", "cross-site")];
        assert_eq!(check(&a, Method::POST, "/api/link", &browser), Verdict::Pass);
        // DNS rebinding: a web site's name that points at this machine
        let rebound = [("host", "evil.example:8770")];
        assert_eq!(refused(&check(&a, Method::GET, "/api/vods", &rebound)), Some(StatusCode::FORBIDDEN));
        assert!(refused(&check(&a, Method::GET, "/", &[])).is_some(), "no host");
        // another site's page
        let cross = [("host", "192.168.1.111:8770"), ("origin", "https://evil.example")];
        assert_eq!(refused(&check(&a, Method::POST, "/api/analyse?id=x", &cross)), Some(StatusCode::FORBIDDEN));
        let cross = [("host", "192.168.1.111:8770"), ("sec-fetch-site", "cross-site")];
        assert_eq!(refused(&check(&a, Method::GET, "/video?id=x", &cross)), Some(StatusCode::FORBIDDEN));
        // on a loopback address dev mode is this machine only, as without a token
        let here = Access::new(&loopback(), None, true).unwrap();
        assert!(!here.is_open_network());
        let lan_host = [("host", "192.168.1.111:8770")];
        assert_eq!(refused(&check(&here, Method::GET, "/api/vods", &lan_host)), Some(StatusCode::FORBIDDEN));
    }

    #[test]
    fn local_network_names() {
        for host in ["192.168.1.2:8770", "[::2]:1", "my-pc", "MY-PC.local:8770", "localhost", "127.0.0.1"] {
            assert!(is_local_network_host(host), "{host}");
        }
        for host in ["", "example.com", "evil.example:8770", "my-pc.local.evil.example", "localhost.example.com"] {
            assert!(!is_local_network_host(host), "{host}");
        }
    }

    #[test]
    fn loopback_names() {
        let loopback = ["localhost", "localhost:1", "127.0.0.1", "127.8.9.10:80", "[::1]", "[::1]:8770", "::1"];
        for host in loopback.into_iter().chain(["[::ffff:127.0.0.1]:1"]) {
            assert!(is_loopback_host(host), "{host}");
        }
        let elsewhere = ["", "example.com", "localhost.example.com", "192.168.1.2:8770", "[::2]:1", "0.0.0.0"];
        for host in elsewhere.into_iter().chain(["localhost:abc"]) {
            assert!(!is_loopback_host(host), "{host}");
        }
    }
}
