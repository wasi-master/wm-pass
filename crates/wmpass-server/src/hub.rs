//! Live "something changed" pushes over SignalR's WebSocket framing, which official clients
//! expect at `/notifications/hub`. A message says only that a user should sync.

use std::collections::HashMap;
use std::sync::Mutex;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State as Extract};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, post};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::broadcast;

use crate::auth::{authenticate, random_token};
use crate::db::{date, now_ms};
use crate::error::{ApiError, ApiResult};
use crate::State;

const RECORD_SEPARATOR: char = '\u{1e}';

/// What a push asks for, in Bitwarden's numbering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateKind {
    /// Sync everything.
    SyncVault = 5,
    /// This session has ended.
    LogOut = 11,
}

#[derive(Clone, Debug)]
struct Push {
    kind: UpdateKind,
    /// The device that made the change, so it can skip its own echo.
    context: Option<String>,
    user: String,
}

/// Live sockets per user.
#[derive(Debug, Default)]
pub struct Hub {
    channels: Mutex<HashMap<String, broadcast::Sender<Push>>>,
}

impl Hub {
    fn subscribe(&self, user: &str) -> Option<broadcast::Receiver<Push>> {
        let mut channels = self.channels.lock().ok()?;
        Some(channels.entry(user.to_owned()).or_insert_with(|| broadcast::channel(16).0).subscribe())
    }

    /// Tells a user's live devices.
    pub fn push(&self, user: &str, kind: UpdateKind, context: Option<&str>) {
        let Ok(mut channels) = self.channels.lock() else { return };
        if let Some(sender) = channels.get(user) {
            let message = Push { kind, context: context.map(str::to_owned), user: user.to_owned() };
            if sender.send(message).is_err() {
                channels.remove(user);
            }
        }
    }
}

/// Routes.
pub fn routes() -> Router<State> {
    Router::new().route("/notifications/hub/negotiate", post(negotiate)).route("/notifications/hub", any(connect))
}

async fn negotiate() -> ApiResult<Json<Value>> {
    let id = random_token()?;
    Ok(Json(json!({
        "connectionId": id,
        "connectionToken": id,
        "negotiateVersion": 1,
        "availableTransports": [{"transport": "WebSockets", "transferFormats": ["Text", "Binary"]}],
    })))
}

async fn connect(
    Extract(state): Extract<State>,
    Query(query): Query<HashMap<String, String>>,
    upgrade: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    let token = query.get("access_token").ok_or(ApiError::Unauthorised)?;
    let auth = authenticate(&state, token).await?;
    let receiver = state.hub.subscribe(&auth.user.id).ok_or(ApiError::Internal)?;
    let device = auth.device_id.clone();
    Ok(upgrade.on_upgrade(move |socket| serve_socket(socket, receiver, device)).into_response())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Protocol {
    Json,
    MessagePack,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct Update<'a> {
    context_id: Option<&'a str>,
    #[serde(rename = "Type")]
    kind: u32,
    payload: Payload<'a>,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct Payload<'a> {
    user_id: &'a str,
    date: String,
}

/// A SignalR binary frame: a varint length, then the message.
fn frame(mut message: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::with_capacity(message.len() + 5);
    let mut length = message.len();
    loop {
        let byte = u8::try_from(length & 0x7f).unwrap_or(0);
        length >>= 7;
        if length == 0 {
            out.push(byte);
            break;
        }
        out.push(byte | 0x80);
    }
    out.append(&mut message);
    out
}

fn encode_push(push: &Push, protocol: Protocol) -> Option<Message> {
    let update = Update {
        context_id: push.context.as_deref(),
        kind: push.kind as u32,
        payload: Payload { user_id: &push.user, date: date(now_ms()) },
    };
    match protocol {
        Protocol::Json => {
            let body = json!({"type": 1, "target": "ReceiveMessage", "arguments": [update]});
            Some(Message::text(format!("{body}{RECORD_SEPARATOR}")))
        }
        Protocol::MessagePack => {
            // [1 invocation, headers, invocation id, target, arguments]
            let headers: HashMap<String, String> = HashMap::new();
            let body = rmp_serde::to_vec_named(&(1, headers, None::<String>, "ReceiveMessage", vec![update])).ok()?;
            Some(Message::binary(frame(body)))
        }
    }
}

fn encode_ping(protocol: Protocol) -> Message {
    match protocol {
        Protocol::Json => Message::text(format!("{{\"type\":6}}{RECORD_SEPARATOR}")),
        // [6]: a one-element array holding the ping type.
        Protocol::MessagePack => Message::binary(frame(vec![0x91, 0x06])),
    }
}

async fn serve_socket(socket: WebSocket, mut receiver: broadcast::Receiver<Push>, device: String) {
    let (mut sender, mut incoming) = socket.split();
    // The first message names the protocol, and the answer is an empty record.
    let protocol = loop {
        match incoming.next().await {
            Some(Ok(Message::Text(text))) => {
                let hello = text.as_str().trim_end_matches(RECORD_SEPARATOR);
                let parsed: Value = serde_json::from_str(hello).unwrap_or(Value::Null);
                break if parsed.get("protocol").and_then(Value::as_str) == Some("json") { Protocol::Json } else { Protocol::MessagePack };
            }
            Some(Ok(_)) => {}
            _ => return,
        }
    };
    if sender.send(Message::text(format!("{{}}{RECORD_SEPARATOR}"))).await.is_err() {
        return;
    }
    let mut ping = tokio::time::interval(std::time::Duration::from_secs(15));
    loop {
        tokio::select! {
            _ = ping.tick() => {
                if sender.send(encode_ping(protocol)).await.is_err() {
                    return;
                }
            }
            message = incoming.next() => {
                match message {
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return,
                    Some(Ok(_)) => {}
                }
            }
            push = receiver.recv() => {
                match push {
                    Ok(push) => {
                        if let Some(encoded) = encode_push(&push, protocol)
                            && sender.send(encoded).await.is_err()
                        {
                            return;
                        }
                        if push.kind == UpdateKind::LogOut && push.context.as_deref() != Some(device.as_str()) {
                            return;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        }
    }
}
