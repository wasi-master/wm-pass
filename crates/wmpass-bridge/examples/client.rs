//! Plays the extension against a running app, through the real relay program. It reports field
//! names and lengths, never values. Usage: client <relay program> <page address> <other address>

use std::io::{Read, Write};
use std::process::{ChildStdin, ChildStdout, Command, Stdio};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use serde_json::{Value, json};
use wmpass_bridge::{Client, Keys, fingerprint};

struct Relay {
    input: ChildStdin,
    output: ChildStdout,
}

impl Relay {
    fn send(&mut self, frame: &[u8]) {
        let body = json!({"f": B64.encode(frame)}).to_string();
        self.input.write_all(&(body.len() as u32).to_ne_bytes()).unwrap();
        self.input.write_all(body.as_bytes()).unwrap();
        self.input.flush().unwrap();
    }

    fn next(&mut self) -> Option<Vec<u8>> {
        let mut length = [0u8; 4];
        self.output.read_exact(&mut length).ok()?;
        let mut body = vec![0u8; u32::from_ne_bytes(length) as usize];
        self.output.read_exact(&mut body).ok()?;
        let value: Value = serde_json::from_slice(&body).ok()?;
        B64.decode(value.get("f")?.as_str()?).ok()
    }
}

/// Connects and runs until the conversation is ready.
fn connect(relay_program: &str, keys: &Keys, pinned: Option<[u8; 32]>) -> Option<(Client, Relay)> {
    let child = Command::new(relay_program).arg("chrome-extension://test/").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    let mut relay = Relay { input: child.stdin.unwrap(), output: child.stdout.unwrap() };
    let (mut client, first) = Client::start(&keys.secret, pinned, "Test client").unwrap();
    relay.send(&first);
    loop {
        let frame = relay.next()?;
        let step = client.feed(&frame).unwrap();
        for out in &step.send {
            relay.send(out);
        }
        if let Some(code) = step.compare {
            println!("compare code length {}", code.len());
        }
        if step.declined {
            println!("declined");
            return None;
        }
        if step.ready {
            return Some((client, relay));
        }
    }
}

fn ask(client: &mut Client, relay: &mut Relay, request: Value) -> Value {
    relay.send(&client.seal(request.to_string().as_bytes()).unwrap());
    let frame = relay.next().unwrap();
    let message = client.feed(&frame).unwrap().message.unwrap();
    serde_json::from_slice(&message).unwrap()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (program, page, other) = (&args[1], &args[2], &args[3]);
    let keys = Keys::generate().unwrap();

    let (mut client, mut relay) = connect(program, &keys, None).expect("first connection");
    let app = client.remote().unwrap();
    println!("paired with app {}", fingerprint(&app).len());
    println!("status {}", ask(&mut client, &mut relay, json!({"id": 1, "method": "status"})));

    let matched = ask(&mut client, &mut relay, json!({"id": 2, "method": "match", "url": page}));
    let rows = matched["entries"].as_array().expect("entries");
    println!("matches {}", rows.len());
    let row = &rows[0]["entry"];
    let fields: Vec<&String> = row.as_object().unwrap().keys().collect();
    println!("row fields {fields:?}");
    let id = row["id"].as_str().unwrap();

    let filled = ask(&mut client, &mut relay, json!({"id": 3, "method": "fill", "url": page, "entry": id}));
    let fill = &filled["fill"];
    println!(
        "fill username {} password {} code {}",
        fill["username"].as_str().map_or(0, str::len),
        fill["password"].as_str().map_or(0, str::len),
        fill["code"].as_str().map_or(0, str::len),
    );

    let wrong = ask(&mut client, &mut relay, json!({"id": 4, "method": "fill", "url": other, "entry": id}));
    println!("fill on another site: {}", wrong["error"]);
    let reading = ask(&mut client, &mut relay, json!({"id": 5, "method": "entry.reveal", "url": page, "entry": id}));
    println!("other method: {}", reading["error"]);
    drop(relay);

    let (mut client, mut relay) = connect(program, &keys, Some(app)).expect("second connection");
    println!("known extension reconnected {}", ask(&mut client, &mut relay, json!({"id": 6, "method": "status"})));

    let thief = Keys::generate().unwrap();
    let known = connect(program, &thief, Some(app));
    println!("unknown key claiming to be paired: {}", known.is_some());
}
