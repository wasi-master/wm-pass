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

/// Decodes RFC 4648 base32, ignoring case, spaces, dashes and padding. Any space counts, the
/// no-break space a web page puts between groups included, as [`normalize_secret`] has it.
pub fn base32_decode(text: &str) -> Result<Zeroizing<Vec<u8>>> {
    let mut out = Zeroizing::new(Vec::with_capacity(text.len() * 5 / 8));
    let (mut buffer, mut bits) = (0u64, 0u32);
    for ch in text.chars() {
        let value = match ch {
            'A'..='Z' => ch as u8 - b'A',
            'a'..='z' => ch as u8 - b'a',
            '2'..='7' => ch as u8 - b'2' + 26,
            '-' | '=' => continue,
            ch if ch.is_whitespace() => continue,
            _ => {
                return Err(Error::Invalid(
                    "the setup key can hold only the letters A to Z and the digits 2 to 7".into(),
                ));
            }
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

/// A seed as other apps expect to read it: upper case, with no spaces, dashes or padding.
/// People type seeds the way sites show them, `jbsw y3dp ehpk 3pxp`.
pub fn normalize_secret(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace() && !matches!(c, '-' | '='))
        .map(|c| c.to_ascii_uppercase())
        .collect()
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
        // Pasted links often carry a line break, and QR codes in alphanumeric mode are upper case.
        let uri = uri.trim();
        let scheme = |name: &str| uri.get(..name.len()).filter(|head| head.eq_ignore_ascii_case(name)).map(|_| &uri[name.len()..]);
        if let Some(seed) = scheme("steam://") {
            return Ok(Self {
                secret: seed.into(),
                kind: OtpKind::Steam,
                digits: 5,
                ..Self::default()
            });
        }
        let rest = scheme("otpauth://").ok_or_else(invalid)?;
        // Zoho Vault writes links with no label at all: `otpauth://totp?secret=…`.
        let kind_end = rest.find(['/', '?']).ok_or_else(invalid)?;
        let (kind, rest) = rest.split_at(kind_end);
        let rest = rest.strip_prefix('/').unwrap_or(rest);
        let (label, query) = rest.split_once('?').unwrap_or((rest, ""));
        // A label is a path, where `+` is itself: `me+2fa@example.com` is one address.
        let label = percent_decode(&label.replace('+', "%2B"));
        let (mut issuer, account) = match label.split_once(':') {
            Some((issuer, account)) => (issuer.trim().to_owned(), account.trim().to_owned()),
            None => (String::new(), label.trim().to_owned()),
        };
        let mut config = Self {
            account,
            digits: 6,
            ..Self::default()
        };
        let (mut period, mut counter, mut steam) = (30u32, 0u64, false);
        for pair in query.split('&') {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            let value = percent_decode(value);
            // Some apps write `issuer=` or `digits=` with nothing after it: the label's issuer,
            // or the default, stands.
            if value.trim().is_empty() {
                continue;
            }
            match key.to_ascii_lowercase().as_str() {
                "secret" => config.secret = Secret::from(value),
                "issuer" => issuer = value,
                "digits" => config.digits = value.trim().parse().map_err(|_| invalid())?,
                "period" => period = value.trim().parse().map_err(|_| invalid())?,
                "counter" => counter = value.trim().parse().map_err(|_| invalid())?,
                // KeePassXC marks a Steam code in a TOTP link this way.
                "encoder" => steam = value.eq_ignore_ascii_case("steam"),
                "algorithm" => {
                    // Some apps write `SHA-256`, or `HMAC-SHA256` as KeePass names it.
                    let name = value.trim().to_ascii_uppercase().replace(['-', '_'], "");
                    config.algorithm = match name.strip_prefix("HMAC").unwrap_or(&name) {
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
            "totp" if steam => OtpKind::Steam,
            "totp" => OtpKind::Totp { period },
            "hotp" => OtpKind::Hotp { counter },
            "steam" => OtpKind::Steam,
            _ => return Err(invalid()),
        };
        // A seed of nothing but padding, or a period of zero, makes no codes worth showing.
        if base32_decode(config.secret.expose())?.is_empty() || config.kind == (OtpKind::Totp { period: 0 }) {
            return Err(invalid());
        }
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
            Zeroizing::new(normalize_secret(self.secret.expose())).as_str(),
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
                // Two hex digits exactly: a sign, as in `%+1`, is no escape.
                let pair = &bytes[at + 1..at + 3];
                let hex = std::str::from_utf8(pair).ok().filter(|_| pair.iter().all(u8::is_ascii_hexdigit));
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
    fn links_read_in_upper_case_with_stray_space_and_write_clean_seeds() {
        let config = OtpConfig::from_uri(" OTPAUTH://TOTP/ACME:ME?SECRET=JBSWY3DPEHPK3PXP&ALGORITHM=SHA-256\n").unwrap();
        assert_eq!(config.account, "ME");
        assert_eq!(config.algorithm, OtpAlgorithm::Sha256);
        assert!(OtpConfig::from_uri("STEAM://JBSWY3DPEHPK3PXP").is_ok_and(|c| c.kind == OtpKind::Steam));
        let typed = OtpConfig::totp("jbsw y3dp-ehpk 3pxp==");
        assert!(typed.to_uri().expose().contains("secret=JBSWY3DPEHPK3PXP&"), "{}", typed.to_uri().expose());
        assert_eq!(OtpConfig::from_uri(typed.to_uri().expose()).unwrap().code(59).unwrap().expose(), typed.code(59).unwrap().expose());
    }

    #[test]
    fn keepassxc_steam_links_and_unlabelled_links_read() {
        let steam = OtpConfig::from_uri(
            "otpauth://totp/Steam:me?secret=JBSWY3DPEHPK3PXP&period=30&digits=5&issuer=Steam&encoder=steam",
        )
        .unwrap();
        assert_eq!(steam.kind, OtpKind::Steam);
        assert_eq!(steam.digits, 5);
        let bare = OtpConfig::from_uri("otpauth://totp?secret=JBSWY3DPEHPK3PXP&issuer=Zoho").unwrap();
        assert_eq!(bare.kind, OtpKind::Totp { period: 30 });
        assert_eq!(bare.issuer, "Zoho");
        assert_eq!(bare.account, "");
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
        assert_eq!(base32_decode("MZXW\t6YTB\r\nOI").unwrap().as_slice(), b"foobar");
    }

    #[test]
    fn a_seed_copied_from_a_page_with_no_break_spaces_reads() {
        let copied = "JBSW\u{a0}Y3DP\u{a0}EHPK\u{2009}3PXP";
        assert_eq!(base32_decode(copied).unwrap().as_slice(), base32_decode("JBSWY3DPEHPK3PXP").unwrap().as_slice());
        let config = OtpConfig::totp(copied);
        assert_eq!(config.code(59).unwrap(), OtpConfig::totp("JBSWY3DPEHPK3PXP").code(59).unwrap());
        let message = base32_decode("JBSW0").unwrap_err().to_string();
        assert!(message.contains("2 to 7"), "{message}");
    }

    #[test]
    fn links_that_make_no_codes_are_refused() {
        for bad in [
            "otpauth://totp/x?secret=%3D%3D%3D%3D",
            "otpauth://totp/x?secret=----",
            "otpauth://totp/x?secret=JBSWY3DPEHPK3PXP&period=0",
        ] {
            assert!(OtpConfig::from_uri(bad).is_err(), "{bad}");
        }
        assert!(OtpConfig::from_uri("otpauth://hotp/x?secret=JBSWY3DPEHPK3PXP&counter=0").is_ok());
    }

    #[test]
    fn percent_handles_a_trailing_marker() {
        assert_eq!(percent_decode("a%2"), "a%2");
        assert_eq!(percent_decode("a%20b+c"), "a b c");
        assert_eq!(percent_decode("%"), "%");
        // A sign is no hex digit: the escape is not taken, and the plus is a space as ever.
        assert_eq!(percent_decode("%+1x"), "% 1x");
    }

    #[test]
    fn a_plus_in_the_label_and_an_empty_issuer_keep_what_the_label_says() {
        let config = OtpConfig::from_uri("otpauth://totp/ACME:me+2fa@example.com?secret=JBSWY3DPEHPK3PXP&issuer=").unwrap();
        assert_eq!(config.account, "me+2fa@example.com");
        assert_eq!(config.issuer, "ACME");
        let blank = OtpConfig::from_uri("otpauth://totp/x?secret=JBSWY3DPEHPK3PXP&digits=&period=&algorithm=").unwrap();
        assert_eq!((blank.digits, blank.kind, blank.algorithm), (6, OtpKind::Totp { period: 30 }, OtpAlgorithm::Sha1));
        let named = OtpConfig::from_uri("otpauth://totp/x?secret=JBSWY3DPEHPK3PXP&algorithm=HMAC-SHA-512").unwrap();
        assert_eq!(named.algorithm, OtpAlgorithm::Sha512);
        // The parameter still wins when it says something.
        let named = OtpConfig::from_uri("otpauth://totp/Old:me?secret=JBSWY3DPEHPK3PXP&issuer=New+Name").unwrap();
        assert_eq!(named.issuer, "New Name");
    }
}
