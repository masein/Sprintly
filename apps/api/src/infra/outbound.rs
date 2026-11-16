//! Guard for outbound requests to user-configured URLs (webhooks).
//!
//! A project lead can point a webhook anywhere, and the worker POSTs to it from
//! inside the deployment's network. Unchecked, that is an SSRF primitive:
//! `http://redis:6379`, MinIO, Postgres, a cloud metadata endpoint — any
//! internal service is one webhook away, and curl would even speak `gopher://`
//! or `file://`. So a destination must pass three gates:
//!
//!   • **Shape.** `http`/`https` only, a plain host (letters, digits, `-`, `.`,
//!     or a bracketed IPv6 literal), no userinfo, no backslashes or control
//!     characters, no ambiguous numeric shorthand like `2130706433` or `127.1`.
//!     The URL handed to curl is *rebuilt* from the parsed parts, so curl can
//!     never read a different host out of it than the one checked here.
//!   • **Address.** Every address the host resolves to must be public:
//!     loopback, private, link-local, CGNAT, multicast, reserved and
//!     documentation ranges are refused — including IPv4 smuggled inside IPv6
//!     (mapped, compatible, NAT64, 6to4).
//!   • **Pinning.** curl is told to connect to the exact address that was
//!     checked (`--resolve`), so DNS can't flip between check and connect.
//!
//! Deployments that deliberately deliver to an internal host (a self-hosted
//! chat server on the LAN, say) list it in `SPRINTLY_WEBHOOK_ALLOWED_HOSTS` —
//! comma-separated host names, case-insensitive. An allowlisted host skips the
//! address check; the shape check still applies.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Env var naming hosts that may resolve to internal addresses.
pub const ALLOWED_HOSTS_ENV: &str = "SPRINTLY_WEBHOOK_ALLOWED_HOSTS";

/// A destination that passed the guard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    /// Canonical URL to hand to curl (rebuilt, never the raw input).
    pub url: String,
    /// `host:port:address` for curl's `--resolve`, pinning the connection to
    /// the address that was checked. `None` for IP literals (nothing to
    /// resolve) and allowlisted hosts.
    pub resolve: Option<String>,
}

/// Parsed pieces of an outbound URL.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Target {
    scheme: &'static str,
    /// Lowercased host; IPv6 literals without brackets.
    host: String,
    port: u16,
    /// Path + query, always starting with `/`. Fragment dropped.
    path_and_query: String,
}

impl Target {
    fn host_for_url(&self) -> String {
        if self.host.contains(':') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        }
    }

    fn url(&self) -> String {
        format!(
            "{}://{}:{}{}",
            self.scheme,
            self.host_for_url(),
            self.port,
            self.path_and_query
        )
    }
}

/// Check `raw` against the allowlist from [`ALLOWED_HOSTS_ENV`].
pub async fn check(raw: &str) -> Result<Checked, String> {
    let allowed: Vec<String> = std::env::var(ALLOWED_HOSTS_ENV)
        .unwrap_or_default()
        .split(',')
        .map(|h| h.trim().to_ascii_lowercase())
        .filter(|h| !h.is_empty())
        .collect();
    check_with(raw, &allowed).await
}

/// [`check`] with an explicit allowlist (tests pass their own).
pub async fn check_with(raw: &str, allowed_hosts: &[String]) -> Result<Checked, String> {
    let target = parse(raw)?;
    let url = target.url();

    if allowed_hosts.contains(&target.host) {
        return Ok(Checked { url, resolve: None });
    }

    // IP literal: nothing to resolve, just classify it.
    if let Ok(ip) = target.host.parse::<IpAddr>() {
        if is_forbidden(ip) {
            return Err(forbidden_msg(&target.host));
        }
        return Ok(Checked { url, resolve: None });
    }

    let addrs: Vec<IpAddr> = tokio::net::lookup_host((target.host.as_str(), target.port))
        .await
        .map_err(|_| format!("could not resolve host {}", target.host))?
        .map(|sa| sa.ip())
        .collect();
    let Some(first) = addrs.first().copied() else {
        return Err(format!("could not resolve host {}", target.host));
    };
    // Every answer must be public — otherwise a round-robin record could mix
    // one public address (to pass) with an internal one (to hit).
    if addrs.iter().any(|ip| is_forbidden(*ip)) {
        return Err(forbidden_msg(&target.host));
    }
    let pinned = match first {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => format!("[{v6}]"),
    };
    Ok(Checked {
        url,
        resolve: Some(format!("{}:{}:{pinned}", target.host, target.port)),
    })
}

fn forbidden_msg(host: &str) -> String {
    format!(
        "{host} is an internal address; webhooks may only target public hosts \
         (an operator can allow it via {ALLOWED_HOSTS_ENV})"
    )
}

/// Strict parse of an `http(s)` URL. Anything ambiguous is rejected rather
/// than interpreted.
fn parse(raw: &str) -> Result<Target, String> {
    let raw = raw.trim();
    if raw
        .chars()
        .any(|c| c.is_control() || c.is_whitespace() || c == '\\')
    {
        return Err("url contains whitespace, control characters or backslashes".into());
    }
    let (scheme, rest) = match raw.split_once("://") {
        Some((s, rest)) if s.eq_ignore_ascii_case("https") => ("https", rest),
        Some((s, rest)) if s.eq_ignore_ascii_case("http") => ("http", rest),
        _ => return Err("url must start with http:// or https://".into()),
    };

    let auth_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, remainder) = rest.split_at(auth_end);
    if authority.contains('@') {
        return Err("url must not contain credentials".into());
    }

    let (host, port) = split_host_port(authority)?;
    let port = match port {
        Some(p) => p,
        None if scheme == "https" => 443,
        None => 80,
    };

    // Drop the fragment; it's never sent anyway.
    let remainder = remainder.split('#').next().unwrap_or("");
    let path_and_query = if remainder.starts_with('/') {
        remainder.to_string()
    } else {
        format!("/{remainder}")
    };

    Ok(Target {
        scheme,
        host,
        port,
        path_and_query,
    })
}

fn split_host_port(authority: &str) -> Result<(String, Option<u16>), String> {
    let (host, port_str) = if let Some(inner) = authority.strip_prefix('[') {
        // [IPv6]:port
        let (v6, after) = inner
            .split_once(']')
            .ok_or_else(|| "unterminated IPv6 literal".to_string())?;
        if v6.parse::<Ipv6Addr>().is_err() {
            return Err("invalid IPv6 literal".into());
        }
        let port = match after {
            "" => None,
            p => Some(
                p.strip_prefix(':')
                    .ok_or_else(|| "invalid characters after IPv6 literal".to_string())?,
            ),
        };
        (v6.to_ascii_lowercase(), port)
    } else {
        match authority.split_once(':') {
            Some((h, p)) => (h.to_ascii_lowercase(), Some(p)),
            None => (authority.to_ascii_lowercase(), None),
        }
    };

    if host.is_empty() {
        return Err("url has no host".into());
    }
    if !host.contains(':') {
        validate_hostname(&host)?;
    }

    let port = match port_str {
        None => None,
        Some(p) => {
            if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
                return Err("invalid port".into());
            }
            match p.parse::<u16>() {
                Ok(n) if n > 0 => Some(n),
                _ => return Err("invalid port".into()),
            }
        }
    };
    Ok((host, port))
}

/// Letters, digits, `-` and `.` only (IDNs must arrive as punycode). A host
/// whose last label is numeric must be a canonical dotted-quad: resolvers
/// accept shorthands like `127.1` or `2130706433`, and we refuse to guess
/// what they mean.
fn validate_hostname(host: &str) -> Result<(), String> {
    let ok_chars = host
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.');
    if !ok_chars {
        return Err("host may contain only letters, digits, '-' and '.'".into());
    }
    let last = host.trim_end_matches('.').rsplit('.').next().unwrap_or("");
    let numeric_tld =
        !last.is_empty() && (last.bytes().all(|b| b.is_ascii_digit()) || last.starts_with("0x"));
    if numeric_tld && host.parse::<Ipv4Addr>().is_err() {
        return Err("numeric hosts must be written as a dotted-quad IPv4 address".into());
    }
    Ok(())
}

/// True for any address a webhook must never reach.
pub fn is_forbidden(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => forbidden_v4(v4),
        IpAddr::V6(v6) => forbidden_v6(v6),
    }
}

fn forbidden_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    a == 0 // 0.0.0.0/8 "this network"
        || ip.is_loopback() // 127.0.0.0/8
        || ip.is_private() // 10/8, 172.16/12, 192.168/16
        || ip.is_link_local() // 169.254/16 — cloud metadata lives here
        || (a == 100 && (64..=127).contains(&b)) // 100.64/10 carrier-grade NAT
        || (a == 192 && b == 0 && c == 0) // 192.0.0/24 IETF protocol assignments
        || (a == 198 && (b == 18 || b == 19)) // 198.18/15 benchmarking
        || ip.is_documentation() // TEST-NET-1/2/3
        || ip.is_multicast() // 224/4
        || a >= 240 // 240/4 reserved, incl. broadcast
}

fn forbidden_v6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = embedded_v4(ip) {
        return forbidden_v4(v4);
    }
    let s = ip.segments();
    ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_multicast()
        || (s[0] & 0xfe00) == 0xfc00 // fc00::/7 unique local
        || (s[0] & 0xffc0) == 0xfe80 // fe80::/10 link-local
        || (s[0] & 0xffc0) == 0xfec0 // fec0::/10 deprecated site-local
        || (s[0] == 0x2001 && s[1] == 0x0db8) // 2001:db8::/32 documentation
}

/// IPv4 addresses carried inside IPv6: mapped (`::ffff:a.b.c.d`), compatible
/// (`::a.b.c.d`), NAT64 (`64:ff9b::/96`) and 6to4 (`2002::/16`).
fn embedded_v4(ip: Ipv6Addr) -> Option<Ipv4Addr> {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return Some(v4);
    }
    let s = ip.segments();
    let [t0, t1] = s[6].to_be_bytes();
    let [t2, t3] = s[7].to_be_bytes();
    let tail = Ipv4Addr::new(t0, t1, t2, t3);
    if s[0] == 0x0064 && s[1] == 0xff9b && s[2..6] == [0, 0, 0, 0] {
        return Some(tail);
    }
    if s[0] == 0x2002 {
        let [a, b] = s[1].to_be_bytes();
        let [c, d] = s[2].to_be_bytes();
        return Some(Ipv4Addr::new(a, b, c, d));
    }
    if s[..6] == [0, 0, 0, 0, 0, 0] && !ip.is_loopback() && !ip.is_unspecified() {
        return Some(tail);
    }
    None
}

/// Short, non-revealing reason for a failed curl run, keyed on its exit code.
/// Raw stderr is never stored: it turned delivery logs into a port scanner.
pub fn curl_failure_reason(exit_code: Option<i32>) -> &'static str {
    match exit_code {
        Some(1) => "unsupported protocol",
        Some(6) => "could not resolve host",
        Some(7) => "could not connect",
        Some(28) => "timed out",
        Some(35 | 51 | 58 | 60) => "TLS error",
        Some(47) => "redirects are not followed",
        _ => "request failed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none() -> Vec<String> {
        Vec::new()
    }

    #[test]
    fn rejects_non_http_schemes() {
        for u in [
            "gopher://redis:6379/_FLUSHALL",
            "file:///etc/passwd",
            "dict://127.0.0.1:6379/info",
            "ftp://example.com/",
            "example.com/hook",
        ] {
            assert!(parse(u).is_err(), "{u}");
        }
    }

    #[test]
    fn rejects_ambiguous_shapes() {
        for u in [
            "http://user:pw@example.com/",
            "http://example.com\\@127.0.0.1/",
            "http://exa mple.com/",
            "http://example.com:99999/",
            "http://example.com:/",
            "http://example.com:80x/",
            "http://2130706433/",
            "http://127.1/",
            "http://0x7f.0.0.1/",
            "http://[::1/",
            "http://exämple.com/",
            "http:///path",
        ] {
            assert!(parse(u).is_err(), "{u}");
        }
    }

    #[test]
    fn rebuilds_a_canonical_url() {
        let t = parse("HTTPS://Hooks.Slack.com/services/T0/B0/x?y=1#frag").unwrap();
        assert_eq!(t.url(), "https://hooks.slack.com:443/services/T0/B0/x?y=1");
        let t = parse("http://example.com").unwrap();
        assert_eq!(t.url(), "http://example.com:80/");
        let t = parse("http://example.com?x=1").unwrap();
        assert_eq!(t.url(), "http://example.com:80/?x=1");
        let t = parse("http://[2606:4700::1111]:8080/h").unwrap();
        assert_eq!(t.url(), "http://[2606:4700::1111]:8080/h");
    }

    #[test]
    fn classifies_internal_v4() {
        for ip in [
            "0.0.0.0",
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "192.0.0.8",
            "198.18.0.1",
            "224.0.0.1",
            "240.0.0.1",
            "255.255.255.255",
        ] {
            assert!(is_forbidden(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["8.8.8.8", "1.1.1.1", "172.32.0.1", "100.128.0.1"] {
            assert!(!is_forbidden(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn classifies_internal_v6_including_embedded_v4() {
        for ip in [
            "::",
            "::1",
            "fc00::1",
            "fd12:3456::1",
            "fe80::1",
            "ff02::1",
            "::ffff:127.0.0.1",
            "::ffff:169.254.169.254",
            "::10.0.0.1",
            "64:ff9b::7f00:1",
            "2002:7f00:1::",
            "2001:db8::1",
        ] {
            assert!(is_forbidden(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["2606:4700:4700::1111", "::ffff:8.8.8.8", "2002:808:808::"] {
            assert!(!is_forbidden(ip.parse().unwrap()), "{ip}");
        }
    }

    #[tokio::test]
    async fn ip_literals_are_checked_without_dns() {
        assert!(check_with("http://127.0.0.1:6379/", &none()).await.is_err());
        assert!(check_with("http://[::1]/", &none()).await.is_err());
        assert!(
            check_with("http://169.254.169.254/latest/meta-data/", &none())
                .await
                .is_err()
        );
        let ok = check_with("https://8.8.8.8/hook", &none()).await.unwrap();
        assert_eq!(ok.url, "https://8.8.8.8:443/hook");
        assert_eq!(ok.resolve, None);
    }

    #[tokio::test]
    async fn localhost_name_is_refused() {
        // Resolves via /etc/hosts — no network needed.
        let err = check_with("http://localhost:8081/", &none())
            .await
            .unwrap_err();
        assert!(err.contains("internal address"), "{err}");
    }

    #[tokio::test]
    async fn allowlisted_hosts_skip_the_address_check() {
        let allowed = vec!["localhost".to_string(), "10.0.0.5".to_string()];
        let ok = check_with("http://LOCALHOST:8065/hooks/x", &allowed)
            .await
            .unwrap();
        assert_eq!(ok.url, "http://localhost:8065/hooks/x");
        assert_eq!(ok.resolve, None);
        assert!(check_with("http://10.0.0.5/", &allowed).await.is_ok());
        // …but the shape check still applies.
        assert!(check_with("gopher://localhost/", &allowed).await.is_err());
    }

    #[test]
    fn curl_failures_map_to_short_reasons() {
        assert_eq!(curl_failure_reason(Some(7)), "could not connect");
        assert_eq!(curl_failure_reason(Some(28)), "timed out");
        assert_eq!(curl_failure_reason(None), "request failed");
    }
}
