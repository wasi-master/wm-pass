//! The program a browser starts for native messaging. It only moves frames: browser messages on
//! one side, a local socket to the running app on the other. Everything it carries is already
//! sealed between the extension and the app, so it holds no keys and needs none.

use std::io::{self, Read, Write};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use interprocess::local_socket::Stream;
use interprocess::local_socket::traits::Stream as _;
use serde_json::{Value, json};

/// Chrome refuses a message from a host over 1 MiB.
const LARGEST: usize = 1024 * 1024;

fn read_browser(input: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut length = [0u8; 4];
    input.read_exact(&mut length)?;
    let length = u32::from_ne_bytes(length) as usize;
    if length > LARGEST * 4 {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let mut body = vec![0u8; length];
    input.read_exact(&mut body)?;
    let value: Value = serde_json::from_slice(&body)?;
    let frame = value.get("f").and_then(Value::as_str).ok_or(io::ErrorKind::InvalidData)?;
    B64.decode(frame).map_err(|_| io::ErrorKind::InvalidData.into())
}

fn write_browser(output: &mut impl Write, value: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(value)?;
    if body.len() > LARGEST {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let length = u32::try_from(body.len()).map_err(|_| io::ErrorKind::InvalidData)?;
    output.write_all(&length.to_ne_bytes())?;
    output.write_all(&body)?;
    output.flush()
}

fn read_frame(input: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut length = [0u8; 4];
    input.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if length > LARGEST {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let mut body = vec![0u8; length];
    input.read_exact(&mut body)?;
    Ok(body)
}

fn write_frame(output: &mut impl Write, frame: &[u8]) -> io::Result<()> {
    let length = u32::try_from(frame.len()).map_err(|_| io::ErrorKind::InvalidData)?;
    output.write_all(&length.to_le_bytes())?;
    output.write_all(frame)?;
    output.flush()
}

#[cfg(unix)]
fn connect() -> io::Result<Stream> {
    use interprocess::local_socket::{GenericFilePath, ToFsName};
    let name = folder()?.join("bridge.sock").to_fs_name::<GenericFilePath>()?;
    Stream::connect(name)
}

#[cfg(unix)]
fn folder() -> io::Result<std::path::PathBuf> {
    #[cfg(debug_assertions)]
    if let Some(folder) = std::env::var_os("WMPASS_RUN_DIR") {
        return Ok(folder.into());
    }
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
        return Ok(std::path::PathBuf::from(runtime).join("wm-pass"));
    }
    let home = std::env::var_os("HOME").ok_or(io::ErrorKind::NotFound)?;
    Ok(std::path::PathBuf::from(home).join(".wm-pass"))
}

#[cfg(windows)]
fn connect() -> io::Result<Stream> {
    use interprocess::local_socket::{GenericNamespaced, ToNsName};
    Stream::connect("wmpass-bridge".to_ns_name::<GenericNamespaced>()?)
}

fn main() {
    let mut stdout = io::stdout().lock();
    let Ok(socket) = connect() else {
        // The app is closed, or its connection setting is off. The extension carries on alone.
        let _ = write_browser(&mut stdout, &json!({"closed": true}));
        return;
    };
    let (mut from_app, mut to_app) = socket.split();
    let origin = std::env::args().nth(1).unwrap_or_default();
    if write_frame(&mut to_app, origin.as_bytes()).is_err() {
        return;
    }

    std::thread::spawn(move || {
        let mut stdin = io::stdin().lock();
        while let Ok(frame) = read_browser(&mut stdin) {
            if write_frame(&mut to_app, &frame).is_err() {
                break;
            }
        }
        // The browser closed the port, which also ends this program.
        std::process::exit(0);
    });

    while let Ok(frame) = read_frame(&mut from_app) {
        if write_browser(&mut stdout, &json!({"f": B64.encode(&frame)})).is_err() {
            return;
        }
    }
    let _ = write_browser(&mut stdout, &json!({"closed": true}));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_browser_message_becomes_its_frame() {
        let body = serde_json::to_vec(&json!({"f": B64.encode(b"sealed")})).unwrap();
        let mut input = (body.len() as u32).to_ne_bytes().to_vec();
        input.extend(body);
        assert_eq!(read_browser(&mut input.as_slice()).unwrap(), b"sealed");
    }

    #[test]
    fn a_message_without_a_frame_is_refused() {
        let body = serde_json::to_vec(&json!({"x": 1})).unwrap();
        let mut input = (body.len() as u32).to_ne_bytes().to_vec();
        input.extend(body);
        assert!(read_browser(&mut input.as_slice()).is_err());
    }

    #[test]
    fn an_absurd_length_is_refused_before_reading() {
        let input = u32::MAX.to_ne_bytes();
        assert!(read_browser(&mut input.as_slice()).is_err());
        let input = u32::MAX.to_le_bytes();
        assert!(read_frame(&mut input.as_slice()).is_err());
    }

    #[test]
    fn frames_round_trip() {
        let mut wire = Vec::new();
        write_frame(&mut wire, b"abc").unwrap();
        assert_eq!(read_frame(&mut wire.as_slice()).unwrap(), b"abc");
    }
}
