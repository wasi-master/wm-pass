//! Places outside this computer, through the shared sink code. The page says what to do; the
//! order backups and syncs do things in lives on its side, so every platform does the same.

use serde::{Deserialize, Serialize};
use wmpass_core::Secret;
use wmpass_net::http::{NativeHttp, NetPolicy};
use wmpass_sinks::folder::Folder;
use wmpass_sinks::http::{Blocking, Client, block_on};
use wmpass_sinks::s3::{S3, S3Config};
use wmpass_sinks::webdav::WebDav;
use wmpass_sinks::{Sink, Stored};

#[derive(Deserialize, Default, Clone)]
#[serde(default)]
pub struct Place {
    pub kind: String,
    pub path: String,
    pub url: String,
    pub user: String,
    pub password: String,
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub prefix: String,
    pub access_key: String,
    pub secret_key: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PlaceFile {
    pub id: String,
    pub name: String,
    pub size: u64,
    pub modified_ms: u64,
}

impl From<Stored> for PlaceFile {
    fn from(file: Stored) -> Self {
        Self { id: file.id, name: file.name, size: file.size, modified_ms: 0 }
    }
}

impl From<&PlaceFile> for Stored {
    fn from(file: &PlaceFile) -> Self {
        Self { id: file.id.clone(), name: file.name.clone(), size: file.size }
    }
}

/// Runs one operation against whichever kind of place this is.
macro_rules! on_sink {
    ($place:expr, $online:expr, |$sink:ident| $body:expr) => {{
        let place: &Place = $place;
        let policy = if $online { NetPolicy::Any } else { NetPolicy::Offline };
        let http = NativeHttp::default();
        let client = Client::new(Blocking(&http), policy);
        match place.kind.as_str() {
            "folder" => {
                let $sink = Folder::new(&place.path);
                block_on(async { $body.await }).map_err(|fault| fault.to_string())
            }
            "webdav" => {
                let $sink = WebDav::new(client, &place.url, &place.user, &Secret::new(place.password.as_str()));
                block_on(async { $body.await }).map_err(|fault| fault.to_string())
            }
            "s3" => {
                let config = S3Config {
                    endpoint: place.endpoint.clone(),
                    region: place.region.clone(),
                    bucket: place.bucket.clone(),
                    prefix: place.prefix.clone(),
                    access_key: place.access_key.clone(),
                };
                let $sink = S3::new(client, config, Secret::new(place.secret_key.as_str()), crate::engine::now_ms());
                block_on(async { $body.await }).map_err(|fault| fault.to_string())
            }
            _ => Err("this kind of place is not available here".to_owned()),
        }
    }};
}

pub fn list(place: &Place, online: bool) -> Result<Vec<PlaceFile>, String> {
    on_sink!(place, online, |sink| sink.list()).map(|files| files.into_iter().map(PlaceFile::from).collect())
}

pub fn read(place: &Place, online: bool, file: &PlaceFile) -> Result<Vec<u8>, String> {
    let stored = Stored::from(file);
    on_sink!(place, online, |sink| sink.read(&stored))
}

pub fn write(place: &Place, online: bool, name: &str, bytes: &[u8]) -> Result<(), String> {
    on_sink!(place, online, |sink| sink.write(name, bytes)).map(|_| ())
}

pub fn remove(place: &Place, online: bool, file: &PlaceFile) -> Result<(), String> {
    let stored = Stored::from(file);
    on_sink!(place, online, |sink| sink.delete(&stored))
}
