//! The `WIFI:` text phones show as a QR code to share a network, and read to join one.

use zeroize::Zeroizing;

/// A network as the text describes it. `security` is one of the Wi-Fi item's choices: open,
/// wep, wpa, sae or eap.
#[derive(Debug, PartialEq, Eq)]
pub struct Network {
    pub ssid: String,
    pub security: &'static str,
    pub password: Zeroizing<String>,
    pub hidden: bool,
}

/// Reads a `WIFI:` text. None when the text is not one.
pub fn parse(text: &str) -> Option<Network> {
    let text = text.trim();
    if !text.get(..5).is_some_and(|head| head.eq_ignore_ascii_case("WIFI:")) {
        return None;
    }
    let mut ssid = String::new();
    let mut kind = String::new();
    let mut password = Zeroizing::new(String::new());
    let mut hidden = String::new();
    let mut key = String::new();
    let mut value = Zeroizing::new(String::new());
    let mut in_value = false;
    let mut escaped = false;
    for c in text[5..].chars() {
        if escaped {
            if in_value { value.push(c) } else { key.push(c) }
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == ':' && !in_value {
            in_value = true;
        } else if c == ';' {
            match key.to_ascii_uppercase().as_str() {
                "S" => ssid = value.to_string(),
                "T" => kind = value.to_string(),
                "P" => password = Zeroizing::new(value.to_string()),
                "H" => hidden = value.to_string(),
                _ => {}
            }
            key.clear();
            value.clear();
            in_value = false;
        } else if in_value {
            value.push(c);
        } else {
            key.push(c);
        }
    }
    let security = match kind.to_ascii_uppercase().as_str() {
        "WPA" | "WPA2" => "wpa",
        "SAE" | "WPA3" => "sae",
        "WEP" => "wep",
        "WPA2-EAP" | "EAP" => "eap",
        _ if password.is_empty() => "open",
        _ => "wpa",
    };
    Some(Network { ssid, security, password, hidden: hidden.eq_ignore_ascii_case("true") })
}

/// The text for a network, for another phone's camera. None for an enterprise network, which
/// has no such code, and for a network without a name.
pub fn code(ssid: &str, security: &str, password: &str, hidden: bool) -> Option<Zeroizing<String>> {
    let kind = match security {
        "wpa" => "WPA",
        "sae" => "SAE",
        "wep" => "WEP",
        "open" => "nopass",
        _ => return None,
    };
    if ssid.is_empty() {
        return None;
    }
    let escaped = |value: &str| {
        let mut out = String::with_capacity(value.len());
        for c in value.chars() {
            if matches!(c, '\\' | ';' | ',' | ':' | '"') {
                out.push('\\');
            }
            out.push(c);
        }
        out
    };
    let mut out = Zeroizing::new(format!("WIFI:T:{kind};S:{};", escaped(ssid)));
    if kind != "nopass" {
        out.push_str("P:");
        out.push_str(&Zeroizing::new(escaped(password)));
        out.push(';');
    }
    if hidden {
        out.push_str("H:true;");
    }
    out.push(';');
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_what_phones_share() {
        let n = parse("WIFI:T:WPA;S:Home \\;Net;P:pa\\:ss;H:true;;").unwrap();
        assert_eq!(n.ssid, "Home ;Net");
        assert_eq!(n.security, "wpa");
        assert_eq!(n.password.as_str(), "pa:ss");
        assert!(n.hidden);
        assert_eq!(parse("wifi:S:Cafe;;").unwrap().security, "open", "no password is an open network");
        assert_eq!(parse("WIFI:S:Cafe;P:x;;").unwrap().security, "wpa", "a password without a type is WPA");
        assert_eq!(parse("WIFI:T:WPA3;S:a;P:b;;").unwrap().security, "sae");
        assert_eq!(parse("WIFI:T:WPA2-EAP;S:a;;").unwrap().security, "eap");
        assert!(parse("otpauth://totp/x").is_none());
    }

    #[test]
    fn builds_what_it_reads() {
        let text = code("Home ;Net", "wpa", "pa:ss\\", true).unwrap();
        assert_eq!(text.as_str(), "WIFI:T:WPA;S:Home \\;Net;P:pa\\:ss\\\\;H:true;;");
        let back = parse(&text).unwrap();
        assert_eq!((back.ssid.as_str(), back.security, back.password.as_str(), back.hidden), ("Home ;Net", "wpa", "pa:ss\\", true));
        assert_eq!(code("Cafe", "open", "ignored", false).unwrap().as_str(), "WIFI:T:nopass;S:Cafe;;");
        assert!(code("Work", "eap", "x", false).is_none(), "enterprise networks have no code");
        assert!(code("", "wpa", "x", false).is_none());
    }
}
