//! The conversation between the browser extension and the desktop app.
//!
//! The two meet through a small relay program the browser starts, and through a socket any local
//! program could connect to. So neither the relay nor the socket is trusted: everything between
//! the ends runs inside a Noise XX session, and a new extension is only accepted after a person
//! has compared a code on both screens.
//!
//! Six digits are short enough for someone in the middle to search for a matching pair if they
//! could pick their keys after seeing the other side's. Each side therefore commits to its random
//! value before seeing the other's, which leaves that attacker one chance in a million.
//!
//! This code does no input or output. It takes frames and says what to send, so the same code
//! runs on threads in the app and inside WebAssembly in the extension.

use serde::{Deserialize, Serialize};
use snow::{Builder, HandshakeState, TransportState};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

const PATTERN: &str = "Noise_XX_25519_ChaChaPoly_BLAKE2s";
const PROLOGUE: &[u8] = b"wmpass/bridge/v1";
const COMMIT_CONTEXT: &str = "wm-pass bridge 2026 commitment";
const CODE_CONTEXT: &str = "wm-pass bridge 2026 comparison code";
/// Noise's own limit on one message.
const NOISE_MESSAGE: usize = 65_535;
/// Noise's own limit on one message, less its tag. A longer one is sealed in several.
pub const LARGEST_MESSAGE: usize = NOISE_MESSAGE - 16;
/// The most a browser takes in one message from its native host: Chrome refuses anything over
/// 1 MiB, and drops the connection with it.
pub const BROWSER_MESSAGE: usize = 1024 * 1024;
/// The longest frame the host can hand the browser: it goes as base64 inside `{"f":"…"}`, which
/// makes it a third longer again, so a frame past about 768 KiB would not fit in a message.
pub const LARGEST_TO_BROWSER: usize = (BROWSER_MESSAGE - r#"{"f":""}"#.len()) / 4 * 3;

/// How long the frame [`Channel::seal`] makes from `plain` bytes is, so a sender can tell before
/// sealing whether it will fit. Sealing moves the channel on, so one sealed and not sent would
/// leave every later frame unreadable.
#[must_use]
pub fn sealed_len(plain: usize) -> usize {
    if plain <= LARGEST_MESSAGE {
        return plain + 16;
    }
    let pieces = plain.div_ceil(LARGEST_MESSAGE - 1);
    plain + pieces * 17
}

/// Why the conversation ended. No variant carries key material.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum BridgeError {
    #[error("the secure connection could not be set up")]
    Handshake,
    #[error("the other side sent something out of order")]
    OutOfOrder,
    #[error("the other side sent something unreadable")]
    Unreadable,
    #[error("the pairing failed a security check")]
    Tampered,
    #[error("this is not the app this extension was connected to")]
    DifferentApp,
    #[error("the message is too large")]
    TooLarge,
    #[error("this system could not supply random numbers")]
    NoRandom,
}

type Result<T> = std::result::Result<T, BridgeError>;

impl From<snow::Error> for BridgeError {
    fn from(_: snow::Error) -> Self {
        Self::Handshake
    }
}

/// One side's long-lived key pair.
pub struct Keys {
    pub public: [u8; 32],
    pub secret: Zeroizing<[u8; 32]>,
}

impl std::fmt::Debug for Keys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Keys").field("public", &self.public).finish_non_exhaustive()
    }
}

impl Keys {
    pub fn generate() -> Result<Self> {
        let params = PATTERN.parse().map_err(|_| BridgeError::Handshake)?;
        let pair = Builder::new(params).generate_keypair()?;
        let public = pair.public.as_slice().try_into().map_err(|_| BridgeError::Handshake)?;
        let secret: [u8; 32] = pair.private.as_slice().try_into().map_err(|_| BridgeError::Handshake)?;
        Ok(Self { public, secret: Zeroizing::new(secret) })
    }
}

/// A short form of a public key, for showing to a person.
pub fn fingerprint(public: &[u8; 32]) -> String {
    let hash = blake3::hash(public);
    let bytes = hash.as_bytes();
    let mut shown = String::new();
    for (i, byte) in bytes.iter().take(10).enumerate() {
        if i > 0 && i % 2 == 0 {
            shown.push(' ');
        }
        shown.push_str(&format!("{byte:02x}"));
    }
    shown
}

fn random32() -> Result<Zeroizing<[u8; 32]>> {
    let mut bytes = Zeroizing::new([0u8; 32]);
    getrandom::fill(bytes.as_mut()).map_err(|_| BridgeError::NoRandom)?;
    Ok(bytes)
}

fn commitment(value: &[u8; 32], session: &[u8; 32]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new_derive_key(COMMIT_CONTEXT);
    hasher.update(session);
    hasher.update(value);
    *hasher.finalize().as_bytes()
}

fn code(session: &[u8; 32], extension_value: &[u8; 32], app_value: &[u8; 32]) -> String {
    let mut hasher = blake3::Hasher::new_derive_key(CODE_CONTEXT);
    hasher.update(session);
    hasher.update(extension_value);
    hasher.update(app_value);
    let hash = hasher.finalize();
    let mut first = [0u8; 8];
    first.copy_from_slice(&hash.as_bytes()[..8]);
    let number = u64::from_le_bytes(first) % 1_000_000;
    format!("{:03} {:03}", number / 1000, number % 1000)
}

/// The session once the handshake is over.
pub struct Channel {
    noise: TransportState,
    remote: [u8; 32],
    hash: [u8; 32],
}

impl std::fmt::Debug for Channel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Channel").field("remote", &self.remote).finish_non_exhaustive()
    }
}

impl Channel {
    fn from(handshake: HandshakeState) -> Result<Self> {
        let remote = handshake.get_remote_static().ok_or(BridgeError::Handshake)?.try_into().map_err(|_| BridgeError::Handshake)?;
        let hash = handshake.get_handshake_hash().try_into().map_err(|_| BridgeError::Handshake)?;
        Ok(Self { noise: handshake.into_transport_mode()?, remote, hash })
    }

    /// One frame for `plain`. Up to [`LARGEST_MESSAGE`] that is one Noise message; anything
    /// longer goes as several back to back, each but the last a full [`NOISE_MESSAGE`] bytes,
    /// and each led by a byte saying whether more follow, so a frame cut short is refused.
    pub fn seal(&mut self, plain: &[u8]) -> Result<Vec<u8>> {
        // Up to the limit is one Noise message, exactly as older builds send it.
        if plain.len() <= LARGEST_MESSAGE {
            return self.seal_one(plain);
        }
        let pieces: Vec<&[u8]> = plain.chunks(LARGEST_MESSAGE - 1).collect();
        let mut out = Vec::with_capacity(pieces.len() * NOISE_MESSAGE);
        for (i, piece) in pieces.iter().enumerate() {
            let mut part = Zeroizing::new(Vec::with_capacity(piece.len() + 1));
            part.push(u8::from(i + 1 < pieces.len()));
            part.extend_from_slice(piece);
            out.extend_from_slice(&self.seal_one(&part)?);
        }
        Ok(out)
    }

    fn seal_one(&mut self, plain: &[u8]) -> Result<Vec<u8>> {
        let mut out = vec![0u8; plain.len() + 16];
        let written = self.noise.write_message(plain, &mut out)?;
        out.truncate(written);
        Ok(out)
    }

    pub fn open(&mut self, frame: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        if frame.len() <= NOISE_MESSAGE {
            let plain = self.open_one(frame)?;
            // A full-size piece that says more follow is a longer frame cut short: a message
            // itself is JSON and never starts with that byte.
            if frame.len() == NOISE_MESSAGE && plain.first() == Some(&1) {
                return Err(BridgeError::Tampered);
            }
            return Ok(plain);
        }
        let pieces: Vec<&[u8]> = frame.chunks(NOISE_MESSAGE).collect();
        let mut out = Zeroizing::new(Vec::with_capacity(frame.len()));
        for (i, piece) in pieces.iter().enumerate() {
            let part = self.open_one(piece)?;
            let (&more, rest) = part.split_first().ok_or(BridgeError::Tampered)?;
            if more != u8::from(i + 1 < pieces.len()) {
                return Err(BridgeError::Tampered);
            }
            out.extend_from_slice(rest);
        }
        Ok(out)
    }

    fn open_one(&mut self, frame: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        let mut out = Zeroizing::new(vec![0u8; frame.len()]);
        let written = self.noise.read_message(frame, &mut out).map_err(|_| BridgeError::Tampered)?;
        out.truncate(written);
        Ok(out)
    }
}

#[derive(Serialize, Deserialize)]
struct Hello {
    name: String,
    want_pair: bool,
}

#[derive(Serialize, Deserialize)]
struct Greeting {
    pair: bool,
}

#[derive(Serialize, Deserialize)]
struct Accepted {
    accepted: bool,
}

/// What one frame from the other side led to.
#[derive(Debug, Default)]
pub struct Step {
    /// Frames to send, in order.
    pub send: Vec<Vec<u8>>,
    /// The code for a person to compare.
    pub compare: Option<String>,
    /// The conversation is ready for requests.
    pub ready: bool,
    /// The other side declined the pairing.
    pub declined: bool,
    /// The other side wants a new pairing, and offers this name for it. App side only.
    pub wants_pairing: Option<String>,
    /// A decrypted message.
    pub message: Option<Zeroizing<Vec<u8>>>,
}

enum ClientStage {
    Handshake(Box<HandshakeState>),
    Greeting(Channel),
    AppValue(Channel, Zeroizing<[u8; 32]>),
    Verdict(Channel),
    Ready(Channel),
    Done,
}

/// The extension's side.
pub struct Client {
    stage: ClientStage,
    pinned: Option<[u8; 32]>,
    name: String,
    remote: Option<[u8; 32]>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client").finish_non_exhaustive()
    }
}

impl Client {
    /// Starts a connection. `pinned` is the app this extension already trusts. Returns the first
    /// frame to send.
    pub fn start(secret: &[u8; 32], pinned: Option<[u8; 32]>, name: &str) -> Result<(Self, Vec<u8>)> {
        let params = PATTERN.parse().map_err(|_| BridgeError::Handshake)?;
        let mut noise = Builder::new(params).prologue(PROLOGUE)?.local_private_key(secret)?.build_initiator()?;
        let mut out = vec![0u8; 128];
        let written = noise.write_message(&[], &mut out)?;
        out.truncate(written);
        Ok((Self { stage: ClientStage::Handshake(Box::new(noise)), pinned, name: name.to_owned(), remote: None }, out))
    }

    /// The app's key, once the handshake has shown it.
    pub fn remote(&self) -> Option<[u8; 32]> {
        self.remote
    }

    pub fn feed(&mut self, frame: &[u8]) -> Result<Step> {
        let mut step = Step::default();
        match std::mem::replace(&mut self.stage, ClientStage::Done) {
            ClientStage::Handshake(mut noise) => {
                let mut scratch = vec![0u8; frame.len()];
                noise.read_message(frame, &mut scratch)?;
                let mut out = vec![0u8; 256];
                let written = noise.write_message(&[], &mut out)?;
                out.truncate(written);
                step.send.push(out);
                let mut channel = Channel::from(*noise)?;
                self.remote = Some(channel.remote);
                if self.pinned.is_some_and(|p| !bool::from(p.ct_eq(&channel.remote))) {
                    return Err(BridgeError::DifferentApp);
                }
                let hello = Hello { name: self.name.clone(), want_pair: self.pinned.is_none() };
                step.send.push(channel.seal(&to_json(&hello)?)?);
                self.stage = ClientStage::Greeting(channel);
            }
            ClientStage::Greeting(mut channel) => {
                let greeting: Greeting = from_json(&channel.open(frame)?)?;
                if greeting.pair {
                    let value = random32()?;
                    step.send.push(channel.seal(&commitment(&value, &channel.hash))?);
                    self.stage = ClientStage::AppValue(channel, value);
                } else if self.pinned.is_some() {
                    step.ready = true;
                    self.stage = ClientStage::Ready(channel);
                } else {
                    // An app that skips the comparison for an extension it should not know.
                    return Err(BridgeError::Tampered);
                }
            }
            ClientStage::AppValue(mut channel, value) => {
                let opened = channel.open(frame)?;
                let app_value: &[u8; 32] = opened.as_slice().try_into().map_err(|_| BridgeError::Unreadable)?;
                step.compare = Some(code(&channel.hash, &value, app_value));
                step.send.push(channel.seal(value.as_ref())?);
                self.stage = ClientStage::Verdict(channel);
            }
            ClientStage::Verdict(mut channel) => {
                let verdict: Accepted = from_json(&channel.open(frame)?)?;
                if verdict.accepted {
                    step.ready = true;
                    self.stage = ClientStage::Ready(channel);
                } else {
                    step.declined = true;
                }
            }
            ClientStage::Ready(mut channel) => {
                step.message = Some(channel.open(frame)?);
                self.stage = ClientStage::Ready(channel);
            }
            ClientStage::Done => return Err(BridgeError::OutOfOrder),
        }
        Ok(step)
    }

    /// Seals one request. Only once ready.
    pub fn seal(&mut self, plain: &[u8]) -> Result<Vec<u8>> {
        match &mut self.stage {
            ClientStage::Ready(channel) => channel.seal(plain),
            _ => Err(BridgeError::OutOfOrder),
        }
    }
}

enum ServerStage {
    Handshake(Box<HandshakeState>),
    Finish(Box<HandshakeState>),
    Hello(Channel),
    Commit(Channel),
    Reveal(Channel, Zeroizing<[u8; 32]>, [u8; 32]),
    Waiting(Channel),
    Ready(Channel),
    Done,
}

/// The app's side.
pub struct Server {
    stage: ServerStage,
    remote: Option<[u8; 32]>,
}

impl std::fmt::Debug for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Server").finish_non_exhaustive()
    }
}

impl Server {
    pub fn start(secret: &[u8; 32]) -> Result<Self> {
        let params = PATTERN.parse().map_err(|_| BridgeError::Handshake)?;
        let noise = Builder::new(params).prologue(PROLOGUE)?.local_private_key(secret)?.build_responder()?;
        Ok(Self { stage: ServerStage::Handshake(Box::new(noise)), remote: None })
    }

    /// The public half of a saved secret.
    pub fn public_of(secret: &[u8; 32]) -> Result<[u8; 32]> {
        Ok(x25519_dalek::PublicKey::from(&x25519_dalek::StaticSecret::from(*secret)).to_bytes())
    }

    /// The extension's key, once the handshake has shown it.
    pub fn remote(&self) -> Option<[u8; 32]> {
        self.remote
    }

    /// Feeds one frame. `known` says whether the extension's key is already paired.
    pub fn feed(&mut self, frame: &[u8], known: impl Fn(&[u8; 32]) -> bool) -> Result<Step> {
        let mut step = Step::default();
        match std::mem::replace(&mut self.stage, ServerStage::Done) {
            ServerStage::Handshake(mut noise) => {
                let mut scratch = vec![0u8; frame.len()];
                noise.read_message(frame, &mut scratch)?;
                let mut out = vec![0u8; 256];
                let written = noise.write_message(&[], &mut out)?;
                out.truncate(written);
                step.send.push(out);
                self.stage = ServerStage::Finish(noise);
            }
            ServerStage::Finish(mut noise) => {
                let mut scratch = vec![0u8; frame.len()];
                noise.read_message(frame, &mut scratch)?;
                let channel = Channel::from(*noise)?;
                self.remote = Some(channel.remote);
                self.stage = ServerStage::Hello(channel);
            }
            ServerStage::Hello(mut channel) => {
                let hello: Hello = from_json(&channel.open(frame)?)?;
                let pair = hello.want_pair || !known(&channel.remote);
                step.send.push(channel.seal(&to_json(&Greeting { pair })?)?);
                if pair {
                    let name = hello.name.chars().filter(|c| !c.is_control()).take(64).collect();
                    step.wants_pairing = Some(name);
                    self.stage = ServerStage::Commit(channel);
                } else {
                    step.ready = true;
                    self.stage = ServerStage::Ready(channel);
                }
            }
            ServerStage::Commit(mut channel) => {
                let opened = channel.open(frame)?;
                let commit: [u8; 32] = opened.as_slice().try_into().map_err(|_| BridgeError::Unreadable)?;
                let value = random32()?;
                step.send.push(channel.seal(value.as_ref())?);
                self.stage = ServerStage::Reveal(channel, value, commit);
            }
            ServerStage::Reveal(mut channel, value, commit) => {
                let opened = channel.open(frame)?;
                let revealed: &[u8; 32] = opened.as_slice().try_into().map_err(|_| BridgeError::Unreadable)?;
                if !bool::from(commitment(revealed, &channel.hash).ct_eq(&commit)) {
                    return Err(BridgeError::Tampered);
                }
                step.compare = Some(code(&channel.hash, revealed, &value));
                self.stage = ServerStage::Waiting(channel);
            }
            ServerStage::Ready(mut channel) => {
                step.message = Some(channel.open(frame)?);
                self.stage = ServerStage::Ready(channel);
            }
            ServerStage::Waiting(_) | ServerStage::Done => return Err(BridgeError::OutOfOrder),
        }
        Ok(step)
    }

    /// What the person said about the code. Returns the frame that tells the extension.
    pub fn answer(&mut self, accepted: bool) -> Result<Vec<u8>> {
        match std::mem::replace(&mut self.stage, ServerStage::Done) {
            ServerStage::Waiting(mut channel) => {
                let frame = channel.seal(&to_json(&Accepted { accepted })?)?;
                if accepted {
                    self.stage = ServerStage::Ready(channel);
                }
                Ok(frame)
            }
            _ => Err(BridgeError::OutOfOrder),
        }
    }

    pub fn seal(&mut self, plain: &[u8]) -> Result<Vec<u8>> {
        match &mut self.stage {
            ServerStage::Ready(channel) => channel.seal(plain),
            _ => Err(BridgeError::OutOfOrder),
        }
    }
}

fn to_json<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(|_| BridgeError::Unreadable)
}

fn from_json<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Result<T> {
    serde_json::from_slice(bytes).map_err(|_| BridgeError::Unreadable)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs frames between the two until nothing is left to send.
    fn pump(client: &mut Client, server: &mut Server, first: Vec<u8>, known: bool) -> (Option<String>, Option<String>, bool) {
        let mut to_server = vec![first];
        let mut to_client: Vec<Vec<u8>> = Vec::new();
        let (mut client_code, mut server_code, mut ready) = (None, None, false);
        while !to_server.is_empty() || !to_client.is_empty() {
            for frame in std::mem::take(&mut to_server) {
                let step = server.feed(&frame, |_| known).unwrap();
                if step.compare.is_some() {
                    server_code = step.compare;
                }
                ready |= step.ready;
                to_client.extend(step.send);
            }
            for frame in std::mem::take(&mut to_client) {
                let step = client.feed(&frame).unwrap();
                if step.compare.is_some() {
                    client_code = step.compare;
                }
                ready |= step.ready;
                to_server.extend(step.send);
            }
        }
        (client_code, server_code, ready)
    }

    #[test]
    fn a_new_extension_compares_a_code_and_then_talks() {
        let (extension, app) = (Keys::generate().unwrap(), Keys::generate().unwrap());
        let (mut client, first) = Client::start(&extension.secret, None, "Firefox").unwrap();
        let mut server = Server::start(&app.secret).unwrap();
        let (a, b, ready) = pump(&mut client, &mut server, first, false);
        assert!(!ready);
        assert_eq!(a, b);
        assert_eq!(a.unwrap().len(), 7);
        assert_eq!(server.remote(), Some(extension.public));
        assert_eq!(client.remote(), Some(app.public));

        let verdict = server.answer(true).unwrap();
        assert!(client.feed(&verdict).unwrap().ready);
        let request = client.seal(b"ping").unwrap();
        let step = server.feed(&request, |_| true).unwrap();
        assert_eq!(step.message.unwrap().as_slice(), b"ping");
        let reply = server.seal(b"pong").unwrap();
        assert_eq!(client.feed(&reply).unwrap().message.unwrap().as_slice(), b"pong");
    }

    #[test]
    fn a_pairing_can_be_declined() {
        let (extension, app) = (Keys::generate().unwrap(), Keys::generate().unwrap());
        let (mut client, first) = Client::start(&extension.secret, None, "Chrome").unwrap();
        let mut server = Server::start(&app.secret).unwrap();
        pump(&mut client, &mut server, first, false);
        let verdict = server.answer(false).unwrap();
        assert!(client.feed(&verdict).unwrap().declined);
        assert!(client.seal(b"x").is_err());
        assert!(server.seal(b"x").is_err());
    }

    #[test]
    fn nothing_flows_before_the_answer() {
        let (extension, app) = (Keys::generate().unwrap(), Keys::generate().unwrap());
        let (mut client, first) = Client::start(&extension.secret, None, "Chrome").unwrap();
        let mut server = Server::start(&app.secret).unwrap();
        pump(&mut client, &mut server, first, false);
        assert_eq!(client.seal(b"x"), Err(BridgeError::OutOfOrder));
        assert_eq!(server.seal(b"x"), Err(BridgeError::OutOfOrder));
    }

    #[test]
    fn a_known_extension_skips_the_comparison() {
        let (extension, app) = (Keys::generate().unwrap(), Keys::generate().unwrap());
        let (mut client, first) = Client::start(&extension.secret, Some(app.public), "Chrome").unwrap();
        let mut server = Server::start(&app.secret).unwrap();
        let (a, b, ready) = pump(&mut client, &mut server, first, true);
        assert!(ready);
        assert_eq!((a, b), (None, None));
        assert!(client.seal(b"x").is_ok());
    }

    #[test]
    fn a_different_app_is_refused() {
        let (extension, app, other) = (Keys::generate().unwrap(), Keys::generate().unwrap(), Keys::generate().unwrap());
        let (mut client, first) = Client::start(&extension.secret, Some(other.public), "Chrome").unwrap();
        let mut server = Server::start(&app.secret).unwrap();
        let step = server.feed(&first, |_| true).unwrap();
        assert_eq!(client.feed(&step.send[0]).unwrap_err(), BridgeError::DifferentApp);
    }

    #[test]
    fn an_app_cannot_skip_the_comparison_for_a_new_extension() {
        let (extension, app) = (Keys::generate().unwrap(), Keys::generate().unwrap());
        let (mut client, first) = Client::start(&extension.secret, None, "Chrome").unwrap();
        let mut server = Server::start(&app.secret).unwrap();
        let reply = server.feed(&first, |_| true).unwrap();
        let answer = client.feed(&reply.send[0]).unwrap();
        server.feed(&answer.send[0], |_| true).unwrap();
        // The app pretends the extension asked to skip, by answering with its own greeting.
        let ServerStage::Hello(channel) = &mut server.stage else { panic!() };
        channel.open(&answer.send[1]).unwrap();
        let forged = channel.seal(&to_json(&Greeting { pair: false }).unwrap()).unwrap();
        assert_eq!(client.feed(&forged).unwrap_err(), BridgeError::Tampered);
    }

    #[test]
    fn a_man_in_the_middle_gets_two_different_codes() {
        let (extension, app, thief) = (Keys::generate().unwrap(), Keys::generate().unwrap(), Keys::generate().unwrap());
        // The extension talks to the thief, and the thief talks to the app.
        let (mut client, first) = Client::start(&extension.secret, None, "Chrome").unwrap();
        let mut fake_app = Server::start(&thief.secret).unwrap();
        let (a, _, _) = pump(&mut client, &mut fake_app, first, false);
        let (mut fake_client, first) = Client::start(&thief.secret, None, "Chrome").unwrap();
        let mut server = Server::start(&app.secret).unwrap();
        let (_, b, _) = pump(&mut fake_client, &mut server, first, false);
        assert_ne!(a.unwrap(), b.unwrap());
    }

    #[test]
    fn a_changed_frame_is_refused() {
        let (extension, app) = (Keys::generate().unwrap(), Keys::generate().unwrap());
        let (mut client, first) = Client::start(&extension.secret, Some(app.public), "Chrome").unwrap();
        let mut server = Server::start(&app.secret).unwrap();
        pump(&mut client, &mut server, first, true);
        let mut request = client.seal(b"fill").unwrap();
        request[0] ^= 1;
        assert_eq!(server.feed(&request, |_| true).unwrap_err(), BridgeError::Tampered);
    }

    #[test]
    fn a_message_past_one_noise_message_crosses_whole() {
        let (extension, app) = (Keys::generate().unwrap(), Keys::generate().unwrap());
        let (mut client, first) = Client::start(&extension.secret, Some(app.public), "Chrome").unwrap();
        let mut server = Server::start(&app.secret).unwrap();
        pump(&mut client, &mut server, first, true);
        for size in [LARGEST_MESSAGE - 1, LARGEST_MESSAGE, LARGEST_MESSAGE + 1, (LARGEST_MESSAGE - 1) * 2, 512 * 1024 + 3] {
            let big: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
            let request = client.seal(&big).unwrap();
            assert_eq!(server.feed(&request, |_| true).unwrap().message.unwrap().as_slice(), big.as_slice(), "{size}");
            let reply = server.seal(&big).unwrap();
            assert_eq!(client.feed(&reply).unwrap().message.unwrap().as_slice(), big.as_slice(), "{size}");
        }
        assert_eq!(client.seal(b"small").unwrap().len(), 5 + 16, "a small message is one Noise message, as before");
        let full = vec![b'{'; LARGEST_MESSAGE];
        assert_eq!(client.seal(&full).unwrap().len(), NOISE_MESSAGE, "the largest single message goes as one, as before");
    }

    #[test]
    fn the_sealed_length_is_known_before_sealing() {
        let (extension, app) = (Keys::generate().unwrap(), Keys::generate().unwrap());
        let (mut client, first) = Client::start(&extension.secret, Some(app.public), "Chrome").unwrap();
        let mut server = Server::start(&app.secret).unwrap();
        pump(&mut client, &mut server, first, true);
        for size in [0, 1, LARGEST_MESSAGE - 1, LARGEST_MESSAGE, LARGEST_MESSAGE + 1, (LARGEST_MESSAGE - 1) * 2, (LARGEST_MESSAGE - 1) * 2 + 1, 700_000] {
            assert_eq!(server.seal(&vec![b'x'; size]).unwrap().len(), sealed_len(size), "{size}");
        }
    }

    #[test]
    fn the_largest_frame_for_the_browser_fits_its_message() {
        use base64::Engine as _;
        let wrapped = |frame: usize| format!(r#"{{"f":"{}"}}"#, base64::engine::general_purpose::STANDARD.encode(vec![0u8; frame])).len();
        assert!(wrapped(LARGEST_TO_BROWSER) <= BROWSER_MESSAGE);
        assert!(wrapped(LARGEST_TO_BROWSER + 1) > BROWSER_MESSAGE);
    }

    #[test]
    fn a_long_frame_cut_short_is_refused() {
        let (extension, app) = (Keys::generate().unwrap(), Keys::generate().unwrap());
        let (mut client, first) = Client::start(&extension.secret, Some(app.public), "Chrome").unwrap();
        let mut server = Server::start(&app.secret).unwrap();
        pump(&mut client, &mut server, first, true);
        let request = client.seal(&vec![b'x'; LARGEST_MESSAGE * 3]).unwrap();
        assert_eq!(server.feed(&request[..NOISE_MESSAGE * 2], |_| true).unwrap_err(), BridgeError::Tampered);
        let (mut client, first) = Client::start(&extension.secret, Some(app.public), "Chrome").unwrap();
        let mut server = Server::start(&app.secret).unwrap();
        pump(&mut client, &mut server, first, true);
        let request = client.seal(&vec![b'x'; LARGEST_MESSAGE * 3]).unwrap();
        assert_eq!(server.feed(&request[..NOISE_MESSAGE], |_| true).unwrap_err(), BridgeError::Tampered);
    }

    #[test]
    fn a_reveal_that_breaks_its_commitment_is_refused() {
        let session = [7u8; 32];
        let value = [1u8; 32];
        let other = [2u8; 32];
        assert_ne!(commitment(&value, &session), commitment(&other, &session));
    }

    #[test]
    fn the_public_half_comes_back_from_the_secret() {
        let keys = Keys::generate().unwrap();
        assert_eq!(Server::public_of(&keys.secret).unwrap(), keys.public);
    }

    #[test]
    fn the_fingerprint_is_short_and_stable() {
        let shown = fingerprint(&[9u8; 32]);
        assert_eq!(shown, fingerprint(&[9u8; 32]));
        assert_eq!(shown.split(' ').count(), 5);
    }
}
