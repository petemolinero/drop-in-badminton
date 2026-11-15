//! One-click updates from GitHub Releases.
//!
//! On startup (and every few hours) a background thread asks GitHub for the
//! latest release. If it's newer than this build and has a file for this
//! platform, the ribbon shows an Update button. Installing downloads that file,
//! swaps it in for the running program, and restarts.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const REPO: &str = "petemolinero/goodminton";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The release file built for this platform (see `.github/workflows/release.yml`).
pub const ASSET: Option<&str> = if cfg!(all(windows, target_arch = "x86_64")) {
    Some("goodminton-windows-x86_64.exe")
} else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
    Some("goodminton-linux-x86_64")
} else {
    None
};

const RECHECK: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    /// Without the leading "v", e.g. "0.2.0".
    pub version: String,
    pub url: String,
    pub notes: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    /// Downloading, with progress from 0 to 1 (if the size is known).
    Installing(Release, Option<f32>),
    /// The new program is in place; it runs after a restart.
    Installed(Release),
    Failed(String),
}

pub struct Updater {
    status: Arc<Mutex<Status>>,
    last_check: Option<Instant>,
}

/// Where this program was launched from, captured before any update replaces it.
static EXE: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Set when the app should start itself again after the window closes.
static RESTART: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn remember_exe() {
    EXE.get_or_init(|| std::env::current_exe().ok());
}

/// Ask `main` to relaunch once the window has closed and everything is saved.
pub fn request_restart() {
    RESTART.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// Called by `main` after the event loop ends.
pub fn restart_if_requested() {
    if !RESTART.load(std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    if let Some(Some(exe)) = EXE.get() {
        if let Err(err) = std::process::Command::new(exe).spawn() {
            eprintln!("goodminton: couldn't restart: {err}");
        }
    }
}

impl Updater {
    pub fn new() -> Self {
        remember_exe();
        Self { status: Arc::new(Mutex::new(Status::Idle)), last_check: None }
    }

    /// Updates are off in development builds and on platforms with no release file.
    pub fn enabled() -> bool {
        ASSET.is_some() && !cfg!(debug_assertions) && std::env::var_os("GOODMINTON_NO_UPDATE").is_none()
    }

    /// Debug screenshots: pretend an update was found.
    #[cfg(debug_assertions)]
    pub fn debug_set(&self, status: Status) {
        *self.status.lock().unwrap() = status;
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }

    fn set(status: &Arc<Mutex<Status>>, ctx: &egui::Context, value: Status) {
        *status.lock().unwrap() = value;
        ctx.request_repaint();
    }

    /// Check now and then, in the background. Call every frame.
    pub fn poll(&mut self, ctx: &egui::Context) {
        let due = self.last_check.is_none_or(|t| t.elapsed() > RECHECK);
        let busy = matches!(
            self.status(),
            Status::Checking | Status::Available(_) | Status::Installing(..) | Status::Installed(_)
        );
        if Self::enabled() && due && !busy {
            self.check(ctx);
        }
    }

    pub fn check(&mut self, ctx: &egui::Context) {
        if !Self::enabled() || matches!(self.status(), Status::Checking | Status::Installing(..)) {
            return;
        }
        self.last_check = Some(Instant::now());
        Self::set(&self.status, ctx, Status::Checking);
        let (status, ctx) = (self.status.clone(), ctx.clone());
        std::thread::spawn(move || {
            let result = match latest_release() {
                Ok(Some(release)) if is_newer(&release.version, VERSION) => Status::Available(release),
                Ok(_) => Status::UpToDate,
                Err(err) => {
                    eprintln!("goodminton: update check failed: {err}");
                    Status::Failed(format!("Couldn't check for updates ({err})"))
                }
            };
            Self::set(&status, &ctx, result);
        });
    }

    /// Download and swap in the available release.
    pub fn install(&mut self, ctx: &egui::Context) {
        let Status::Available(release) = self.status() else { return };
        Self::set(&self.status, ctx, Status::Installing(release.clone(), None));
        let (status, ctx) = (self.status.clone(), ctx.clone());
        std::thread::spawn(move || {
            let result = match download_and_replace(&release, |p| {
                Self::set(&status, &ctx, Status::Installing(release.clone(), p));
            }) {
                Ok(()) => Status::Installed(release.clone()),
                Err(err) => {
                    eprintln!("goodminton: update failed: {err}");
                    Status::Failed(format!("The update didn't install ({err})"))
                }
            };
            Self::set(&status, &ctx, result);
        });
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_global(Some(Duration::from_secs(10 * 60)))
        .user_agent(concat!("goodminton/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

/// The newest published release that has a file for this platform.
fn latest_release() -> Result<Option<Release>, String> {
    let Some(asset_name) = ASSET else { return Ok(None) };
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let mut resp = match agent().get(&url).header("Accept", "application/vnd.github+json").call() {
        Ok(resp) => resp,
        // No releases published yet.
        Err(ureq::Error::StatusCode(404)) => return Ok(None),
        Err(err) => return Err(err.to_string()),
    };
    let body = resp.body_mut().read_to_string().map_err(|e| e.to_string())?;
    parse_release(&body, asset_name)
}

fn parse_release(body: &str, asset_name: &str) -> Result<Option<Release>, String> {
    let json: serde_json::Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let tag = json["tag_name"].as_str().ok_or("release has no tag")?;
    let asset = json["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|a| a["name"].as_str() == Some(asset_name));
    let Some(url) = asset.and_then(|a| a["browser_download_url"].as_str()) else {
        return Ok(None);
    };
    Ok(Some(Release {
        version: tag.trim_start_matches('v').to_owned(),
        url: url.to_owned(),
        notes: json["body"].as_str().unwrap_or_default().trim().to_owned(),
    }))
}

fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let mut parts = v.trim().trim_start_matches('v').split('.').map(|p| p.parse::<u64>().ok());
    Some((parts.next()??, parts.next().unwrap_or(Some(0))?, parts.next().unwrap_or(Some(0))?))
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

fn download_and_replace(release: &Release, progress: impl Fn(Option<f32>)) -> Result<(), String> {
    use std::io::{Read, Write};
    let mut resp = agent().get(&release.url).call().map_err(|e| e.to_string())?;
    let total = resp.body().content_length();
    let mut reader = resp.body_mut().with_config().limit(500 * 1024 * 1024).reader();

    let tmp = std::env::temp_dir().join(format!("goodminton-{}.download", release.version));
    let mut file = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
    let (mut buf, mut done, mut last) = (vec![0u8; 64 * 1024], 0u64, Instant::now());
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        if last.elapsed() > Duration::from_millis(100) {
            last = Instant::now();
            progress(total.map(|t| done as f32 / t.max(1) as f32));
        }
    }
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);

    let result = check_binary(&tmp, total, done).and_then(|()| {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
        }
        self_replace::self_replace(&tmp).map_err(|e| e.to_string())
    });
    let _ = std::fs::remove_file(&tmp);
    result
}

/// Refuse anything that isn't a complete program for this platform.
fn check_binary(path: &std::path::Path, expected: Option<u64>, got: u64) -> Result<(), String> {
    if expected.is_some_and(|e| e != got) {
        return Err("download was incomplete".into());
    }
    if got < 1024 * 1024 {
        return Err("download was too small".into());
    }
    let mut magic = [0u8; 4];
    use std::io::Read;
    std::fs::File::open(path).and_then(|mut f| f.read_exact(&mut magic)).map_err(|e| e.to_string())?;
    let ok = if cfg!(windows) { magic.starts_with(b"MZ") } else { magic == *b"\x7fELF" };
    if ok { Ok(()) } else { Err("download wasn't a program".into()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically() {
        assert!(is_newer("v0.2.0", "0.1.0"));
        assert!(is_newer("0.10.0", "0.9.3"));
        assert!(is_newer("1.0", "0.9.9"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("v0.1.0", "0.2.0"));
        assert!(!is_newer("nightly", "0.1.0"));
    }

    #[test]
    fn finds_this_platforms_file_in_a_release() {
        let body = r#"{"tag_name":"v0.3.1","body":"Faster rounds",
            "assets":[{"name":"other.zip","browser_download_url":"https://x/other"},
                      {"name":"goodminton-windows-x86_64.exe","browser_download_url":"https://x/win"}]}"#;
        let r = parse_release(body, "goodminton-windows-x86_64.exe").unwrap().unwrap();
        assert_eq!(r, Release { version: "0.3.1".into(), url: "https://x/win".into(), notes: "Faster rounds".into() });
        assert_eq!(parse_release(body, "goodminton-linux-x86_64").unwrap(), None);
    }
}
