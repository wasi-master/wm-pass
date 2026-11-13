//! The sink against an in-process Proton that keeps its links in memory and decrypts
//! what the sink writes with the keys it was given, as the real one would hand them to
//! another client.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use wmpass_net::http::fake::FakeHttp;
use wmpass_net::{NetError, NetPolicy, Request, Response};

use super::*;
use crate::http::{Blocking, block_on};

const NOW_MS: u64 = 1_762_000_000_000;
const NOW: u32 = 1_762_000_000;
const VOLUME: &str = "vol1";
const ROOT: &str = "root1";
const STORAGE: &str = "https://storage.example/blocks/";
const SALTED: &str = "saltedkeypassword";
const CODE: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];

struct Node {
    parent: String,
    folder: bool,
    name: String,
    node_key: String,
    node_passphrase: String,
    hash_key: Option<String>,
    content_key_packet: Option<String>,
    active: Option<String>,
    /// Revision id to its blocks: index, storage id, hash.
    revisions: BTreeMap<String, Vec<(i64, usize, String)>>,
    state: i64,
}

#[derive(Default)]
struct State {
    user_key: String,
    address_key: String,
    address_token: String,
    share_key: String,
    share_passphrase: String,
    nodes: BTreeMap<String, Node>,
    /// Plain keys the server learnt by decrypting what clients sent, as another client
    /// would: node id to its unlocked key.
    keys: BTreeMap<String, pgp::Key>,
    storage: Vec<Vec<u8>>,
    pending_tokens: Vec<(usize, Vec<u8>)>,
    counter: usize,
    xattrs: Vec<Value>,
    refreshes: usize,
    /// Pages that ignore where they were asked to start, as a broken server would.
    pages_never_move: bool,
}

#[derive(Clone, Default)]
struct Server(Arc<Mutex<State>>);

fn ok(body: Value) -> Response {
    let mut body = body;
    body["Code"] = json!(1000);
    Response { status: 200, body: body.to_string().into_bytes(), ..Response::default() }
}

fn path_of(url: &str) -> (&str, &str) {
    let rest = url.strip_prefix(&format!("{}/", api::HOST)).unwrap_or(url);
    rest.split_once('?').unwrap_or((rest, ""))
}

impl Server {
    fn new() -> Self {
        let user_key = pgp::generate_key(SALTED, NOW).unwrap();
        let user = pgp::Key::unlock(&user_key, SALTED.as_bytes()).unwrap();
        let token = pgp::new_passphrase().unwrap();
        let address_key = pgp::generate_key(&token, NOW).unwrap();
        let address = pgp::Key::unlock(&address_key, token.as_bytes()).unwrap();
        let address_token = pgp::encrypt(&user, token.as_bytes(), None, false, NOW).unwrap();
        let share_pass = pgp::new_passphrase().unwrap();
        let share_key = pgp::generate_key(&share_pass, NOW).unwrap();
        let share = pgp::Key::unlock(&share_key, share_pass.as_bytes()).unwrap();
        let share_passphrase = pgp::encrypt(&address, share_pass.as_bytes(), None, false, NOW).unwrap();
        let root_pass = pgp::new_passphrase().unwrap();
        let root_key = pgp::generate_key(&root_pass, NOW).unwrap();
        let root = pgp::Key::unlock(&root_key, root_pass.as_bytes()).unwrap();
        let mut state = State {
            user_key,
            address_key,
            address_token,
            share_key,
            share_passphrase,
            ..State::default()
        };
        state.nodes.insert(
            ROOT.into(),
            Node {
                parent: String::new(),
                folder: true,
                name: String::new(),
                node_key: root_key,
                node_passphrase: pgp::encrypt(&share, root_pass.as_bytes(), None, false, NOW).unwrap(),
                hash_key: Some(pgp::encrypt(&root, b"roothashkey", None, false, NOW).unwrap()),
                content_key_packet: None,
                active: None,
                revisions: BTreeMap::new(),
                state: 1,
            },
        );
        state.keys.insert(ROOT.into(), root);
        Self(Arc::new(Mutex::new(state)))
    }

    fn link(state: &State, id: &str) -> Value {
        let node = &state.nodes[id];
        let mut details = json!({
            "Link": {
                "LinkID": id,
                "State": node.state,
                "Type": if node.folder { 1 } else { 2 },
                "Name": node.name,
                "NodeKey": node.node_key,
                "NodePassphrase": node.node_passphrase,
            }
        });
        if node.folder {
            details["Folder"] = json!({ "NodeHashKey": node.hash_key });
        } else {
            details["File"] = json!({
                "ContentKeyPacket": node.content_key_packet,
                "ActiveRevision": node.active.as_ref().map(|r| json!({ "RevisionID": r })),
            });
        }
        details
    }

    /// Learns a new node's key the way another client would: through its parent's.
    fn learn(state: &mut State, id: &str, passphrase_for: &str) {
        let node = &state.nodes[id];
        let parent = state.keys[&node.parent].clone();
        let pass = pgp::decrypt_armored(&node.node_passphrase, &[&parent]).unwrap();
        let key = pgp::Key::unlock(&node.node_key, &pass).unwrap();
        assert_eq!(pgp::decrypt_armored(&node.name, &[&parent]).unwrap(), passphrase_for.as_bytes());
        state.keys.insert(id.to_owned(), key);
    }

    fn handle(&self, request: &Request) -> Response {
        let mut state = self.0.lock().unwrap();
        let body: Value = request.body.as_ref().and_then(|(_, b)| serde_json::from_slice(b).ok()).unwrap_or(Value::Null);
        if let Some(n) = request.url.strip_prefix(STORAGE) {
            let n: usize = n.parse().unwrap();
            return match request.method {
                "POST" => {
                    let raw = &request.body.as_ref().unwrap().1;
                    let start = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
                    let end = raw.len() - "\r\n--wmpass-proton-block-5c1e--\r\n".len();
                    let block = raw[start..end].to_vec();
                    let (_, token) = state.pending_tokens.iter().find(|(i, _)| *i == n).cloned().unwrap();
                    let expected: Vec<u8> = CODE.iter().zip(&block).map(|(c, b)| c ^ b).collect();
                    assert_eq!(token, expected, "verifier token");
                    state.storage[n] = block;
                    ok(json!({}))
                }
                _ => Response { status: 200, body: state.storage[n].clone(), ..Response::default() },
            };
        }
        let (path, query) = path_of(&request.url);
        let parts: Vec<&str> = path.split('/').collect();
        match (request.method, parts.as_slice()) {
            ("GET", ["core", "v4", "users"]) => {
                ok(json!({ "User": { "Keys": [{ "ID": "uk", "Primary": 1, "PrivateKey": state.user_key }] } }))
            }
            ("GET", ["core", "v4", "addresses"]) => ok(json!({ "Addresses": [{
                "ID": "addr", "Email": "me@proton.me",
                "Keys": [{ "Primary": 1, "PrivateKey": state.address_key, "Token": state.address_token }],
            }] })),
            ("GET", ["drive", "v2", "shares", "my-files"]) => ok(json!({
                "Volume": { "VolumeID": VOLUME },
                "Share": { "AddressID": "addr", "Key": state.share_key, "Passphrase": state.share_passphrase, "CreatorEmail": "me@proton.me" },
                "Link": Self::link(&state, ROOT),
            })),
            ("GET", ["drive", "v2", "volumes", _, "folders", folder, "children"]) => {
                let ids: Vec<&String> = state.nodes.iter().filter(|(_, n)| n.parent == *folder).map(|(id, _)| id).collect();
                if state.pages_never_move {
                    return ok(json!({ "LinkIDs": ids, "More": true, "AnchorID": "same" }));
                }
                ok(json!({ "LinkIDs": ids, "More": false }))
            }
            ("POST", ["drive", "v2", "volumes", _, "links"]) => {
                let links: Vec<Value> = body["LinkIDs"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|id| id.as_str())
                    .filter(|id| state.nodes.contains_key(*id))
                    .map(|id| Self::link(&state, id))
                    .collect();
                ok(json!({ "Links": links }))
            }
            ("POST", ["drive", "v2", "volumes", _, "folders"]) => {
                state.counter += 1;
                let id = format!("folder{}", state.counter);
                let parent = body["ParentLinkID"].as_str().unwrap().to_owned();
                state.nodes.insert(
                    id.clone(),
                    Node {
                        parent,
                        folder: true,
                        name: body["Name"].as_str().unwrap().into(),
                        node_key: body["NodeKey"].as_str().unwrap().into(),
                        node_passphrase: body["NodePassphrase"].as_str().unwrap().into(),
                        hash_key: Some(body["NodeHashKey"].as_str().unwrap().into()),
                        content_key_packet: None,
                        active: None,
                        revisions: BTreeMap::new(),
                        state: 1,
                    },
                );
                Self::learn(&mut state, &id, "Backups");
                ok(json!({ "Folder": { "ID": id } }))
            }
            ("POST", ["drive", "v2", "volumes", _, "files"]) => {
                state.counter += 1;
                let id = format!("file{}", state.counter);
                let revision = format!("rev{}", state.counter);
                let parent = body["ParentLinkID"].as_str().unwrap().to_owned();
                // The name hash is what finds a name again without decrypting it.
                let parent_key = state.keys[&parent].clone();
                let hash_key = pgp::decrypt_armored(state.nodes[&parent].hash_key.as_ref().unwrap(), &[&parent_key]).unwrap();
                let name = pgp::decrypt_armored(body["Name"].as_str().unwrap(), &[&parent_key]).unwrap();
                assert_eq!(body["Hash"].as_str().unwrap(), pgp::name_hash(&hash_key, std::str::from_utf8(&name).unwrap()));
                state.nodes.insert(
                    id.clone(),
                    Node {
                        parent,
                        folder: false,
                        name: body["Name"].as_str().unwrap().into(),
                        node_key: body["NodeKey"].as_str().unwrap().into(),
                        node_passphrase: body["NodePassphrase"].as_str().unwrap().into(),
                        hash_key: None,
                        content_key_packet: Some(body["ContentKeyPacket"].as_str().unwrap().into()),
                        active: None,
                        revisions: BTreeMap::from([(revision.clone(), Vec::new())]),
                        state: 1,
                    },
                );
                let name = String::from_utf8(name).unwrap();
                Self::learn(&mut state, &id, &name);
                ok(json!({ "File": { "ID": id, "RevisionID": revision } }))
            }
            ("POST", ["drive", "v2", "volumes", _, "files", file, "revisions"]) => {
                state.counter += 1;
                let revision = format!("rev{}", state.counter);
                let node = state.nodes.get_mut(*file).unwrap();
                assert_eq!(body["CurrentRevisionID"].as_str(), node.active.as_deref());
                node.revisions.insert(revision.clone(), Vec::new());
                ok(json!({ "Revision": { "ID": revision } }))
            }
            ("GET", ["drive", "v2", "volumes", _, "links", _, "revisions", _, "verification"]) => {
                ok(json!({ "VerificationCode": b64e(&CODE) }))
            }
            ("POST", ["drive", "blocks"]) => {
                let file = body["LinkID"].as_str().unwrap().to_owned();
                let revision = body["RevisionID"].as_str().unwrap().to_owned();
                let mut links = Vec::new();
                for block in body["BlockList"].as_array().unwrap() {
                    let index = block["Index"].as_i64().unwrap();
                    let n = state.storage.len();
                    state.storage.push(Vec::new());
                    state.pending_tokens.push((n, b64(block["Verifier"]["Token"].as_str().unwrap()).unwrap()));
                    state.nodes.get_mut(&file).unwrap().revisions.get_mut(&revision).unwrap().push((index, n, String::new()));
                    links.push(json!({ "Index": index, "BareURL": format!("{STORAGE}{n}"), "Token": "t" }));
                }
                ok(json!({ "UploadLinks": links }))
            }
            ("PUT", ["drive", "v2", "volumes", _, "files", file, "revisions", revision]) => {
                let key = state.keys[*file].clone();
                let xattr = pgp::decrypt_armored(body["XAttr"].as_str().unwrap(), &[&key]).unwrap();
                state.xattrs.push(serde_json::from_slice(&xattr).unwrap());
                let storage = state.storage.clone();
                let node = state.nodes.get_mut(*file).unwrap();
                for (_, n, hash) in node.revisions.get_mut(*revision).unwrap() {
                    *hash = b64e(&Sha256::digest(&storage[*n]));
                }
                node.active = Some((*revision).to_owned());
                ok(json!({}))
            }
            ("GET", ["drive", "v2", "volumes", _, "files", file, "revisions", revision]) => {
                let from: i64 = query.split('&').find_map(|q| q.strip_prefix("FromBlockIndex=")).unwrap().parse().unwrap();
                let blocks: Vec<Value> = state.nodes[*file].revisions[*revision]
                    .iter()
                    .filter(|(index, _, _)| *index >= from)
                    .map(|(index, n, hash)| json!({ "Index": index, "BareURL": format!("{STORAGE}{n}"), "Token": "t", "Hash": hash }))
                    .collect();
                if state.pages_never_move {
                    // A full page of the first block, wherever it was asked to start.
                    let first = state.nodes[*file].revisions[*revision][0].clone();
                    let (index, n, hash) = first;
                    let block = json!({ "Index": index, "BareURL": format!("{STORAGE}{n}"), "Token": "t", "Hash": hash });
                    return ok(json!({ "Revision": { "Blocks": vec![block; BLOCK_PAGE] } }));
                }
                ok(json!({ "Revision": { "Blocks": blocks } }))
            }
            ("POST", ["drive", "v2", "volumes", _, "trash_multiple"]) => {
                for id in body["LinkIDs"].as_array().unwrap() {
                    if let Some(node) = state.nodes.get_mut(id.as_str().unwrap()) {
                        node.state = 2;
                    }
                }
                ok(json!({}))
            }
            ("POST", ["drive", "v2", "volumes", _, "trash", "delete_multiple"]) => {
                for id in body["LinkIDs"].as_array().unwrap() {
                    state.nodes.remove(id.as_str().unwrap());
                }
                ok(json!({}))
            }
            ("POST", ["auth", "v4", "refresh"]) => {
                state.refreshes += 1;
                ok(json!({ "AccessToken": "acc2", "RefreshToken": "ref2" }))
            }
            _ => Response { status: 404, body: br#"{"Code":2501}"#.to_vec(), ..Response::default() },
        }
    }
}

impl AsyncHttp for Server {
    async fn send(&self, request: &Request) -> std::result::Result<Response, NetError> {
        Ok(self.handle(request))
    }
}

fn config() -> ProtonConfig {
    ProtonConfig {
        user: "me".into(),
        uid: "uid".into(),
        access_token: "acc".into(),
        refresh_token: "ref".into(),
        salted_key_pass: SALTED.into(),
        folder: "Backups".into(),
    }
}

fn sink(server: &Server) -> Proton<Server> {
    Proton::new(Client::new(server.clone(), NetPolicy::Any), config(), NOW_MS, Box::new(|_| {}))
}

#[test]
fn a_file_goes_up_encrypted_comes_back_and_is_replaced_as_a_new_revision() {
    let server = Server::new();
    let proton = sink(&server);
    block_on(proton.readiness()).unwrap();
    let data: Vec<u8> = (0..BLOCK_SIZE + 1000).map(|i| (i % 253) as u8).collect();
    let stored = block_on(proton.write("vault.wmpass", &data)).unwrap();
    assert_eq!(stored.size, data.len() as u64);
    {
        let state = server.0.lock().unwrap();
        // Two blocks, neither holding the plain bytes.
        let node = &state.nodes[&stored.id];
        assert_eq!(node.revisions.values().next().unwrap().len(), 2);
        assert!(state.storage.iter().all(|b| !b.windows(64).any(|w| w == &data[1000..1064])));
        let common = &state.xattrs[0]["Common"];
        assert_eq!(common["Size"], json!(data.len()));
        assert_eq!(common["BlockSizes"], json!([BLOCK_SIZE, 1000]));
        assert_eq!(common["ModificationTime"], json!("2025-11-01T12:26:40+0000"));
        assert_eq!(common["Digests"]["SHA1"], json!(hex(&Sha1::digest(&data))));
        // The folder was made under My files, named in the clear only to its key holders.
        assert!(state.nodes.values().any(|n| n.folder && n.parent == ROOT));
    }
    let listed = block_on(proton.list()).unwrap();
    assert_eq!(listed, vec![Stored { id: stored.id.clone(), name: "vault.wmpass".into(), size: 0 }]);
    assert_eq!(block_on(proton.read(&listed[0])).unwrap(), data);

    // A second write of the same name is a revision under the same keys.
    block_on(proton.write("vault.wmpass", b"newer")).unwrap();
    let again = sink(&server);
    let listed = block_on(again.list()).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(block_on(again.read(&listed[0])).unwrap(), b"newer");

    block_on(again.delete(&listed[0])).unwrap();
    assert!(block_on(again.list()).unwrap().is_empty());
    assert_eq!(block_on(again.read(&listed[0])), Err(Fault::TargetMissing));
}

#[test]
fn pages_that_never_move_end_rather_than_repeat() {
    let server = Server::new();
    let proton = sink(&server);
    block_on(proton.readiness()).unwrap();
    let stored = block_on(proton.write("small.bin", b"one block")).unwrap();
    server.0.lock().unwrap().pages_never_move = true;
    let listed = block_on(proton.list()).unwrap();
    assert_eq!(listed.len(), 1, "the children were asked for once more, then no further");
    assert_eq!(block_on(proton.read(&stored)), Err(Fault::Io));
}

#[test]
fn an_empty_file_has_no_blocks() {
    let server = Server::new();
    let proton = sink(&server);
    let stored = block_on(proton.write("empty", b"")).unwrap();
    assert_eq!(block_on(proton.read(&stored)).unwrap(), b"");
}

#[test]
fn a_wrong_key_password_is_a_lost_permission_and_nothing_is_configured_without_a_session() {
    let server = Server::new();
    let mut wrong = config();
    wrong.salted_key_pass = "nope".into();
    let proton = Proton::new(Client::new(server.clone(), NetPolicy::Any), wrong, NOW_MS, Box::new(|_| {}));
    assert_eq!(block_on(proton.readiness()), Err(Fault::PermissionLost));
    let proton = Proton::new(Client::new(server, NetPolicy::Any), ProtonConfig::default(), NOW_MS, Box::new(|_| {}));
    assert_eq!(block_on(proton.readiness()), Err(Fault::NotConfigured));
    assert_eq!(ProtonConfig::default().folder, ProtonConfig::DEFAULT_FOLDER);
}

#[test]
fn an_expired_token_is_refreshed_once_and_the_new_pair_is_stored() {
    let http = FakeHttp::new()
        .answer(&format!("{}/auth/v4/refresh", api::HOST), 200, r#"{"Code":1000,"AccessToken":"acc2","RefreshToken":"ref2"}"#)
        .answer(&format!("{}/core/v4/users", api::HOST), 401, r#"{"Code":401}"#);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let saved = seen.clone();
    let proton = Proton::new(
        Client::new(Blocking(&http), NetPolicy::Any),
        config(),
        NOW_MS,
        Box::new(move |tokens: &ProtonTokens| saved.lock().unwrap().push((tokens.access.expose().to_owned(), tokens.refresh.expose().to_owned()))),
    );
    assert_eq!(block_on(proton.readiness()), Err(Fault::PermissionLost));
    assert_eq!(*seen.lock().unwrap(), vec![("acc2".to_owned(), "ref2".to_owned())]);
    let asked = http.asked.lock().unwrap();
    assert_eq!(asked.len(), 3);
    let auth = |r: &Request| r.headers.iter().find(|(k, _)| *k == "Authorization").map(|(_, v)| v.clone());
    assert_eq!(auth(&asked[0]).as_deref(), Some("Bearer acc"));
    assert_eq!(auth(&asked[1]), None, "the refresh goes without the old bearer");
    assert_eq!(auth(&asked[2]).as_deref(), Some("Bearer acc2"));
    assert!(asked.iter().all(|r| r.headers.iter().any(|(k, v)| *k == "x-pm-appversion" && v == APP_VERSION)));
}

#[test]
fn a_refused_refresh_is_a_lost_permission() {
    let http = FakeHttp::new()
        .answer(&format!("{}/auth/v4/refresh", api::HOST), 400, r#"{"Code":10013}"#)
        .answer(&format!("{}/core/v4/users", api::HOST), 401, r#"{"Code":401}"#);
    let proton = Proton::new(Client::new(Blocking(&http), NetPolicy::Any), config(), NOW_MS, Box::new(|_| panic!("no tokens")));
    assert_eq!(block_on(proton.readiness()), Err(Fault::PermissionLost));
}

#[test]
fn failures_mean_the_right_thing() {
    assert_eq!(status_error(422, 200_001, ""), Fault::OutOfSpace);
    assert_eq!(status_error(422, 2003, ""), Fault::OutOfSpace);
    assert_eq!(status_error(422, 0, "Storage quota exceeded"), Fault::OutOfSpace);
    assert_eq!(status_error(401, 0, ""), Fault::PermissionLost);
    assert_eq!(status_error(422, 8002, "Incorrect login credentials"), Fault::PermissionLost);
    assert_eq!(status_error(422, 2501, ""), Fault::TargetMissing);
    assert_eq!(status_error(500, 0, ""), Fault::Io);
}

#[test]
fn config_debug_hides_the_session() {
    let printed = format!("{:?}", config());
    assert!(!printed.contains("acc") && !printed.contains(SALTED) && !printed.contains("ref"), "{printed}");
}

/// A Proton sign-in server for go-srp's test account: modulus, salt and ephemeral from
/// go-srp's `TestSRPauth`, with its published server proof.
mod sign_in {
    use super::*;
    use crate::proton::login::ProtonOutcome;

    const MODULUS: &str = "W2z5HBi8RvsfYzZTS7qBaUxxPhsfHJFZpu3Kd6s1JafNrCCH9rfvPLrfuqocxWPgWDH2R8neK7PkNvjxto9TStuY5z7jAzWRvFWN9cQhAKkdWgy0JY6ywVn22+HFpF4cYesHrqFIKUPDMSSIlWjBVmEJZ/MusD44ZT29xcPrOqeZvwtCffKtGAIjLYPZIEbZKnDM1Dm3q2K/xS5h+xdhjnndhsrkwm9U9oyA2wxzSXFL+pdfj2fOdRwuR5nW0J2NFrq3kJjkRmpO/Genq1UW+TEknIWAb6VzJJJA244K/H8cnSx2+nSNZO3bbo6Ys228ruV9A8m6DhxmS+bihN3ttQ==";

    fn info() -> String {
        json!({
            "Code": 1000,
            "Version": 4,
            "Modulus": format!("-----BEGIN PGP SIGNED MESSAGE-----\nHash: SHA256\n\n{MODULUS}\n-----BEGIN PGP SIGNATURE-----\n\nx\n-----END PGP SIGNATURE-----\n"),
            "Salt": "yKlc5/CvObfoiw==",
            "ServerEphemeral": "l13IQSVFBEV0ZZREuRQ4ZgP6OpGiIfIjbSDYQG3Yp39FkT2B/k3n1ZhwqrAdy+qvPPFq/le0b7UDtayoX4aOTJihoRvifas8Hr3icd9nAHqd0TUBbkZkT6Iy6UpzmirCXQtEhvGQIdOLuwvy+vZWh24G2ahBM75dAqwkP961EJMh67/I5PA5hJdQZjdPT5luCyVa7BS1d9ZdmuR0/VCjUOdJbYjgtIH7BQoZs+KacjhUN8gybu+fsycvTK3eC+9mCN2Y6GdsuCMuR3pFB0RF9eKae7cA6RbJfF1bjm0nNfWLXzgKguKBOeF3GEAsnCgK68q82/pq9etiUDizUlUBcA==",
            "SRPSession": "sess",
        })
        .to_string()
    }

    fn url(path: &str) -> String {
        format!("{}/{path}", api::HOST)
    }

    #[test]
    fn a_server_that_cannot_prove_the_verifier_is_refused() {
        // A random client secret makes a proof the canned server proof cannot match.
        let http = FakeHttp::new()
            .answer(&url("auth/v4/info"), 200, &info())
            .answer(&url("auth/v4"), 200, r#"{"Code":1000,"ServerProof":"AAAA","UID":"u","AccessToken":"a","RefreshToken":"r"}"#);
        let client = Client::new(Blocking(&http), NetPolicy::Any);
        let outcome = block_on(sign_in(&client, "jakubqa", &"abc123".into(), &Secret::default()));
        assert!(matches!(outcome, ProtonOutcome::Failed(Fault::Unsafe)), "{outcome:?}");
        let sent: Value = serde_json::from_slice(&http.last().unwrap().body.unwrap().1).unwrap();
        assert_eq!(sent["SRPSession"], "sess");
        assert_eq!(b64(sent["ClientProof"].as_str().unwrap()).unwrap().len(), 256);
    }

    #[test]
    fn a_captcha_and_an_old_auth_version_stop_the_sign_in() {
        let http = FakeHttp::new().answer(&url("auth/v4/info"), 422, r#"{"Code":9001,"Error":"Human verification required"}"#);
        let client = Client::new(Blocking(&http), NetPolicy::Any);
        let outcome = block_on(sign_in(&client, "me", &"pw".into(), &Secret::default()));
        assert!(matches!(outcome, ProtonOutcome::NeedsHumanVerification), "{outcome:?}");
        let http = FakeHttp::new().answer(&url("auth/v4/info"), 200, r#"{"Code":1000,"Version":2}"#);
        let client = Client::new(Blocking(&http), NetPolicy::Any);
        let outcome = block_on(sign_in(&client, "me", &"pw".into(), &Secret::default()));
        assert!(matches!(outcome, ProtonOutcome::Failed(Fault::Io)), "{outcome:?}");
    }

    /// After the code: the key salt turns the password into the key password, which must
    /// open the primary user key.
    #[test]
    fn the_code_then_the_key_salt_finish_the_sign_in() {
        let salt = [9u8; 16];
        let salted = srp::salted_key_pass(b"pw", &salt);
        let user_key = pgp::generate_key(&salted, NOW).unwrap();
        let users = json!({ "Code": 1000, "User": { "Keys": [{ "ID": "k1", "Primary": 1, "PrivateKey": user_key }] } }).to_string();
        let salts = json!({ "Code": 1000, "KeySalts": [{ "ID": "k1", "KeySalt": b64e(&salt) }] }).to_string();
        let http = FakeHttp::new()
            .answer(&url("auth/v4/2fa"), 200, r#"{"Code":1000}"#)
            .answer(&url("core/v4/users"), 200, &users)
            .answer(&url("core/v4/keys/salts"), 200, &salts);
        let client = Client::new(Blocking(&http), NetPolicy::Any);
        let pending = ProtonPending::for_tests("u", "a", "r", "pw");
        let ProtonOutcome::SignedIn(done) = block_on(submit_code(&client, pending, "123 456")) else { panic!("not signed in") };
        assert_eq!(done.salted_key_pass.expose(), salted);
        assert_eq!((done.uid.as_str(), done.access_token.expose(), done.refresh_token.expose()), ("u", "a", "r"));
        let code: Value = serde_json::from_slice(&http.asked.lock().unwrap()[0].body.as_ref().unwrap().1).unwrap();
        assert_eq!(code["TwoFactorCode"], "123456");

        let pending = ProtonPending::for_tests("u", "a", "r", "wrong");
        let outcome = block_on(submit_code(&client, pending, "1"));
        assert!(matches!(outcome, ProtonOutcome::Failed(Fault::PermissionLost)), "{outcome:?}");
    }
}
