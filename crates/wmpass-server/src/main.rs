//! Runs the server.

use std::net::SocketAddr;
use std::path::PathBuf;

use wmpass_server::{Config, Signups};

const HELP: &str = "\
wmpass-server: self-hosted sync for WM Pass and Bitwarden clients

  --bind ADDRESS       Where to listen. Default 127.0.0.1:8222
  --data FOLDER        Signing key and database. Default ./wmpass-data
  --database URL       sqlite://... or postgres://...
  --signups MODE       closed (default), open, or a comma-separated list of addresses
  --behind-proxy       Required to bind beyond this machine, and promises that a reverse
                       proxy terminates TLS in front of this server

Every option can also be set in the environment as WMPASS_BIND, WMPASS_DATA,
WMPASS_DATABASE, WMPASS_SIGNUPS and WMPASS_BEHIND_PROXY=1.
";

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("wmpass-server: {message}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut bind = std::env::var("WMPASS_BIND").unwrap_or_else(|_| "127.0.0.1:8222".to_owned());
    let mut data = std::env::var("WMPASS_DATA").unwrap_or_else(|_| "wmpass-data".to_owned());
    let mut database = std::env::var("WMPASS_DATABASE").ok();
    let mut signups = std::env::var("WMPASS_SIGNUPS").unwrap_or_else(|_| "closed".to_owned());
    let mut behind_proxy = std::env::var("WMPASS_BEHIND_PROXY").is_ok_and(|v| v == "1");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--bind" => bind = value()?,
            "--data" => data = value()?,
            "--database" => database = Some(value()?),
            "--signups" => signups = value()?,
            "--behind-proxy" => behind_proxy = true,
            "--help" | "-h" => {
                print!("{HELP}");
                return Ok(());
            }
            other => return Err(format!("unknown option {other}")),
        }
    }
    let address: SocketAddr = bind.parse().map_err(|_| format!("{bind} is not an address and port"))?;
    if !address.ip().is_loopback() && !behind_proxy {
        return Err(format!(
            "refusing to listen on {address} without TLS. Put a reverse proxy in front and pass --behind-proxy."
        ));
    }
    let signups = match signups.as_str() {
        "closed" => Signups::Closed,
        "open" => Signups::Open,
        list => Signups::Emails(list.split(',').map(|s| s.trim().to_lowercase()).collect()),
    };
    let config = Config { data_dir: PathBuf::from(data), database_url: database, signups, public_url: None };
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().map_err(|e| e.to_string())?;
    runtime.block_on(async move {
        let app = wmpass_server::build(config).await?;
        let listener = tokio::net::TcpListener::bind(address).await.map_err(|e| format!("cannot listen on {address}: {e}"))?;
        println!("wmpass-server listening on {address}");
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = tokio::signal::ctrl_c().await;
            })
            .await
            .map_err(|e| e.to_string())
    })
}
