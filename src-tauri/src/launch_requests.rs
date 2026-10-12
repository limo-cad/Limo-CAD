//! Relaunch delivery scoped to this executable artifact and optional test profile.
//! The file lock is claimed before GUI initialization. The authenticated loopback
//! receiver retains requests before the WebView exists; only frontend handling
//! acknowledges them. No command line is evaluated and no engine mutation occurs here.
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    fs::{self, File, OpenOptions, TryLockError},
    hash::{DefaultHasher, Hash, Hasher},
    io::{Read, Write},
    net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{Emitter, Manager};

const MAX_REQUESTS: usize = 16;
const MAX_PACKET_BYTES: usize = 64 * 1024;
const MAX_TARGET_BYTES: usize = 32 * 1024;
const DELIVERY_AGE_MS: u64 = 30_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum LaunchTarget {
    File { path: String },
    Recipe { recipe: String },
}

impl LaunchTarget {
    pub fn from_argument(argument: &str, cwd: &Path) -> Result<Self, String> {
        if argument.len() > MAX_TARGET_BYTES || argument.is_empty() || argument.contains('\0') {
            return Err("Invalid or oversized launch target".into());
        }
        if argument.starts_with("nbcad:") {
            return nbcad_mcp::recipe_id_from_uri(argument).map(|recipe| Self::Recipe {
                recipe: recipe.into(),
            });
        }
        let path = Path::new(argument);
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            cwd.join(path)
        };
        let path = path
            .to_str()
            .ok_or("File path must be valid Unicode")?
            .to_string();
        let target = Self::File { path };
        target.validate()?;
        Ok(target)
    }

    fn validate(&self) -> Result<(), String> {
        match self {
            Self::File { path } => {
                if path.is_empty()
                    || path.len() > MAX_TARGET_BYTES
                    || path.contains('\0')
                    || !Path::new(path).is_absolute()
                {
                    return Err("Launch file path must be absolute and bounded".into());
                }
            }
            Self::Recipe { recipe } => {
                nbcad_mcp::recipe_id_from_uri(&format!("nbcad://recipe/{recipe}"))?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Serialize)]
pub struct PendingLaunchRequest {
    id: u64,
    #[serde(flatten)]
    target: LaunchTarget,
}

#[derive(Default)]
struct Queue {
    next_id: u64,
    pending: VecDeque<PendingLaunchRequest>,
    deliveries: VecDeque<String>,
}

impl Queue {
    fn accept(&mut self, delivery: &str, targets: &[LaunchTarget]) -> Result<(), String> {
        if self.deliveries.iter().any(|seen| seen == delivery) {
            return Ok(());
        }
        if targets.len() > MAX_REQUESTS {
            return Err("Too many launch requests".into());
        }
        let mut unique = Vec::new();
        for target in targets {
            target.validate()?;
            if !self.pending.iter().any(|pending| pending.target == *target)
                && !unique.contains(target)
            {
                unique.push(target.clone());
            }
        }
        if self.pending.len() + unique.len() > MAX_REQUESTS {
            return Err("The launch queue is full; open another window".into());
        }
        for target in unique {
            self.next_id += 1;
            self.pending.push_back(PendingLaunchRequest {
                id: self.next_id,
                target,
            });
        }
        self.deliveries.push_back(delivery.to_string());
        if self.deliveries.len() > 64 {
            self.deliveries.pop_front();
        }
        Ok(())
    }

    fn acknowledge(&mut self, id: u64) {
        self.pending.retain(|request| request.id != id);
    }
}

#[derive(Default)]
struct Shared {
    queue: Mutex<Queue>,
    app: Mutex<Option<tauri::AppHandle>>,
}

#[derive(Clone, Default)]
pub struct LaunchRequestsState {
    shared: Arc<Shared>,
    // Retain the OS lock for the lifetime of the ordinary primary instance.
    _lock: Option<Arc<File>>,
    pub isolated: bool,
}

impl LaunchRequestsState {
    pub fn standalone(targets: &[LaunchTarget]) -> Self {
        let state = Self {
            isolated: true,
            ..Self::default()
        };
        let _ = state
            .shared
            .queue
            .lock()
            .unwrap()
            .accept("initial", targets);
        state
    }

    pub fn install(&self, app: &tauri::AppHandle) {
        *self.shared.app.lock().unwrap() = Some(app.clone());
        notify(&self.shared);
    }
}

pub enum Relaunch {
    Forwarded,
    Primary(LaunchRequestsState),
}

pub fn reuse_enabled(
    new_window: bool,
    explicit_reuse: bool,
    profile: bool,
    diagnostic: bool,
) -> bool {
    !new_window && (explicit_reuse || (!profile && !diagnostic))
}

fn identity(
    executable: &Path,
    bytes: u64,
    modified: u128,
    profile: Option<&std::ffi::OsStr>,
    report: Option<&std::ffi::OsStr>,
) -> String {
    let mut hasher = DefaultHasher::new();
    (
        executable,
        bytes,
        modified,
        env!("CARGO_PKG_VERSION"),
        profile,
        report,
    )
        .hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn storage_directory() -> Result<PathBuf, String> {
    #[cfg(target_os = "windows")]
    let root = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let root = std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join("Library/Application Support"));
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let root = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")));
    let directory = root
        .ok_or("Local user application storage is unavailable")?
        .join("org.nbcad.desktop/launch");
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
    }
    Ok(directory)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Endpoint {
    version: u8,
    identity: String,
    port: u16,
    token: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Delivery {
    version: u8,
    token: String,
    id: String,
    issued_ms: u64,
    targets: Vec<LaunchTarget>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    id: String,
    accepted: bool,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn read_packet<T: serde::de::DeserializeOwned>(stream: &mut TcpStream) -> Result<T, String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    stream
        .take((MAX_PACKET_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > MAX_PACKET_BYTES {
        return Err("Oversized launch delivery".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "Invalid launch delivery".into())
}

fn send_packet<T: Serialize>(stream: &mut TcpStream, value: &T) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    if bytes.len() > MAX_PACKET_BYTES {
        return Err("Oversized launch delivery".into());
    }
    stream
        .set_write_timeout(Some(Duration::from_secs(1)))
        .map_err(|error| error.to_string())?;
    stream
        .write_all(&bytes)
        .map_err(|error| error.to_string())?;
    stream
        .shutdown(Shutdown::Write)
        .map_err(|error| error.to_string())
}

fn delivery_valid(delivery: &Delivery, token: &str, now: u64) -> bool {
    delivery.version == 1
        && delivery.token == token
        && uuid::Uuid::parse_str(&delivery.id).is_ok()
        && now.abs_diff(delivery.issued_ms) <= DELIVERY_AGE_MS
        && delivery.targets.len() <= MAX_REQUESTS
}

fn notify(shared: &Shared) {
    let Some(app) = shared.app.lock().ok().and_then(|app| app.clone()) else {
        return;
    };
    let _ = app.emit("native-launch-pending", ());
    let focus_app = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(window) = focus_app.get_webview_window("main") {
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
        }
    });
}

fn serve(listener: TcpListener, state: LaunchRequestsState, token: String) {
    for incoming in listener.incoming() {
        let Ok(mut stream) = incoming else {
            continue;
        };
        let accepted = receive_delivery(&mut stream, &state, &token).unwrap_or(false);
        if accepted {
            notify(&state.shared);
        }
    }
}

fn receive_delivery(
    stream: &mut TcpStream,
    state: &LaunchRequestsState,
    token: &str,
) -> Result<bool, String> {
    let delivery = read_packet::<Delivery>(stream)?;
    let accepted = delivery_valid(&delivery, token, now_ms())
        && state
            .shared
            .queue
            .lock()
            .is_ok_and(|mut queue| queue.accept(&delivery.id, &delivery.targets).is_ok());
    // Queue acceptance precedes the receipt, independently of WebView readiness.
    send_packet(
        stream,
        &Receipt {
            id: delivery.id,
            accepted,
        },
    )?;
    Ok(accepted)
}

fn forward(path: &Path, expected_identity: &str, targets: &[LaunchTarget]) -> Result<(), String> {
    let id = uuid::Uuid::new_v4().to_string();
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(3) {
        let attempt = (|| {
            let mut bytes = Vec::new();
            File::open(path)
                .map_err(|_| "Existing window is still starting")?
                .take((MAX_PACKET_BYTES + 1) as u64)
                .read_to_end(&mut bytes)
                .map_err(|_| "Existing window endpoint is unavailable")?;
            if bytes.len() > MAX_PACKET_BYTES {
                return Err("Invalid existing-window endpoint".into());
            }
            let endpoint: Endpoint =
                serde_json::from_slice(&bytes).map_err(|_| "Existing window is still starting")?;
            if endpoint.version != 1
                || endpoint.identity != expected_identity
                || uuid::Uuid::parse_str(&endpoint.token).is_err()
            {
                return Err("Existing-window endpoint does not match this launch".into());
            }
            let address = SocketAddr::from((Ipv4Addr::LOCALHOST, endpoint.port));
            let mut stream = TcpStream::connect_timeout(&address, Duration::from_millis(250))
                .map_err(|_| "Existing window is unavailable")?;
            send_packet(
                &mut stream,
                &Delivery {
                    version: 1,
                    token: endpoint.token,
                    id: id.clone(),
                    issued_ms: now_ms(),
                    targets: targets.to_vec(),
                },
            )?;
            let receipt: Receipt = read_packet(&mut stream)?;
            if receipt.id == id && receipt.accepted {
                Ok(())
            } else {
                Err("Existing window could not accept the launch request".into())
            }
        })();
        if attempt.is_ok() {
            return attempt;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    Err("Existing-window delivery did not complete; opening another window".into())
}

/// Diagnostic profiles opt out by default. --reuse-window is an explicit,
/// profile-scoped test opt-in; it never joins an ordinary user's namespace.
pub fn prepare(
    new_window: bool,
    explicit_reuse: bool,
    targets: &[LaunchTarget],
) -> Result<Relaunch, String> {
    let profile = std::env::var_os("WEBVIEW2_USER_DATA_FOLDER");
    let report = std::env::var_os("NBCAD_STARTUP_REPORT_FILE");
    if !reuse_enabled(
        new_window,
        explicit_reuse,
        profile.is_some(),
        report.is_some(),
    ) {
        return Ok(Relaunch::Primary(LaunchRequestsState::standalone(targets)));
    }
    let executable = std::env::current_exe()
        .and_then(fs::canonicalize)
        .map_err(|error| error.to_string())?;
    let metadata = fs::metadata(&executable).map_err(|error| error.to_string())?;
    let modified = metadata
        .modified()
        .map_err(|error| error.to_string())?
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let key = identity(
        &executable,
        metadata.len(),
        modified,
        profile.as_deref(),
        report.as_deref(),
    );
    let directory = storage_directory()?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(directory.join(format!("{key}.lock")))
        .map_err(|error| error.to_string())?;
    let endpoint_path = directory.join(format!("{key}.endpoint.json"));
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            forward(&endpoint_path, &key, targets)?;
            return Ok(Relaunch::Forwarded);
        }
        Err(TryLockError::Error(error)) => return Err(error.to_string()),
    }
    let listener =
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|error| error.to_string())?;
    let token = uuid::Uuid::new_v4().to_string();
    let endpoint = Endpoint {
        version: 1,
        identity: key,
        port: listener
            .local_addr()
            .map_err(|error| error.to_string())?
            .port(),
        token: token.clone(),
    };
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut endpoint_file = options
        .open(endpoint_path)
        .map_err(|error| error.to_string())?;
    endpoint_file
        .write_all(&serde_json::to_vec(&endpoint).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;
    drop(endpoint_file);
    let state = LaunchRequestsState {
        shared: Arc::new(Shared::default()),
        _lock: Some(Arc::new(lock)),
        isolated: profile.is_some() || report.is_some(),
    };
    state
        .shared
        .queue
        .lock()
        .unwrap()
        .accept("initial", targets)?;
    let worker_state = state.clone();
    std::thread::Builder::new()
        .name("cad-launch-delivery".into())
        .spawn(move || serve(listener, worker_state, token))
        .map_err(|error| error.to_string())?;
    Ok(Relaunch::Primary(state))
}

pub fn enqueue(app: &tauri::AppHandle, targets: &[LaunchTarget]) -> Result<(), String> {
    let state = app.state::<LaunchRequestsState>();
    state
        .shared
        .queue
        .lock()
        .map_err(|_| "Launch queue unavailable")?
        .accept(&uuid::Uuid::new_v4().to_string(), targets)?;
    notify(&state.shared);
    Ok(())
}

#[tauri::command]
pub fn native_launch_pending(
    state: tauri::State<'_, LaunchRequestsState>,
) -> Result<Vec<PendingLaunchRequest>, String> {
    Ok(state
        .shared
        .queue
        .lock()
        .map_err(|_| "Launch queue unavailable")?
        .pending
        .iter()
        .cloned()
        .collect())
}

#[tauri::command]
pub fn native_launch_ack(
    state: tauri::State<'_, LaunchRequestsState>,
    id: u64,
) -> Result<(), String> {
    state
        .shared
        .queue
        .lock()
        .map_err(|_| "Launch queue unavailable")?
        .acknowledge(id);
    Ok(())
}

#[tauri::command]
pub fn native_new_window() -> Result<(), String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    Command::new(executable)
        .arg("--new-window")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn file(name: &str) -> LaunchTarget {
        LaunchTarget::from_argument(name, &std::env::temp_dir()).unwrap()
    }
    #[test]
    fn loopback_delivery_retains_work_before_a_webview_exists() {
        let state = LaunchRequestsState::default();
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let token = uuid::Uuid::new_v4().to_string();
        let worker_state = state.clone();
        let worker_token = token.clone();
        let worker = std::thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                receive_delivery(&mut stream, &worker_state, &worker_token).unwrap();
            }
        });
        for valid in [false, true] {
            let mut stream = TcpStream::connect(address).unwrap();
            let id = uuid::Uuid::new_v4().to_string();
            send_packet(
                &mut stream,
                &Delivery {
                    version: 1,
                    token: if valid { token.clone() } else { "wrong".into() },
                    id: id.clone(),
                    issued_ms: now_ms(),
                    targets: vec![file("part Ω.nbcad")],
                },
            )
            .unwrap();
            let receipt: Receipt = read_packet(&mut stream).unwrap();
            assert_eq!(receipt.id, id);
            assert_eq!(receipt.accepted, valid);
        }
        worker.join().unwrap();
        assert!(state.shared.app.lock().unwrap().is_none());
        let mut queue = state.shared.queue.lock().unwrap();
        assert_eq!(queue.pending.len(), 1);
        let id = queue.pending[0].id;
        queue.acknowledge(id);
        assert!(queue.pending.is_empty());
    }

    #[test]
    fn primary_lock_excludes_a_peer_and_releases_after_exit() {
        let path = std::env::temp_dir().join(format!("limo-launch-lock-{}", uuid::Uuid::new_v4()));
        let first = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        first.try_lock().unwrap();
        let peer = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        assert!(matches!(peer.try_lock(), Err(TryLockError::WouldBlock)));
        drop(first);
        peer.try_lock().unwrap();
        drop(peer);
        fs::remove_file(path).unwrap();
    }
    #[test]
    fn retained_queue_is_atomic_bounded_and_acknowledged() {
        let mut queue = Queue::default();
        queue
            .accept("first", &[file("part Ω.nbcad"), file("part Ω.nbcad")])
            .unwrap();
        assert_eq!(queue.pending.len(), 1);
        let first_id = queue.pending[0].id;
        queue.acknowledge(999);
        assert_eq!(queue.pending[0].id, first_id);
        queue.accept("first", &[file("other.nbcad")]).unwrap();
        assert_eq!(queue.pending.len(), 1, "delivery retry never opens twice");
        for index in 1..MAX_REQUESTS {
            queue
                .accept(
                    &format!("request{index}"),
                    &[file(&format!("part{index}.nbcad"))],
                )
                .unwrap();
        }
        assert!(queue.accept("overflow", &[file("overflow.nbcad")]).is_err());
        assert_eq!(queue.pending.len(), MAX_REQUESTS);
        queue.acknowledge(first_id);
        queue.accept("later", &[file("part Ω.nbcad")]).unwrap();
        assert_eq!(queue.pending.len(), MAX_REQUESTS);
    }
    #[test]
    fn diagnostic_and_explicit_new_windows_do_not_join_ordinary_windows() {
        assert!(reuse_enabled(false, false, false, false));
        assert!(!reuse_enabled(true, false, false, false));
        assert!(!reuse_enabled(false, false, true, false));
        assert!(!reuse_enabled(false, false, false, true));
        assert!(reuse_enabled(false, true, true, true));
        assert!(!reuse_enabled(true, true, true, true));
        let executable = Path::new("cad.exe");
        let ordinary = identity(executable, 100, 1, None, None);
        assert_ne!(
            ordinary,
            identity(Path::new("other/cad.exe"), 100, 1, None, None)
        );
        assert_ne!(ordinary, identity(executable, 100, 2, None, None));
        assert_ne!(
            ordinary,
            identity(
                executable,
                100,
                1,
                Some(std::ffi::OsStr::new("private")),
                None
            )
        );
    }
    #[test]
    fn authentication_age_and_paths_are_checked_before_delivery() {
        let token = uuid::Uuid::new_v4().to_string();
        let mut delivery = Delivery {
            version: 1,
            token: token.clone(),
            id: uuid::Uuid::new_v4().to_string(),
            issued_ms: 50_000,
            targets: vec![file("part.nbcad")],
        };
        assert!(delivery_valid(&delivery, &token, 50_001));
        assert!(!delivery_valid(&delivery, "wrong", 50_001));
        assert!(!delivery_valid(&delivery, &token, 90_000));
        delivery.targets = vec![LaunchTarget::File {
            path: "relative.nbcad".into(),
        }];
        let mut queue = Queue::default();
        assert!(queue.accept("invalid", &delivery.targets).is_err());
        assert!(queue.pending.is_empty());
        assert!(LaunchTarget::from_argument(
            "nbcad://recipe/fillet-basics?run=true",
            &std::env::temp_dir()
        )
        .is_err());
    }
}
