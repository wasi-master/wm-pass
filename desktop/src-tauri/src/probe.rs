//! Username pages that only show their answer once their own scripts have run. Each loads in a
//! hidden, private window that keeps nothing afterwards and may not leave the site's domain, so it
//! cannot reach the app's own pages. No capability names the window, and Tauri refuses commands
//! from a remote page without one. The page reports its text through its title.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use tauri::utils::config::BackgroundThrottlingPolicy;
use tauri::webview::NewWindowResponse;
use tauri::{AppHandle, WebviewUrl, WebviewWindowBuilder};
use wmpass_net::usernames::{self, Availability, PageSite};

/// Long enough for a slow page on a slow line. A page still blank after this has no answer.
const WAIT: Duration = Duration::from_secs(15);
/// Marks a title this module set, so the page's own titles are ignored.
const TAG: &str = "\u{2063}wmpass-page:";

/// Sends the page's text up through its title, twice a second, from the top frame only.
const REPORT: &str = r"(() => {
  if (window.top !== window) return;
  setInterval(() => {
    const text = document.body ? document.body.innerText.slice(0, 20000) : '';
    document.title = '⁣wmpass-page:' + text;
  }, 500);
})();";

/// macOS's web view leaves "Safari" out of its user agent, and TikTok holds such a browser at its
/// bot check. The other systems' web views name their browser already.
#[cfg(target_os = "macos")]
const AGENT: Option<&str> = Some("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15");
#[cfg(not(target_os = "macos"))]
const AGENT: Option<&str> = None;

/// One window at a time: the checker asks one site after another anyway.
static ONE: Mutex<()> = Mutex::new(());
static NEXT: AtomicU64 = AtomicU64::new(0);

pub fn site(name: &str) -> Result<&'static PageSite, String> {
    usernames::PAGE_SITES.iter().find(|s| s.name == name).ok_or_else(|| "unknown site".to_owned())
}

/// Loads one page and waits for its answer. Runs off the main thread.
pub fn check(app: &AppHandle, site: &'static PageSite, name: &str) -> Result<Availability, String> {
    let url = usernames::page_url(site, name).map_err(|e| e.to_string())?;
    let url = url.parse().map_err(|_| "that address is not valid".to_owned())?;
    let _one = ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (sender, answers) = mpsc::channel();
    let label = format!("username-page-{}", NEXT.fetch_add(1, Ordering::SeqCst));
    let mut builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(url));
    if let Some(agent) = AGENT {
        builder = builder.user_agent(agent);
    }
    let window = builder
        .title("WM Pass username check")
        .visible(false)
        .focused(false)
        .skip_taskbar(true)
        .inner_size(1280.0, 900.0)
        .incognito(true)
        .background_throttling(BackgroundThrottlingPolicy::Disabled)
        .initialization_script(REPORT)
        .on_navigation(move |url| usernames::may_visit(site, url.as_str()))
        .on_new_window(|_, _| NewWindowResponse::Deny)
        .on_download(|_, _| false)
        .on_document_title_changed(move |_, title| {
            if let Some(answer) = title.strip_prefix(TAG).and_then(|text| usernames::read_page(site, text)) {
                let _ = sender.send(answer);
            }
        })
        .build()
        .map_err(|e| e.to_string())?;
    let answer = answers.recv_timeout(WAIT).unwrap_or(Availability::Unknown);
    let _ = window.destroy();
    Ok(answer)
}
