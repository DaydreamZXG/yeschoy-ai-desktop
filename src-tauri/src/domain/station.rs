//! Station identity, independent of HTTP clients, Tauri, and secret/trust stores.
//!
//! A structurally valid origin is an identity, **not** user authorization. In
//! particular, reading a previously installed credential must not require a
//! possibly missing trust file; authorization belongs to the connect/session
//! boundary. No built-in station allowlist is part of this model.

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct StationOrigin(String);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct InvalidStationOrigin;

impl StationOrigin {
    /// Parse canonical HTTPS identity. A single root slash is accepted for
    /// compatibility with existing connect inputs; the stored identity omits it.
    /// Do not normalize untrusted URLs: that would erase userinfo, ports, encoded
    /// hostnames, and other evidence that the declared identity was not canonical.
    pub(crate) fn parse(raw: &str) -> Result<Self, InvalidStationOrigin> {
        let origin = raw.strip_suffix('/').unwrap_or(raw);
        let host = origin
            .strip_prefix("https://")
            .ok_or(InvalidStationOrigin)?;
        if !canonical_host_is_valid(host) {
            return Err(InvalidStationOrigin);
        }
        Ok(Self(origin.to_owned()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn into_string(self) -> String {
        self.0
    }
}

fn canonical_host_is_valid(host: &str) -> bool {
    // Canonical DNS spelling (including already encoded IDNA/punycode and
    // underscore names accepted by the existing URL boundary). Reject trailing
    // dots / empty labels rather than introducing multiple identities for a host.
    if host.is_empty()
        || !host.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_' | b'.')
        })
        || host.split('.').any(str::is_empty)
    {
        return false;
    }
    // URL parsers accept abbreviated, integer, octal and hexadecimal IPv4, not
    // just Rust's dotted-decimal form. A numeric final label invokes those rules;
    // reject it before any transport can reinterpret this identity as an IP.
    let last = host.rsplit('.').next().unwrap_or_default();
    let numeric = last.bytes().all(|b| b.is_ascii_digit())
        || last
            .strip_prefix("0x")
            .is_some_and(|hex| hex.bytes().all(|b| b.is_ascii_hexdigit()));
    !numeric
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn station_origin_supports_third_party_and_historical_identities() {
        for origin in [
            "https://yeschoy.com",
            "https://api.yeschoy.com",
            "https://ai.yeschoy.io",
            "https://third-party.station.example",
            "https://xn--bcher-kva.example",
            "https://relay_name.example",
        ] {
            let identity = StationOrigin::parse(origin).unwrap();
            assert_eq!(identity.as_str(), origin);
            assert_eq!(StationOrigin::parse(&format!("{origin}/")), Ok(identity));
        }
    }

    #[test]
    fn station_origin_rejects_noncanonical_and_unsafe_shapes_without_io() {
        for origin in [
            "",
            "http://relay.example",
            "HTTPS://relay.example",
            "https://RELAY.example",
            " https://relay.example",
            "https://relay.example\n",
            "https://user@relay.example",
            "https://user:pass@relay.example",
            "https://@relay.example",
            "https://relay.example:443",
            "https://relay.example:8443",
            "https://relay.example/path",
            "https://relay.example//",
            "https://relay.example/.",
            "https://relay.example/../",
            "https://relay.example?",
            "https://relay.example?x=1",
            "https://relay.example#",
            "https://relay.example#fragment",
            "https://relay.example\\path",
            "https://%72elay.example",
            "https://bücher.example",
            "https://relay.example.",
            "https://relay..example",
            "https://127.0.0.1",
            "https://127.1",
            "https://2130706433",
            "https://0177.0.0.1",
            "https://0x7f000001",
            "https://0x7f.0.0.1",
            "https://[::1]",
            "https://[::ffff:127.0.0.1]",
        ] {
            assert!(StationOrigin::parse(origin).is_err(), "accepted {origin:?}");
        }
    }

    #[test]
    fn station_identity_does_not_imply_authorization_or_rewrite_legacy_hosts() {
        let legacy = StationOrigin::parse("https://api.yeschoy.com").unwrap();
        let default = StationOrigin::parse("https://yeschoy.com").unwrap();
        let unknown = StationOrigin::parse("https://untrusted.station.example").unwrap();
        assert_ne!(legacy, default);
        assert_eq!(legacy.into_string(), "https://api.yeschoy.com");
        assert_eq!(unknown.as_str(), "https://untrusted.station.example");
    }
}
