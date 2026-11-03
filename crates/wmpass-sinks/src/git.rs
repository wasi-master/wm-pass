//! A folder in a Git repository, through the host's REST API: GitHub (and GitHub
//! Enterprise), GitLab, and Gitea or Forgejo, which Codeberg runs.
//!
//! No Git client and no clone. Each verb is one or two HTTPS calls, and each write or
//! delete is a commit the host makes on the branch. The history keeps every file ever
//! written, deleted or not, which is why [`GitConfig::allow_public`] is off by default and
//! a public repository is refused with [`Fault::Unsafe`].
//!
//! There is no temporary name: the host publishes a file in the same commit that writes
//! it, so a failed upload leaves nothing. The routines still read a file back before
//! rotating.

use std::sync::Mutex;

use base64::Engine;
use serde_json::{Map, Value, json};
use wmpass_core::Secret;
use wmpass_core::otp::percent_encode;
use wmpass_net::{Request, Response};

use crate::http::{AsyncHttp, Client};
use crate::{Fault, Result, Sink, Stored};

const USER_AGENT: &str = "WMPass";
const GITHUB_API_VERSION: &str = "2022-11-28";
const RAW_ACCEPT: &str = "application/vnd.github.raw";

/// Which host API a repository is reached through.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GitProvider {
    /// GitHub and GitHub Enterprise.
    #[default]
    GitHub,
    /// GitLab, hosted or self-managed.
    GitLab,
    /// Gitea and Forgejo, which share one API. Codeberg is Forgejo.
    Gitea,
}

impl GitProvider {
    /// The public service, used when [`GitConfig::server`] is empty.
    pub fn default_server(self) -> &'static str {
        match self {
            Self::GitHub => "https://github.com",
            Self::GitLab => "https://gitlab.com",
            Self::Gitea => "https://codeberg.org",
        }
    }
}

/// A folder in a repository.
#[derive(Clone, PartialEq, Eq)]
pub struct GitConfig {
    /// Which API.
    pub provider: GitProvider,
    /// The web address of the host, `https://git.example.com`. Empty means the public
    /// service of the provider. The API address is derived from it.
    pub server: String,
    /// `owner/name`. On GitLab a project in a subgroup is `group/sub/name`.
    pub repository: String,
    /// Empty means the repository's default branch.
    pub branch: String,
    /// Folder inside the repository. Empty is the top.
    pub path: String,
    /// A token that may write repository contents, and nothing more.
    pub token: Secret,
    /// Commit author. Empty uses the account the token belongs to.
    pub author_name: String,
    /// Commit author's email.
    pub author_email: String,
    /// The commit message, with `{action}` (Add, Update or Delete), `{file}`, `{device}`
    /// and `{date}` replaced. Empty means `{action} {file}`.
    pub message: String,
    /// Appends `[skip ci]`, so a backup never starts the repository's CI.
    pub skip_ci: bool,
    /// Allows a repository anyone can read. Off, one is refused: it would hand every
    /// encrypted copy to everybody, history included.
    pub allow_public: bool,
}

/// The message used when [`GitConfig::message`] is empty.
pub const DEFAULT_MESSAGE: &str = "{action} {file}";

impl Default for GitConfig {
    fn default() -> Self {
        Self {
            provider: GitProvider::GitHub,
            server: String::new(),
            repository: String::new(),
            branch: String::new(),
            path: String::new(),
            token: Secret::default(),
            author_name: String::new(),
            author_email: String::new(),
            message: String::new(),
            skip_ci: true,
            allow_public: false,
        }
    }
}

impl GitConfig {
    /// Whether there is enough to reach a repository.
    pub fn configured(&self) -> bool {
        self.repository.contains('/') && !self.token.is_empty()
    }
}

impl std::fmt::Debug for GitConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GitConfig")
            .field("provider", &self.provider)
            .field("skip_ci", &self.skip_ci)
            .field("allow_public", &self.allow_public)
            .finish_non_exhaustive()
    }
}

/// What a status means. A 403 with no requests left is GitHub's rate limit, not a refusal.
pub fn status_fault(status: u16, rate_remaining: Option<&str>) -> Fault {
    match status {
        403 if rate_remaining == Some("0") => Fault::Io,
        429 => Fault::Io,
        401 | 403 => Fault::PermissionLost,
        404 => Fault::TargetMissing,
        507 => Fault::OutOfSpace,
        _ => Fault::Io,
    }
}

/// `https://host` with no trailing slash, from whatever the user typed.
pub fn normalize_server(server: &str) -> String {
    let typed = server.trim().trim_end_matches('/');
    if typed.is_empty() {
        return String::new();
    }
    let lower = typed.to_ascii_lowercase();
    if lower.starts_with("https://") || lower.starts_with("http://") { typed.to_owned() } else { format!("https://{typed}") }
}

/// The REST API root for a provider on a server.
pub fn api_base(provider: GitProvider, server: &str) -> String {
    let typed = normalize_server(server);
    let host = if typed.is_empty() { provider.default_server().to_owned() } else { typed };
    match provider {
        GitProvider::GitHub if host.eq_ignore_ascii_case("https://github.com") => "https://api.github.com".to_owned(),
        GitProvider::GitHub => format!("{host}/api/v3"),
        GitProvider::GitLab => format!("{host}/api/v4"),
        GitProvider::Gitea => format!("{host}/api/v1"),
    }
}

/// A repository address pasted whole, `https://github.com/me/backups.git` or
/// `git@codeberg.org:me/backups.git`, as server and `owner/name`. None for anything that
/// is not one.
pub fn parse_remote(address: &str) -> Option<(String, String)> {
    let trimmed = address.trim();
    let trimmed = trimmed.strip_suffix('/').unwrap_or(trimmed);
    let text = trimmed.strip_suffix(".git").unwrap_or(trimmed);
    if let Some(rest) = text.strip_prefix("git@") {
        let (host, path) = rest.split_once(':')?;
        let (owner, name) = path.split_once('/')?;
        if host.is_empty() || host.contains('/') || owner.is_empty() || name.is_empty() {
            return None;
        }
        return Some((format!("https://{host}"), path.to_owned()));
    }
    let lower = text.to_ascii_lowercase();
    let scheme_end = if lower.starts_with("https://") {
        8
    } else if lower.starts_with("http://") {
        7
    } else {
        return None;
    };
    let after = &text[scheme_end..];
    let (host, path) = after.split_once('/')?;
    if host.is_empty() {
        return None;
    }
    let (head, last) = path.rsplit_once('/')?;
    if head.is_empty() || last.is_empty() {
        return None;
    }
    // A web address inside the repository ("/tree/main/backups") is not the repository.
    let cut = |text: &str, marker: &str| text.split(marker).next().unwrap_or(text).to_owned();
    let path = cut(&cut(&cut(path, "/-/"), "/tree/"), "/src/");
    if !path.contains('/') {
        return None;
    }
    Some((text[..scheme_end + host.len()].to_owned(), path))
}

/// `yyyy-MM-dd HH:mm`, in UTC.
fn minute(now_ms: u64) -> String {
    let stamp = crate::s3::amz_date(now_ms);
    let digits = |range: std::ops::Range<usize>| stamp.get(range).unwrap_or("00").to_owned();
    format!("{}-{}-{} {}:{}", digits(0..4), digits(4..6), digits(6..8), digits(9..11), digits(11..13))
}

/// The commit message for an action on a file, with the user's template and `[skip ci]`.
pub fn commit_message(config: &GitConfig, action: &str, file: &str, device: &str, now_ms: u64) -> String {
    let template = if config.message.trim().is_empty() { DEFAULT_MESSAGE } else { config.message.as_str() };
    let text = template
        .replace("{action}", action)
        .replace("{file}", file)
        .replace("{device}", device)
        .replace("{date}", &minute(now_ms))
        .trim()
        .to_owned();
    if config.skip_ci && !text.contains("[skip ci]") { format!("{text} [skip ci]") } else { text }
}

fn encode_path(path: &str) -> String {
    path.split('/').map(percent_encode).collect::<Vec<_>>().join("/")
}

/// A write that lost a race with another commit, retried once; or any other failure.
enum Failure {
    Conflict,
    Fault(Fault),
}

impl From<Fault> for Failure {
    fn from(fault: Fault) -> Self {
        Self::Fault(fault)
    }
}

fn flatten<T>(result: std::result::Result<T, Failure>) -> Result<T> {
    result.map_err(|failure| match failure {
        Failure::Conflict => Fault::Io,
        Failure::Fault(fault) => fault,
    })
}

/// A folder in a repository.
pub struct Git<H> {
    client: Client<H>,
    config: GitConfig,
    device: String,
    now_ms: u64,
    api: String,
    repo: String,
    folder: String,
    /// The branch once the repository has been checked: the configured one, or its default.
    branch: Mutex<Option<String>>,
}

impl<H> std::fmt::Debug for Git<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Git").field("config", &self.config).finish_non_exhaustive()
    }
}

impl<H: AsyncHttp> Git<H> {
    /// A folder in a repository. `device` fills `{device}` in the commit message, and
    /// `now_ms` fills `{date}`.
    pub fn new(client: Client<H>, config: GitConfig, device: &str, now_ms: u64) -> Self {
        let api = api_base(config.provider, &config.server);
        let repository = config.repository.trim().trim_matches('/');
        let repo = repository.strip_suffix(".git").unwrap_or(repository).to_owned();
        let folder = config.path.trim().trim_matches('/').to_owned();
        Self { client, config, device: device.to_owned(), now_ms, api, repo, folder, branch: Mutex::new(None) }
    }

    fn path_of(&self, name: &str) -> String {
        if self.folder.is_empty() { name.to_owned() } else { format!("{}/{name}", self.folder) }
    }

    fn project(&self) -> String {
        percent_encode(&self.repo)
    }

    fn authorized(&self, mut request: Request, accept: Option<&str>) -> Request {
        let token = self.config.token.expose().trim();
        request.headers.push(("User-Agent", USER_AGENT.to_owned()));
        match self.config.provider {
            GitProvider::GitHub => {
                request.headers.push(("Authorization", format!("Bearer {token}")));
                request.headers.push(("Accept", accept.unwrap_or("application/vnd.github+json").to_owned()));
                request.headers.push(("X-GitHub-Api-Version", GITHUB_API_VERSION.to_owned()));
            }
            GitProvider::GitLab => request.headers.push(("PRIVATE-TOKEN", token.to_owned())),
            GitProvider::Gitea => {
                request.headers.push(("Authorization", format!("token {token}")));
                request.headers.push(("Accept", accept.unwrap_or("application/json").to_owned()));
            }
        }
        request
    }

    async fn call(&self, request: Request, missing_ok: bool) -> std::result::Result<Response, Failure> {
        let writing = request.method != "GET";
        let response = self.client.send(request).await?;
        match response.status {
            200..=299 => Ok(response),
            404 if missing_ok => Ok(response),
            409 if writing => Err(Failure::Conflict),
            422 if writing && String::from_utf8_lossy(&response.body).to_ascii_lowercase().contains("sha") => Err(Failure::Conflict),
            status => Err(Failure::Fault(status_fault(status, response.header("x-ratelimit-remaining")))),
        }
    }

    /// That the repository exists, the token can see it, it is private (or the user
    /// allowed otherwise), and which branch to use. Once per sink.
    async fn check_repository(&self) -> Result<String> {
        if let Some(branch) = self.branch.lock().map_err(|_| Fault::Io)?.clone() {
            return Ok(branch);
        }
        if !self.config.configured() {
            return Err(Fault::NotConfigured);
        }
        let url = match self.config.provider {
            GitProvider::GitHub | GitProvider::Gitea => format!("{}/repos/{}", self.api, self.repo),
            GitProvider::GitLab => format!("{}/projects/{}", self.api, self.project()),
        };
        let info = flatten(self.call(self.authorized(Request::get(url), None), false).await)?.json().map_err(|_| Fault::Io)?;
        let public = match self.config.provider {
            GitProvider::GitLab => info.get("visibility").and_then(Value::as_str) != Some("private"),
            GitProvider::GitHub | GitProvider::Gitea => info.get("private").and_then(Value::as_bool) == Some(false),
        };
        if public && !self.config.allow_public {
            return Err(Fault::Unsafe);
        }
        if info.pointer("/permissions/push").and_then(Value::as_bool) == Some(false) {
            return Err(Fault::PermissionLost);
        }
        let configured = self.config.branch.trim();
        let branch = if configured.is_empty() {
            info.get("default_branch").and_then(Value::as_str).filter(|b| !b.is_empty()).unwrap_or("main").to_owned()
        } else {
            configured.to_owned()
        };
        *self.branch.lock().map_err(|_| Fault::Io)? = Some(branch.clone());
        Ok(branch)
    }

    /// Every file in the folder. Empty when the folder is not there yet.
    async fn list_folder(&self, branch: &str) -> Result<Vec<Stored>> {
        let mut out = Vec::new();
        let reference = percent_encode(branch);
        match self.config.provider {
            GitProvider::GitHub | GitProvider::Gitea => {
                let url = if self.folder.is_empty() {
                    format!("{}/repos/{}/contents?ref={reference}", self.api, self.repo)
                } else {
                    format!("{}/repos/{}/contents/{}?ref={reference}", self.api, self.repo, encode_path(&self.folder))
                };
                let response = flatten(self.call(self.authorized(Request::get(url), None), true).await)?;
                if response.status == 404 {
                    return Ok(out);
                }
                let listing = response.json().map_err(|_| Fault::Io)?;
                for item in listing.as_array().ok_or(Fault::Io)? {
                    if item.get("type").and_then(Value::as_str) != Some("file") {
                        continue;
                    }
                    let text = |key: &str| item.get(key).and_then(Value::as_str).unwrap_or_default();
                    out.push(Stored {
                        id: format!("{}\n{}", text("path"), text("sha")),
                        name: text("name").to_owned(),
                        size: item.get("size").and_then(Value::as_u64).unwrap_or(0),
                    });
                }
            }
            GitProvider::GitLab => {
                let mut page = "1".to_owned();
                while !page.is_empty() {
                    let mut url = format!(
                        "{}/projects/{}/repository/tree?ref={reference}&per_page=100&page={}",
                        self.api,
                        self.project(),
                        percent_encode(&page)
                    );
                    if !self.folder.is_empty() {
                        url.push_str("&path=");
                        url.push_str(&percent_encode(&self.folder));
                    }
                    let response = flatten(self.call(self.authorized(Request::get(url), None), true).await)?;
                    if response.status == 404 {
                        return Ok(out);
                    }
                    let listing = response.json().map_err(|_| Fault::Io)?;
                    for item in listing.as_array().ok_or(Fault::Io)? {
                        if item.get("type").and_then(Value::as_str) != Some("blob") {
                            continue;
                        }
                        let text = |key: &str| item.get(key).and_then(Value::as_str).unwrap_or_default();
                        out.push(Stored { id: format!("{}\n{}", text("path"), text("id")), name: text("name").to_owned(), size: 0 });
                    }
                    page = response.header("x-next-page").unwrap_or_default().trim().to_owned();
                }
            }
        }
        Ok(out)
    }

    fn message(&self, action: &str, file: &str) -> String {
        commit_message(&self.config, action, file, &self.device, self.now_ms)
    }

    fn author(&self) -> Option<Value> {
        let name = self.config.author_name.trim();
        let email = self.config.author_email.trim();
        (!name.is_empty() && !email.is_empty()).then(|| json!({"name": name, "email": email}))
    }

    /// Commits the bytes as `name`, replacing `existing` if there is one. Returns the new
    /// blob id, or empty where the host does not say.
    async fn put(&self, branch: &str, name: &str, bytes: &[u8], existing: Option<&Stored>) -> std::result::Result<String, Failure> {
        let path = self.path_of(name);
        let content = base64::engine::general_purpose::STANDARD.encode(bytes);
        let message = self.message(if existing.is_none() { "Add" } else { "Update" }, name);
        match self.config.provider {
            GitProvider::GitHub | GitProvider::Gitea => {
                let mut body = Map::new();
                body.insert("message".into(), message.into());
                body.insert("content".into(), content.into());
                body.insert("branch".into(), branch.into());
                if let Some(existing) = existing {
                    body.insert("sha".into(), sha_of(existing).into());
                }
                if let Some(author) = self.author() {
                    body.insert("author".into(), author.clone());
                    body.insert("committer".into(), author);
                }
                // Gitea creates with POST and updates with PUT; GitHub does both with PUT.
                let method = if self.config.provider == GitProvider::Gitea && existing.is_none() { "POST" } else { "PUT" };
                let url = format!("{}/repos/{}/contents/{}", self.api, self.repo, encode_path(&path));
                let mut request = self.authorized(Request::json(method, url, &Value::Object(body)), None);
                request.timeout_ms = 300_000;
                let reply = self.call(request, false).await?.json().map_err(|_| Failure::Fault(Fault::Io))?;
                Ok(reply.pointer("/content/sha").and_then(Value::as_str).unwrap_or_default().to_owned())
            }
            GitProvider::GitLab => {
                let mut body = json!({"branch": branch, "content": content, "encoding": "base64", "commit_message": message});
                let (name_given, email_given) = (self.config.author_name.trim(), self.config.author_email.trim());
                if !name_given.is_empty() && !email_given.is_empty() {
                    body["author_name"] = name_given.into();
                    body["author_email"] = email_given.into();
                }
                let method = if existing.is_none() { "POST" } else { "PUT" };
                let url = format!("{}/projects/{}/repository/files/{}", self.api, self.project(), percent_encode(&path));
                let mut request = self.authorized(Request::json(method, url, &body), None);
                request.timeout_ms = 300_000;
                self.call(request, false).await?;
                // GitLab answers with the path and branch only; the blob id comes with
                // the next listing, and nothing needs it before then.
                Ok(String::new())
            }
        }
    }

    async fn remove(&self, branch: &str, file: &Stored) -> std::result::Result<(), Failure> {
        let path = file.id.split('\n').next().unwrap_or_default();
        let message = self.message("Delete", &file.name);
        let request = match self.config.provider {
            GitProvider::GitHub | GitProvider::Gitea => {
                let mut body = json!({"message": message, "sha": sha_of(file), "branch": branch});
                if let Some(author) = self.author() {
                    body["author"] = author.clone();
                    body["committer"] = author;
                }
                let url = format!("{}/repos/{}/contents/{}", self.api, self.repo, encode_path(path));
                Request::json("DELETE", url, &body)
            }
            GitProvider::GitLab => Request::verb(
                "DELETE",
                format!(
                    "{}/projects/{}/repository/files/{}?branch={}&commit_message={}",
                    self.api,
                    self.project(),
                    percent_encode(path),
                    percent_encode(branch),
                    percent_encode(&message)
                ),
            ),
        };
        self.call(self.authorized(request, None), true).await.map(|_| ())
    }
}

fn sha_of(file: &Stored) -> &str {
    file.id.split_once('\n').map_or("", |(_, sha)| sha)
}

impl<H: AsyncHttp> Sink for Git<H> {
    async fn readiness(&self) -> Result<()> {
        self.check_repository().await.map(|_| ())
    }

    async fn write(&self, name: &str, bytes: &[u8]) -> Result<Stored> {
        let branch = self.check_repository().await?;
        let existing = self.list_folder(&branch).await?.into_iter().find(|file| file.name == name);
        let sha = match self.put(&branch, name, bytes, existing.as_ref()).await {
            Ok(sha) => sha,
            // Someone committed in between: another device, or a sync on this one. Look
            // again and go once more.
            Err(Failure::Conflict) => {
                let fresh = self.list_folder(&branch).await?.into_iter().find(|file| file.name == name);
                flatten(self.put(&branch, name, bytes, fresh.as_ref()).await)?
            }
            Err(Failure::Fault(fault)) => return Err(fault),
        };
        Ok(Stored { id: format!("{}\n{sha}", self.path_of(name)), name: name.to_owned(), size: bytes.len() as u64 })
    }

    async fn list(&self) -> Result<Vec<Stored>> {
        let branch = self.check_repository().await?;
        self.list_folder(&branch).await
    }

    async fn read(&self, file: &Stored) -> Result<Vec<u8>> {
        let branch = percent_encode(&self.check_repository().await?);
        let path = file.id.split('\n').next().unwrap_or_default();
        let url = match self.config.provider {
            GitProvider::GitHub => format!("{}/repos/{}/contents/{}?ref={branch}", self.api, self.repo, encode_path(path)),
            GitProvider::GitLab => format!("{}/projects/{}/repository/files/{}/raw?ref={branch}", self.api, self.project(), percent_encode(path)),
            GitProvider::Gitea => format!("{}/repos/{}/raw/{}?ref={branch}", self.api, self.repo, encode_path(path)),
        };
        let mut request = self.authorized(Request::get(url), Some(RAW_ACCEPT));
        request.timeout_ms = 300_000;
        Ok(flatten(self.call(request, false).await)?.body)
    }

    async fn delete(&self, file: &Stored) -> Result<()> {
        let branch = self.check_repository().await?;
        match self.remove(&branch, file).await {
            Ok(()) => Ok(()),
            Err(Failure::Conflict) => match self.list_folder(&branch).await?.into_iter().find(|fresh| fresh.name == file.name) {
                Some(fresh) => flatten(self.remove(&branch, &fresh).await),
                None => Ok(()),
            },
            Err(Failure::Fault(fault)) => Err(fault),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{Blocking, block_on};
    use wmpass_net::NetPolicy;
    use wmpass_net::http::fake::FakeHttp;

    const GH: &str = "https://api.github.com/repos/me/b";

    fn sink(http: &FakeHttp, config: GitConfig) -> Git<Blocking<'_>> {
        Git::new(Client::new(Blocking(http), NetPolicy::Any), config, "Laptop", 0)
    }

    fn github() -> GitConfig {
        GitConfig { repository: "me/b.git".into(), path: "/vault/".into(), token: " tok ".into(), ..GitConfig::default() }
    }

    #[test]
    fn api_roots_and_pasted_addresses() {
        assert_eq!(api_base(GitProvider::GitHub, ""), "https://api.github.com");
        assert_eq!(api_base(GitProvider::GitHub, "ghe.corp/"), "https://ghe.corp/api/v3");
        assert_eq!(api_base(GitProvider::GitLab, ""), "https://gitlab.com/api/v4");
        assert_eq!(api_base(GitProvider::Gitea, ""), "https://codeberg.org/api/v1");
        let pair = |server: &str, repo: &str| Some((server.to_owned(), repo.to_owned()));
        assert_eq!(parse_remote("https://github.com/me/b.git"), pair("https://github.com", "me/b"));
        assert_eq!(parse_remote("git@codeberg.org:me/b.git"), pair("https://codeberg.org", "me/b"));
        assert_eq!(parse_remote("https://gitlab.com/g/sub/b/-/tree/main"), pair("https://gitlab.com", "g/sub/b"));
        assert_eq!(parse_remote("https://codeberg.org/me/b/src/branch/main"), pair("https://codeberg.org", "me/b"));
        assert_eq!(parse_remote("me/b"), None);
        assert_eq!(parse_remote("https://github.com/me"), None);
    }

    #[test]
    fn commit_messages_take_the_template_and_skip_ci() {
        let config = GitConfig { message: "{action} {file} from {device}".into(), ..GitConfig::default() };
        assert_eq!(commit_message(&config, "Add", "x.json", "Pixel", 0), "Add x.json from Pixel [skip ci]");
        let plain = GitConfig { skip_ci: false, ..GitConfig::default() };
        assert_eq!(commit_message(&plain, "Delete", "x.json", "", 0), "Delete x.json");
        let dated = GitConfig { message: "{date}".into(), skip_ci: false, ..GitConfig::default() };
        assert_eq!(commit_message(&dated, "Add", "x", "", 1_754_575_353_000), "2025-08-07 14:02");
    }

    #[test]
    fn errors_tell_the_rate_limit_apart_from_a_refusal() {
        assert_eq!(status_fault(403, Some("0")), Fault::Io);
        assert_eq!(status_fault(403, Some("12")), Fault::PermissionLost);
        assert_eq!(status_fault(401, None), Fault::PermissionLost);
        assert_eq!(status_fault(404, None), Fault::TargetMissing);
        assert!(!GitConfig { repository: "noslash".into(), token: "t".into(), ..GitConfig::default() }.configured());
        assert!(GitConfig { repository: "me/r".into(), token: "t".into(), ..GitConfig::default() }.configured());
    }

    #[test]
    fn a_public_repository_is_refused_unless_allowed() {
        let http = FakeHttp::new().answer(GH, 200, r#"{"private":false,"default_branch":"main"}"#);
        assert_eq!(block_on(sink(&http, github()).readiness()), Err(Fault::Unsafe));
        let allowed = GitConfig { allow_public: true, ..github() };
        assert_eq!(block_on(sink(&http, allowed).readiness()), Ok(()));
        let read_only = FakeHttp::new().answer(GH, 200, r#"{"private":true,"permissions":{"push":false}}"#);
        assert_eq!(block_on(sink(&read_only, github()).readiness()), Err(Fault::PermissionLost));
        let gitlab = FakeHttp::new().answer("https://gitlab.com/api/v4/projects/me%2Fb", 200, r#"{"visibility":"internal"}"#);
        let config = GitConfig { provider: GitProvider::GitLab, ..github() };
        assert_eq!(block_on(sink(&gitlab, config).readiness()), Err(Fault::Unsafe));
    }

    #[test]
    fn github_writes_a_new_file_then_updates_it_with_its_sha() {
        let http = FakeHttp::new()
            .answer(&format!("{GH}/contents/vault?ref=dev"), 200, r#"[{"type":"dir","name":"d","path":"vault/d"},{"type":"file","name":"v.bin","path":"vault/v.bin","sha":"old","size":2}]"#)
            .answer(&format!("{GH}/contents/vault/v.bin"), 200, r#"{"content":{"sha":"new"}}"#)
            .answer(GH, 200, r#"{"private":true,"default_branch":"dev"}"#);
        let stored = block_on(sink(&http, github()).write("v.bin", b"data")).unwrap();
        assert_eq!(stored, Stored { id: "vault/v.bin\nnew".into(), name: "v.bin".into(), size: 4 });
        let put = http.last().unwrap();
        assert_eq!(put.method, "PUT");
        assert!(put.headers.iter().any(|(n, v)| *n == "Authorization" && v == "Bearer tok"));
        let body: Value = serde_json::from_slice(&put.body.unwrap().1).unwrap();
        assert_eq!(body["sha"], "old");
        assert_eq!(body["branch"], "dev");
        assert_eq!(body["content"], "ZGF0YQ==");
        assert_eq!(body["message"], "Update v.bin [skip ci]");
    }

    #[test]
    fn a_lost_race_is_retried_once_with_a_fresh_listing() {
        let http = FakeHttp::new()
            .answer_once(&format!("{GH}/contents/vault?"), 200, "[]")
            .answer(&format!("{GH}/contents/vault?"), 200, r#"[{"type":"file","name":"v","path":"vault/v","sha":"theirs"}]"#)
            .answer_once(&format!("{GH}/contents/vault/v"), 409, "")
            .answer(&format!("{GH}/contents/vault/v"), 200, r#"{"content":{"sha":"mine"}}"#)
            .answer(GH, 200, r#"{"private":true}"#);
        let stored = block_on(sink(&http, github()).write("v", b"x")).unwrap();
        assert_eq!(stored.id, "vault/v\nmine");
        let body: Value = serde_json::from_slice(&http.last().unwrap().body.unwrap().1).unwrap();
        assert_eq!(body["sha"], "theirs");
    }

    #[test]
    fn gitea_creates_with_post_and_reads_raw() {
        let base = "https://git.example/api/v1/repos/me/b";
        let http = FakeHttp::new()
            .answer(&format!("{base}/contents/v?"), 404, "")
            .answer(&format!("{base}/contents?"), 200, "[]")
            .answer(&format!("{base}/contents/v"), 201, r#"{"content":{"sha":"s"}}"#)
            .answer(&format!("{base}/raw/v"), 200, "data")
            .answer(base, 200, r#"{"private":true,"default_branch":"main"}"#);
        let config = GitConfig { provider: GitProvider::Gitea, server: "git.example".into(), path: String::new(), ..github() };
        let sink = sink(&http, config);
        let stored = block_on(sink.write("v", b"data")).unwrap();
        let asked = http.last().unwrap();
        assert_eq!(asked.method, "POST");
        assert!(asked.headers.iter().any(|(n, v)| *n == "Authorization" && v == "token tok"));
        assert_eq!(block_on(sink.read(&stored)).unwrap(), b"data");
        assert!(http.last().unwrap().url.ends_with("/raw/v?ref=main"));
    }

    #[test]
    fn gitlab_pages_through_the_tree_and_deletes_with_a_message() {
        let tree = "https://gitlab.com/api/v4/projects/g%2Fsub%2Fb/repository/tree";
        let http = FakeHttp::new()
            .answer(&format!("{tree}?ref=main&per_page=100&page=2"), 200, r#"[{"type":"blob","name":"b","path":"vault/b","id":"2"}]"#)
            .answer_with(&format!("{tree}?ref=main&per_page=100&page=1"), 200, br#"[{"type":"tree","name":"d"},{"type":"blob","name":"a","path":"vault/a","id":"1"}]"#, &[("X-Next-Page", "2")])
            .answer("https://gitlab.com/api/v4/projects/g%2Fsub%2Fb/repository/files", 204, "")
            .answer("https://gitlab.com/api/v4/projects/g%2Fsub%2Fb", 200, r#"{"visibility":"private","default_branch":"main"}"#);
        let config = GitConfig { provider: GitProvider::GitLab, repository: "g/sub/b".into(), ..github() };
        let sink = sink(&http, config);
        let files = block_on(sink.list()).unwrap();
        assert_eq!(files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), vec!["a", "b"]);
        assert!(http.last().unwrap().url.ends_with("&path=vault"));
        block_on(sink.delete(&files[0])).unwrap();
        let asked = http.last().unwrap();
        assert_eq!(asked.method, "DELETE");
        assert!(asked.url.ends_with("/files/vault%2Fa?branch=main&commit_message=Delete%20a%20%5Bskip%20ci%5D"));
        assert!(asked.headers.iter().any(|(n, v)| *n == "PRIVATE-TOKEN" && v == "tok"));
    }

    #[test]
    fn debug_shows_no_token_or_repository() {
        let printed = format!("{:?}", github());
        assert!(!printed.contains("tok") && !printed.contains("me/b"));
    }
}
