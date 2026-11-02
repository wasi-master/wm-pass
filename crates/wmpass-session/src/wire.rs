//! One call as one run of bytes, for shells whose bridge carries bytes.
//!
//! JSON cannot carry a secret here: the JavaScript strings it would become cannot be wiped, and
//! neither can the text of the message itself. So a call travels as a frame the caller can fill
//! with zeros afterwards, and the secret stays in its own run of bytes on both sides.
//!
//! All lengths are little-endian `u32`.
//!
//! ```text
//! request: head_len head secret_len secret blob_count { blob_len blob }*
//! reply:   json_len json has_secret [secret_len secret] blob_count { blob_len blob }*
//! ```
//!
//! `head` is JSON: `{"method": "...", "handle": 0, "args": {...}, "now_ms": 0}`. A failed call
//! replies with `{"error": {"code": "...", "message": "..."}}` as its JSON.

use serde::Deserialize;
use serde_json::{Value, json};
use zeroize::Zeroizing;

use crate::{Call, Reply, Service, SessionError};

/// The most a frame may say it holds, so a damaged one fails rather than asks for everything.
const MAX_RUN: usize = 1 << 31;

/// A request taken apart. Everything borrows the frame, so wiping the frame wipes it all.
#[derive(Debug)]
pub struct Request<'a> {
    /// What to run.
    pub method: String,
    /// The vault it runs on, or zero.
    pub handle: u64,
    /// The arguments, as JSON text.
    pub args: String,
    /// The clock, in milliseconds.
    pub now_ms: u64,
    /// Whether this call may use the network.
    pub online: bool,
    /// The secret going in.
    pub secret: &'a [u8],
    /// Files going in.
    pub blobs: Vec<&'a [u8]>,
}

fn bad() -> SessionError {
    SessionError {
        code: "invalid",
        message: "that call could not be read".to_owned(),
    }
}

fn take_len(bytes: &mut &[u8]) -> Result<usize, SessionError> {
    let (head, rest) = bytes.split_first_chunk::<4>().ok_or_else(bad)?;
    *bytes = rest;
    let length = u32::from_le_bytes(*head) as usize;
    if length > MAX_RUN {
        return Err(bad());
    }
    Ok(length)
}

fn take_run<'a>(bytes: &mut &'a [u8]) -> Result<&'a [u8], SessionError> {
    let length = take_len(bytes)?;
    if length > bytes.len() {
        return Err(bad());
    }
    let (run, rest) = bytes.split_at(length);
    *bytes = rest;
    Ok(run)
}

/// Takes a request frame apart.
pub fn parse_request(frame: &[u8]) -> Result<Request<'_>, SessionError> {
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct Head {
        method: String,
        handle: u64,
        args: Value,
        now_ms: u64,
        online: bool,
    }
    let mut rest = frame;
    let head: Head = serde_json::from_slice(take_run(&mut rest)?).map_err(|_| bad())?;
    let secret = take_run(&mut rest)?;
    let count = take_len(&mut rest)?;
    // Four bytes at least per blob, so a lying count cannot reserve anything.
    if count > rest.len() / 4 {
        return Err(bad());
    }
    let mut blobs = Vec::with_capacity(count);
    for _ in 0..count {
        blobs.push(take_run(&mut rest)?);
    }
    if !rest.is_empty() {
        return Err(bad());
    }
    Ok(Request {
        method: head.method,
        handle: head.handle,
        args: if head.args.is_null() { "{}".to_owned() } else { head.args.to_string() },
        now_ms: head.now_ms,
        online: head.online,
        secret,
        blobs,
    })
}

fn push_run(out: &mut Vec<u8>, run: &[u8]) -> Result<(), SessionError> {
    let length = u32::try_from(run.len()).map_err(|_| bad())?;
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(run);
    Ok(())
}

/// Builds a reply frame. The result holds any secret, so it comes wrapped for wiping.
pub fn reply_frame(result: &Result<Reply, SessionError>) -> Zeroizing<Vec<u8>> {
    let build = |reply: &Reply| -> Result<Zeroizing<Vec<u8>>, SessionError> {
        let secret = reply.secret.as_ref().map_or(0, |s| s.len());
        let blobs: usize = reply.blobs.iter().map(Vec::len).sum();
        let mut out = Zeroizing::new(Vec::with_capacity(reply.json.len() + secret + blobs + 32));
        push_run(&mut out, reply.json.as_bytes())?;
        match &reply.secret {
            Some(value) => {
                out.push(1);
                push_run(&mut out, value)?;
            }
            None => out.push(0),
        }
        let count = u32::try_from(reply.blobs.len()).map_err(|_| bad())?;
        out.extend_from_slice(&count.to_le_bytes());
        for blob in &reply.blobs {
            push_run(&mut out, blob)?;
        }
        Ok(out)
    };
    let failed = |error: &SessionError| -> Zeroizing<Vec<u8>> {
        let text = json!({"error": {"code": error.code, "message": error.message}}).to_string();
        let mut out = Zeroizing::new(Vec::with_capacity(text.len() + 9));
        let length = u32::try_from(text.len()).unwrap_or(0);
        out.extend_from_slice(&length.to_le_bytes());
        out.extend_from_slice(text.as_bytes());
        out.push(0);
        out.extend_from_slice(&0u32.to_le_bytes());
        out
    };
    match result {
        Ok(reply) => build(reply).unwrap_or_else(|error| failed(&error)),
        Err(error) => failed(error),
    }
}

/// Runs a request frame against a service, for calls with no network.
pub fn dispatch(service: &mut Service, frame: &[u8]) -> Zeroizing<Vec<u8>> {
    let result = parse_request(frame).and_then(|request| {
        service.call(
            &request.method,
            Call {
                handle: request.handle,
                args: &request.args,
                secret: request.secret,
                blobs: &request.blobs,
                now_ms: request.now_ms,
            },
        )
    });
    reply_frame(&result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(method: &str, args: &Value, secret: &[u8], blobs: &[&[u8]]) -> Vec<u8> {
        let head = json!({"method": method, "handle": 0, "args": args, "now_ms": 1_700_000_000_000u64}).to_string();
        let mut out = Vec::new();
        push_run(&mut out, head.as_bytes()).expect("fits");
        push_run(&mut out, secret).expect("fits");
        out.extend_from_slice(&u32::try_from(blobs.len()).expect("fits").to_le_bytes());
        for blob in blobs {
            push_run(&mut out, blob).expect("fits");
        }
        out
    }

    #[test]
    fn a_request_comes_apart_as_it_was_put_together() {
        let frame = request("x.y", &json!({"a": 1}), b"secret", &[b"one", b"", b"three"]);
        let parsed = parse_request(&frame).expect("reads");
        assert_eq!(parsed.method, "x.y");
        assert_eq!(parsed.args, r#"{"a":1}"#);
        assert_eq!(parsed.secret, b"secret");
        assert_eq!(parsed.blobs, vec![&b"one"[..], b"", b"three"]);
    }

    #[test]
    fn damage_is_refused() {
        let frame = request("x.y", &json!({}), b"secret", &[b"one"]);
        for cut in 0..frame.len() {
            assert!(parse_request(&frame[..cut]).is_err(), "cut at {cut}");
        }
        let mut longer = frame.clone();
        longer.push(0);
        assert!(parse_request(&longer).is_err());
        let mut lying = request("x.y", &json!({}), b"", &[]);
        let at = lying.len() - 4;
        lying[at..].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse_request(&lying).is_err());
    }

    #[test]
    fn a_call_runs_through_frames() {
        let mut service = Service::new(7);
        let reply = dispatch(&mut service, &request("gen.generate", &json!({"Pin": {"length": 6}}), b"", &[]));
        let mut rest: &[u8] = &reply;
        let view: Value = serde_json::from_slice(take_run(&mut rest).expect("json")).expect("view");
        assert!(view.get("error").is_none(), "{view}");
        let (flag, after) = rest.split_first().expect("flag");
        assert_eq!(*flag, 1);
        rest = after;
        assert!(!take_run(&mut rest).expect("secret").is_empty());
        assert_eq!(take_len(&mut rest).expect("count"), 0);
    }

    #[test]
    fn a_failed_call_says_why_in_the_same_frame() {
        let mut service = Service::new(7);
        let reply = dispatch(&mut service, &request("entry.list", &json!({}), b"", &[]));
        let mut rest: &[u8] = &reply;
        let view: Value = serde_json::from_slice(take_run(&mut rest).expect("json")).expect("view");
        assert_eq!(view["error"]["code"], "locked");
    }
}
