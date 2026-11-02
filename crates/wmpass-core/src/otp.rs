//! One-time codes: HOTP (RFC 4226), TOTP (RFC 6238), Steam Guard.

use chacha20poly1305::aead::KeyInit;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use sha2::{Sha256, Sha512};
use zeroize::Zeroizing;

use crate::{Error, Result, Secret};

/// Hash used by the code.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum OtpAlgorithm {
    /// HMAC-SHA1, what nearly every site uses.
    #[default]
    Sha1,
    /// HMAC-SHA256.
    Sha256,
    /// HMAC-SHA512.
    Sha512,
}

/// How codes advance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OtpKind {
    /// Time based, with a period in seconds.
    Totp {
        /// Seconds per code.
        period: u32,
    },
    /// Counter based. The counter is the next value to use.
    Hotp {
        /// Next counter.
        counter: u64,
    },
    /// Steam's five-character alphabet, 30 seconds.
    Steam,
}

impl Default for OtpKind {
    fn default() -> Self {
        Self::Totp { period: 30 }
    }
}

/// Everything needed to produce codes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OtpConfig {
    /// Base32 seed.
    pub secret: Secret,
    /// Issuer, such as a site's name.
    pub issuer: String,
    /// Account label.
    pub account: String,
    /// Hash.
    pub algorithm: OtpAlgorithm,
    /// Code length, 6 to 10.
    pub digits: u32,
    /// Time, counter or Steam.
    pub kind: OtpKind,
}

const STEAM_ALPHABET: &[u8] = b"23456789BCDFGHJKMNPQRTVWXY";

/// Decodes RFC 4648 base32, ignoring case, spaces, dashes and padding.
pub fn base32_decode(text: &str) -> Result<Zeroizing<Vec<u8>>> {
    let mut out = Zeroizing::new(Vec::with_capacity(text.len() * 5 / 8));
    let (mut buffer, mut bits) = (0u64, 0u32);
    for ch in text.bytes() {
        let value = match ch {
            b'A'..=b'Z' => ch - b'A',
            b'a'..=b'z' => ch - b'a',
            b'2'..=b'7' => ch - b'2' + 26,
            b' ' | b'-' | b'=' => continue,
            _ => return Err(Error::Invalid("the secret is not valid base32".into())),
        };
        buffer = (buffer << 5) | u64::from(value);
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(((buffer >> bits) & 0xff) as u8);
        }
    }
    Ok(out)
}

/// Encodes RFC 4648 base32 without padding.
pub fn base32_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut out = String::with_capacity(bytes.len() * 8 / 5 + 1);
    let (mut buffer, mut bits) = (0u64, 0u32);
    for byte in bytes {
        buffer = (buffer << 8) | u64::from(*byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((buffer >> bits) & 31) as usize] as char);
        }
        buffer &= (1u64 << bits) - 1;
    }
    if bits > 0 {
        out.push(ALPHABET[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    out
}

fn hmac_digest(algorithm: OtpAlgorithm, key: &[u8], message: &[u8]) -> Result<Vec<u8>> {
    fn run<M: Mac + KeyInit>(key: &[u8], message: &[u8]) -> Result<Vec<u8>> {
        let mut mac = <M as KeyInit>::new_from_slice(key).map_err(|_| Error::Crypto)?;
        mac.update(message);
        Ok(mac.finalize().into_bytes().to_vec())
    }
    match algorithm {
        OtpAlgorithm::Sha1 => run::<Hmac<Sha1>>(key, message),
        OtpAlgorithm::Sha256 => run::<Hmac<Sha256>>(key, message),
        OtpAlgorithm::Sha512 => run::<Hmac<Sha512>>(key, message),
    }
}

fn truncate(digest: &[u8]) -> Result<u32> {
    let last = *digest.last().ok_or(Error::Crypto)?;
    let at = usize::from(last & 0x0f);
    let word = digest.get(at..at + 4).ok_or(Error::Crypto)?;
    let value = u32::from_be_bytes(word.try_into().map_err(|_| Error::Crypto)?);
    Ok(value & 0x7fff_ffff)
}

fn decimal_code(value: u32, digits: u32) -> String {
    let digits = digits.clamp(6, 10);
    let modulus = 10u64.pow(digits);
    format!(
        "{:0width$}",
        u64::from(value) % modulus,
        width = digits as usize
    )
}

impl OtpConfig {
    /// Default settings for a seed: SHA-1, six digits, 30 seconds.
    pub fn totp(secret: impl Into<Secret>) -> Self {
        Self {
            secret: secret.into(),
            digits: 6,
            ..Self::default()
        }
    }

    fn counter_at(&self, unix_seconds: u64) -> u64 {
        match self.kind {
            OtpKind::Totp { period } => unix_seconds / u64::from(period.max(1)),
            OtpKind::Hotp { counter } => counter,
            OtpKind::Steam => unix_seconds / 30,
        }
    }

    /// The code at a moment. For HOTP the moment is ignored; call
    /// [`OtpConfig::advance`] after showing the code.
    pub fn code(&self, unix_seconds: u64) -> Result<Secret> {
        let key = base32_decode(self.secret.expose())?;
        if key.is_empty() {
            return Err(Error::Invalid("the one-time code has no secret".into()));
        }
        let counter = self.counter_at(unix_seconds).to_be_bytes();
        let digest = hmac_digest(self.algorithm, &key, &counter)?;
        let value = truncate(&digest)?;
        Ok(match self.kind {
            OtpKind::Steam => {
                let mut rest = value;
                let mut code = String::with_capacity(5);
                for _ in 0..5 {
                    let base = STEAM_ALPHABET.len() as u32;
                    code.push(STEAM_ALPHABET[(rest % base) as usize] as char);
                    rest /= base;
                }
                Secret::from(code)
            }
            _ => Secret::from(decimal_code(value, self.digits)),
        })
    }

    /// Seconds until the code changes. `None` for HOTP.
    pub fn seconds_left(&self, unix_seconds: u64) -> Option<u32> {
        let period = match self.kind {
            OtpKind::Totp { period } => u64::from(period.max(1)),
            OtpKind::Steam => 30,
            OtpKind::Hotp { .. } => return None,
        };
        u32::try_from(period - unix_seconds % period).ok()
    }

    /// Moves a HOTP counter on. Does nothing for other kinds.
    pub fn advance(&mut self) {
        if let OtpKind::Hotp { counter } = &mut self.kind {
            *counter = counter.saturating_add(1);
        }
    }

    /// Parses an `otpauth://` link. A `steam://` seed is also accepted.
    pub fn from_uri(uri: &str) -> Result<Self> {
        let invalid = || Error::Invalid("not a valid one-time code link".to_string());
        if let Some(seed) = uri.strip_prefix("steam://") {
            return Ok(Self {
                secret: seed.into(),
                kind: OtpKind::Steam,
                digits: 5,
                ..Self::default()
            });
        }
        let rest = uri.strip_prefix("otpauth://").ok_or_else(invalid)?;
        let (kind, rest) = rest.split_once('/').ok_or_else(invalid)?;
        let (label, query) = rest.split_once('?').unwrap_or((rest, ""));
        let label = percent_decode(label);
        let (mut issuer, account) = match label.split_once(':') {
            Some((issuer, account)) => (issuer.trim().to_owned(), account.trim().to_owned()),
            None => (String::new(), label.trim().to_owned()),
        };
        let mut config = Self {
            account,
            digits: 6,
            ..Self::default()
        };
        let (mut period, mut counter) = (30u32, 0u64);
        for pair in query.split('&') {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            let value = percent_decode(value);
            match key.to_ascii_lowercase().as_str() {
                "secret" => config.secret = Secret::from(value),
                "issuer" => issuer = value,
                "digits" => config.digits = value.parse().map_err(|_| invalid())?,
                "period" => period = value.parse().map_err(|_| invalid())?,
                "counter" => counter = value.parse().map_err(|_| invalid())?,
                "algorithm" => {
                    config.algorithm = match value.to_ascii_uppercase().as_str() {
                        "SHA1" => OtpAlgorithm::Sha1,
                        "SHA256" => OtpAlgorithm::Sha256,
                        "SHA512" => OtpAlgorithm::Sha512,
                        _ => return Err(invalid()),
                    };
                }
                _ => {}
            }
        }
        config.issuer = issuer;
        config.kind = match kind.to_ascii_lowercase().as_str() {
            "totp" => OtpKind::Totp { period },
            "hotp" => OtpKind::Hotp { counter },
            "steam" => OtpKind::Steam,
            _ => return Err(invalid()),
        };
        if config.secret.is_empty() {
            return Err(invalid());
        }
        base32_decode(config.secret.expose())?;
        Ok(config)
    }

    /// Writes an `otpauth://` link. It contains the seed, so treat it as a secret.
    pub fn to_uri(&self) -> Secret {
        let label = if self.issuer.is_empty() {
            percent_encode(&self.account)
        } else {
            format!(
                "{}:{}",
                percent_encode(&self.issuer),
                percent_encode(&self.account)
            )
        };
        let algorithm = match self.algorithm {
            OtpAlgorithm::Sha1 => "SHA1",
            OtpAlgorithm::Sha256 => "SHA256",
            OtpAlgorithm::Sha512 => "SHA512",
        };
        let (kind, extra) = match self.kind {
            OtpKind::Totp { period } => ("totp", format!("&period={period}")),
            OtpKind::Hotp { counter } => ("hotp", format!("&counter={counter}")),
            OtpKind::Steam => ("steam", String::new()),
        };
        Secret::from(format!(
            "otpauth://{kind}/{label}?secret={}&issuer={}&algorithm={algorithm}&digits={}{extra}",
            self.secret.expose(),
            percent_encode(&self.issuer),
            self.digits,
        ))
    }
}

/// Decodes `%XX` and `+`.
pub fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'%' if at + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[at + 1..at + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(value) => {
                        out.push(value);
                        at += 3;
                        continue;
                    }
                    None => out.push(b'%'),
                }
            }
            b'+' => out.push(b' '),
            other => out.push(other),
        }
        at += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Encodes everything but unreserved characters.
pub fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // RFC 4226 Appendix D: secret "12345678901234567890".
    const RFC_SECRET: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";

    #[test]
    fn hotp_matches_rfc_4226() {
        let expected = [
            "755224", "287082", "359152", "969429", "338314", "254676", "287922", "162583",
            "399871", "520489",
        ];
        for (counter, want) in expected.iter().enumerate() {
            let config = OtpConfig {
                secret: RFC_SECRET.into(),
                digits: 6,
                kind: OtpKind::Hotp {
                    counter: counter as u64,
                },
                ..OtpConfig::default()
            };
            assert_eq!(config.code(0).unwrap().expose(), *want, "counter {counter}");
        }
    }

    #[test]
    fn totp_matches_rfc_6238() {
        // Appendix B. Secrets are the ASCII seed, repeated to each hash's length.
        let sha1 = base32_encode(b"12345678901234567890");
        let sha256 = base32_encode(b"12345678901234567890123456789012");
        let sha512 =
            base32_encode(b"1234567890123456789012345678901234567890123456789012345678901234");
        let cases = [
            (59u64, "94287082", "46119246", "90693936"),
            (1111111109, "07081804", "68084774", "25091201"),
            (1111111111, "14050471", "67062674", "99943326"),
            (1234567890, "89005924", "91819424", "93441116"),
            (2000000000, "69279037", "90698825", "38618901"),
            (20000000000, "65353130", "77737706", "47863826"),
        ];
        for (time, want1, want256, want512) in cases {
            for (seed, algorithm, want) in [
                (&sha1, OtpAlgorithm::Sha1, want1),
                (&sha256, OtpAlgorithm::Sha256, want256),
                (&sha512, OtpAlgorithm::Sha512, want512),
            ] {
                let config = OtpConfig {
                    secret: seed.as_str().into(),
                    digits: 8,
                    algorithm,
                    kind: OtpKind::Totp { period: 30 },
                    ..OtpConfig::default()
                };
                assert_eq!(
                    config.code(time).unwrap().expose(),
                    want,
                    "{time} {algorithm:?}"
                );
            }
        }
    }

    #[test]
    fn steam_codes_use_its_alphabet() {
        let config = OtpConfig {
            secret: RFC_SECRET.into(),
            kind: OtpKind::Steam,
            digits: 5,
            ..OtpConfig::default()
        };
        let code = config.code(1234567890).unwrap();
        assert_eq!(code.expose().len(), 5);
        assert!(code.expose().bytes().all(|b| STEAM_ALPHABET.contains(&b)));
    }

    #[test]
    fn uri_round_trip() {
        let uri = "otpauth://totp/ACME%20Co:john%40example.com?secret=JBSWY3DPEHPK3PXP&issuer=ACME%20Co&algorithm=SHA256&digits=8&period=60";
        let config = OtpConfig::from_uri(uri).unwrap();
        assert_eq!(config.issuer, "ACME Co");
        assert_eq!(config.account, "john@example.com");
        assert_eq!(config.digits, 8);
        assert_eq!(config.algorithm, OtpAlgorithm::Sha256);
        assert_eq!(config.kind, OtpKind::Totp { period: 60 });
        let again = OtpConfig::from_uri(config.to_uri().expose()).unwrap();
        assert_eq!(config, again);
    }

    #[test]
    fn uri_refuses_rubbish() {
        for bad in [
            "https://x",
            "otpauth://totp/x",
            "otpauth://totp/x?secret=!!!",
            "otpauth://nope/x?secret=AAAA",
        ] {
            assert!(OtpConfig::from_uri(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn hotp_advances_and_totp_counts_down() {
        let mut hotp = OtpConfig {
            secret: RFC_SECRET.into(),
            digits: 6,
            kind: OtpKind::Hotp { counter: 0 },
            ..OtpConfig::default()
        };
        let first = hotp.code(0).unwrap();
        hotp.advance();
        assert_ne!(first, hotp.code(0).unwrap());
        assert_eq!(hotp.seconds_left(0), None);
        let totp = OtpConfig::totp(RFC_SECRET);
        assert_eq!(totp.seconds_left(29), Some(1));
        assert_eq!(totp.seconds_left(30), Some(30));
    }

    #[test]
    fn base32_round_trips_and_tolerates_formatting() {
        assert_eq!(base32_encode(b"foobar"), "MZXW6YTBOI");
        assert_eq!(
            base32_decode("mzxw 6ytb-oi==").unwrap().as_slice(),
            b"foobar"
        );
        assert!(base32_decode("0189").is_err());
    }

    #[test]
    fn percent_handles_a_trailing_marker() {
        assert_eq!(percent_decode("a%2"), "a%2");
        assert_eq!(percent_decode("a%20b+c"), "a b c");
        assert_eq!(percent_decode("%"), "%");
    }
}
