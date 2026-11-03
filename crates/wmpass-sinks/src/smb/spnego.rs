//! The SPNEGO wrapping SMB puts around NTLM (RFC 4178), with NTLM as the only mechanism
//! offered. Just enough DER to write the two client tokens and to find the server's token
//! in its answer.

use super::ntlm;

/// 1.3.6.1.5.5.2, SPNEGO itself.
const SPNEGO_OID: &[u8] = &[0x2b, 0x06, 0x01, 0x05, 0x05, 0x02];

/// 1.3.6.1.4.1.311.2.2.10, NTLMSSP.
const NTLM_OID: &[u8] = &[0x2b, 0x06, 0x01, 0x04, 0x01, 0x82, 0x37, 0x02, 0x02, 0x0a];

/// The DER of the MechTypeList, which the mechListMIC signs. Its own value because it
/// must be byte for byte what [`init`] sent.
pub(crate) fn mech_type_list() -> Vec<u8> {
    tlv(0x30, &tlv(0x06, NTLM_OID))
}

/// The first token: NegTokenInit, offering NTLM, carrying NTLM's message 1.
pub(crate) fn init(ntlm_negotiate: &[u8]) -> Vec<u8> {
    let inner = [tlv(0xa0, &mech_type_list()), tlv(0xa2, &tlv(0x04, ntlm_negotiate))].concat();
    let neg_token_init = tlv(0x30, &inner);
    tlv(0x60, &[tlv(0x06, SPNEGO_OID), tlv(0xa0, &neg_token_init)].concat())
}

/// The second token: NegTokenResp with NTLM's message 3 and the MIC.
pub(crate) fn response(ntlm_authenticate: &[u8], mech_list_mic: Option<&[u8]>) -> Vec<u8> {
    let mut body = tlv(0xa2, &tlv(0x04, ntlm_authenticate));
    if let Some(mic) = mech_list_mic {
        body.extend_from_slice(&tlv(0xa3, &tlv(0x04, mic)));
    }
    tlv(0xa1, &tlv(0x30, &body))
}

/// The mechanism token inside a server NegTokenResp, or the input itself when the server
/// answered with a bare NTLM message, which some do.
pub(crate) fn response_token(token: &[u8]) -> Option<&[u8]> {
    if token.len() >= 8 && &token[..8] == ntlm::SIGNATURE {
        return Some(token);
    }
    let outer = read(token, 0)?;
    if outer.tag != 0xa1 {
        return None;
    }
    let seq = read(token, outer.start)?;
    if seq.tag != 0x30 {
        return None;
    }
    let mut at = seq.start;
    while at < seq.end {
        let field = read(token, at)?;
        if field.tag == 0xa2 {
            let octets = read(token, field.start)?;
            if octets.tag != 0x04 {
                return None;
            }
            return token.get(octets.start..octets.end);
        }
        at = field.end;
    }
    None
}

/// A decoded tag, and where its contents start and end.
struct Tlv {
    tag: u8,
    start: usize,
    end: usize,
}

fn read(b: &[u8], at: usize) -> Option<Tlv> {
    let tag = *b.get(at)?;
    let mut len = usize::from(*b.get(at + 1)?);
    let mut start = at + 2;
    if len & 0x80 != 0 {
        let n = len & 0x7f;
        if n == 0 || n > 3 {
            return None;
        }
        len = 0;
        for _ in 0..n {
            len = (len << 8) | usize::from(*b.get(start)?);
            start += 1;
        }
    }
    let end = start.checked_add(len)?;
    (end <= b.len()).then_some(Tlv { tag, start, end })
}

/// One DER tag with its length and contents.
pub(crate) fn tlv(tag: u8, content: &[u8]) -> Vec<u8> {
    let n = content.len();
    let mut out = Vec::with_capacity(n + 6);
    out.push(tag);
    let be = n.to_be_bytes();
    match n {
        0..0x80 => out.push(be[be.len() - 1]),
        0x80..0x100 => out.extend_from_slice(&[0x81, be[be.len() - 1]]),
        0x100..0x10000 => out.extend_from_slice(&[0x82, be[be.len() - 2], be[be.len() - 1]]),
        _ => out.extend_from_slice(&[0x83, be[be.len() - 3], be[be.len() - 2], be[be.len() - 1]]),
    }
    out.extend_from_slice(content);
    out
}
