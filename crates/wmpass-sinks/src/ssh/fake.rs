//! A scripted SSH server for tests, built from the client's own packet layer with the
//! directions swapped: curve25519-sha256, an ed25519 host key, password sign-in, and one
//! session channel whose bytes go to a handler. It proves the state machine and the
//! failure mapping; interoperability is what the live tests against OpenSSH are for.

#![allow(clippy::unwrap_used)]

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;

use super::*;

/// Turns the bytes a channel received into the bytes it answers with.
pub(crate) type Handler = Box<dyn FnMut(&[u8]) -> Vec<u8> + Send>;

#[derive(Clone)]
pub(crate) struct Options {
    pub cipher: &'static str,
    pub mac: &'static str,
    pub password: &'static str,
    pub host_seed: [u8; 32],
    pub strict: bool,
    /// The sign-in methods offered.
    pub methods: &'static [&'static str],
    /// Whether the keyboard-interactive prompt shows what is typed.
    pub echo: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            cipher: "aes256-gcm@openssh.com",
            mac: "hmac-sha2-256-etm@openssh.com",
            password: "right",
            host_seed: [7; 32],
            strict: true,
            methods: &["password"],
            echo: false,
        }
    }
}

/// The blob of the host key a seed makes.
pub(crate) fn host_blob(seed: &[u8; 32]) -> Vec<u8> {
    let key = ed25519_dalek::SigningKey::from_bytes(seed);
    Wire::default().text("ssh-ed25519").bytes(&key.verifying_key().to_bytes()).0
}

/// Serves connections one after another on a fresh port, each with a new handler.
pub(crate) fn spawn(options: Options, handler: Arc<dyn Fn() -> Handler + Send + Sync>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { return };
            let _ = serve(stream, &options, handler());
        }
    });
    port
}

fn kind_of(name: &str) -> (CipherKind, Option<(MacKind, bool)>) {
    let cipher = CIPHERS.iter().find(|(n, _)| *n == name).unwrap().1;
    (cipher, None)
}

fn serve(stream: TcpStream, o: &Options, mut handler: Handler) -> Result<()> {
    let io = |_| Fault::Io;
    let mut reader = BufReader::new(stream.try_clone().map_err(io)?);
    let mut writer = stream;
    let v_s = b"SSH-2.0-Fake_1".to_vec();
    writer.write_all(b"a banner line first\r\nSSH-2.0-Fake_1\r\n").map_err(io)?;
    let mut v_c = Vec::new();
    reader.read_until(b'\n', &mut v_c).map_err(io)?;
    while v_c.last().is_some_and(|b| *b == b'\n' || *b == b'\r') {
        v_c.pop();
    }
    let mut send = Direction::plain();
    let mut recv = Direction::plain();
    let i_c = recv.open(&mut reader)?;
    let kex = if o.strict { "curve25519-sha256,kex-strict-s-v00@openssh.com" } else { "curve25519-sha256" };
    let i_s = Wire::msg(KEXINIT)
        .raw(&[0; 16])
        .text(kex)
        .text("ssh-ed25519")
        .text(o.cipher)
        .text(o.cipher)
        .text(o.mac)
        .text(o.mac)
        .text("none")
        .text("none")
        .text("")
        .text("")
        .bool(false)
        .u32(0)
        .0;
    if !o.strict {
        // Allowed before KEXINIT only without strict kex.
        writer.write_all(&send.seal(&[IGNORE, 0, 0, 0, 0])?).map_err(io)?;
    }
    writer.write_all(&send.seal(&i_s)?).map_err(io)?;
    let init = recv.open(&mut reader)?;
    assert_eq!(init[0], KEX_INIT);
    let q_c: [u8; 32] = Parse::new(&init[1..]).bytes()?.try_into().map_err(|_| Fault::Io)?;
    let secret = x25519_dalek::StaticSecret::from([9u8; 32]);
    let public = x25519_dalek::PublicKey::from(&secret);
    let shared = secret.diffie_hellman(&x25519_dalek::PublicKey::from(q_c));
    let k = Wire::default().mpint(shared.as_bytes()).0;
    let signing = ed25519_dalek::SigningKey::from_bytes(&o.host_seed);
    let blob = host_blob(&o.host_seed);
    let h = KexHash::Sha256.digest(&[
        &Wire::default()
            .bytes(&v_c)
            .bytes(&v_s)
            .bytes(&i_c)
            .bytes(&i_s)
            .bytes(&blob)
            .bytes(&q_c)
            .bytes(public.as_bytes())
            .0,
        &k,
    ]);
    use ed25519_dalek::Signer;
    let signature = Wire::default().text("ssh-ed25519").bytes(&signing.sign(&h).to_bytes()).0;
    writer
        .write_all(&send.seal(&Wire::msg(KEX_REPLY).bytes(&blob).bytes(public.as_bytes()).bytes(&signature).0)?)
        .map_err(io)?;
    writer.write_all(&send.seal(&[NEWKEYS])?).map_err(io)?;
    let mut algorithms = kind_of(o.cipher);
    if !algorithms.0.aead() {
        let (_, mac, etm) = MACS.iter().find(|(n, _, _)| *n == o.mac).unwrap();
        algorithms.1 = Some((*mac, *etm));
    }
    let seq = if o.strict { 0 } else { send.seq };
    send = direction(KexHash::Sha256, &k, &h, &h, *b"BDF", algorithms)?;
    send.seq = seq;
    assert_eq!(recv.open(&mut reader)?, vec![NEWKEYS]);
    let seq = if o.strict { 0 } else { recv.seq };
    recv = direction(KexHash::Sha256, &k, &h, &h, *b"ACE", algorithms)?;
    recv.seq = seq;
    let mut out = |send: &mut Direction, payload: &[u8]| -> Result<()> {
        writer.write_all(&send.seal(payload)?).map_err(|_| Fault::Io)
    };
    out(&mut send, &Wire::msg(EXT_INFO).u32(1).text("server-sig-algs").text("ssh-ed25519,rsa-sha2-256").0)?;
    let mut client_channel = 0;
    loop {
        let message = recv.open(&mut reader)?;
        let mut p = Parse::new(&message[1..]);
        match message[0] {
            SERVICE_REQUEST => out(&mut send, &Wire::msg(SERVICE_ACCEPT).text("ssh-userauth").0)?,
            USERAUTH_REQUEST => {
                p.text()?;
                p.text()?;
                let method = p.text()?;
                if method == "keyboard-interactive" && o.methods.contains(&method) {
                    let prompt = Wire::msg(USERAUTH_60).text("").text("").text("").u32(1).text("Password: ");
                    out(&mut send, &prompt.bool(o.echo).0)?;
                    continue;
                }
                let ok = method == "password" && o.methods.contains(&method) && {
                    p.bool()?;
                    p.text()? == o.password
                };
                if ok {
                    out(&mut send, &[USERAUTH_BANNER, 0, 0, 0, 2, b'h', b'i', 0, 0, 0, 0])?;
                    out(&mut send, &[USERAUTH_SUCCESS])?;
                    out(&mut send, &Wire::msg(GLOBAL_REQUEST).text("hostkeys-00@openssh.com").bool(true).0)?;
                } else {
                    out(&mut send, &Wire::msg(USERAUTH_FAILURE).names(o.methods).bool(false).0)?;
                }
            }
            USERAUTH_INFO_RESPONSE => {
                assert_eq!(p.u32()?, 1);
                if p.text()? == o.password {
                    out(&mut send, &[USERAUTH_SUCCESS])?;
                } else {
                    out(&mut send, &Wire::msg(USERAUTH_FAILURE).names(o.methods).bool(false).0)?;
                }
            }
            CHANNEL_OPEN => {
                p.text()?;
                client_channel = p.u32()?;
                // A small window and packet, so the client has to wait for adjustments.
                out(&mut send, &Wire::msg(CHANNEL_OPEN_CONFIRMATION).u32(client_channel).u32(7).u32(40_000).u32(16_384).0)?;
            }
            CHANNEL_REQUEST => {
                p.u32()?;
                assert_eq!(p.text()?, "subsystem");
                out(&mut send, &Wire::msg(CHANNEL_SUCCESS).u32(client_channel).0)?;
                out(&mut send, &Wire::msg(CHANNEL_REQUEST).u32(client_channel).text("keepalive@openssh.com").bool(true).0)?;
            }
            CHANNEL_DATA => {
                p.u32()?;
                let data = p.bytes()?.to_vec();
                out(&mut send, &Wire::msg(CHANNEL_WINDOW_ADJUST).u32(client_channel).u32(data.len() as u32).0)?;
                let answer = handler(&data);
                for chunk in answer.chunks(30_000) {
                    out(&mut send, &Wire::msg(CHANNEL_DATA).u32(client_channel).bytes(chunk).0)?;
                }
            }
            CHANNEL_CLOSE | DISCONNECT => return Ok(()),
            _ => {}
        }
    }
}
