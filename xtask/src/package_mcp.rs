//! Exercise the shipped executable and its runtime libraries over the ordinary
//! Rust MCP client. Optional desktop checks exercise one isolated, owned window.
use crate::replay::Client;
use anyhow::{bail, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    io::{Cursor, Read},
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

mod lifecycle_evidence;

#[derive(Debug)]
struct Options {
    server: String,
    arguments: Vec<String>,
    out: Option<PathBuf>,
    timeout: Duration,
    desktop: bool,
}
impl Options {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Self> {
        let mut server = None;
        let mut arguments = Vec::new();
        let mut out = None;
        let mut timeout = None;
        let mut desktop = false;
        while let Some(option) = args.next() {
            if option == "--desktop" {
                ensure!(!desktop, "Duplicate --desktop option");
                desktop = true;
                continue;
            }
            let value = args
                .next()
                .with_context(|| format!("Missing value for {option}"))?;
            match option.as_str() {
                "--server" if server.is_none() => server = Some(value),

                "--server-arg" => arguments.push(value),
                "--out" if out.is_none() => out = Some(PathBuf::from(value)),
                "--timeout-seconds" if timeout.is_none() => {
                    let seconds: u64 = value.parse().context("timeout must be whole seconds")?;
                    ensure!(
                        (1..=600).contains(&seconds),
                        "timeout must be 1–600 seconds"
                    );
                    timeout = Some(Duration::from_secs(seconds));
                }
                _ => bail!("Unknown or duplicate option {option}"),
            }
        }
        if desktop {
            ensure!(arguments.iter().filter(|argument| *argument == "--headless").count() == 1
                && arguments.iter().all(|argument| matches!(argument.as_str(), "--headless" | "--appimage-extract-and-run")),
                "--desktop requires --server-arg --headless; only an optional AppImage runtime argument may accompany it");
        }
        Ok(Self {
            server: server.context("Supply --server PATH")?,
            arguments,
            out,
            timeout: timeout.unwrap_or(Duration::from_secs(120)),
            desktop,
        })
    }
}

struct SessionDirectory(PathBuf);
impl SessionDirectory {
    fn create() -> Result<Self> {
        let time = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = std::env::temp_dir().join(format!(
            "limo-cad-package-mcp-{}-{time}",
            std::process::id()
        ));
        limo_cad_session_storage::create_registry(&path)
            .context("Create isolated private package-check session directory")?;
        Ok(Self(path))
    }
    fn ensure_empty(&self) -> Result<()> {
        ensure!(
            fs::read_dir(&self.0)?.next().is_none(),
            "Headless MCP unexpectedly published desktop/session files under {}",
            self.0.display()
        );
        Ok(())
    }
}
impl Drop for SessionDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.0);
    }
}

pub fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut options = Options::parse(args)?;
    options.server = fs::canonicalize(&options.server)
        .context("Resolve packaged executable before changing child working directory")?
        .to_string_lossy()
        .into_owned();
    if let Some(parent) = options.out.as_ref().and_then(|path| path.parent()) {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).context("Create package-check report directory")?;
        }
    }
    let started = Instant::now();
    let result = verify(&options).and_then(|mut report| {
        if options.desktop {
            report["desktop"] = verify_desktop(&options)?;
        }
        Ok(report)
    });
    let report = match &result {
        Ok(report) => report.clone(),
        Err(error) => json!({"passed":false,"error":format!("{error:#}")}),
    };
    let mut report = report;
    report["executable"] = json!(options.server);
    report["arguments"] = json!(options.arguments);
    report["desktop_requested"] = json!(options.desktop);
    report["elapsed_ms"] = json!(started.elapsed().as_millis());
    if let Some(path) = options.out {
        fs::write(&path, serde_json::to_vec_pretty(&report)?)
            .with_context(|| format!("Write package-check report {}", path.display()))?;
    }
    println!("{}", serde_json::to_string_pretty(&report)?);
    result.map(|_| ())
}

fn package_command(
    options: &Options,
    sessions: &SessionDirectory,
    desktop: bool,
) -> Result<Command> {
    let mut command = Command::new(&options.server);
    command
        .args(
            options
                .arguments
                .iter()
                .filter(|arg| !desktop || *arg != "--headless"),
        )
        .current_dir(&sessions.0)
        .env("LIMO_CAD_SESSION_DIR", &sessions.0);
    if desktop {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT_PROFILE: AtomicU64 = AtomicU64::new(0);
        let profile = sessions.0.join(format!(
            "native-profile-{}",
            NEXT_PROFILE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&profile).context("Create isolated native configuration")?;
        command.env("LIMO_CAD_CONFIG_DIR", &profile);
        #[cfg(target_os = "linux")]
        for (name, directory) in [
            ("XDG_DATA_HOME", "data"),
            ("XDG_CACHE_HOME", "cache"),
            ("XDG_CONFIG_HOME", "config"),
        ] {
            let path = profile.join(directory);
            fs::create_dir(&path).context("Create isolated desktop XDG directory")?;
            command.env(name, path);
        }
    }

    if !desktop {
        command.env_remove("DISPLAY").env_remove("WAYLAND_DISPLAY");
    }
    for name in [
        "OCCT_ROOT",
        "LIMO_CAD_OCCT_LIB_DIR",
        "LIMO_CAD_PROJECT_ROOT",
        "LIMO_CAD_REPO_ROOT",
        "VCPKG_INSTALLED_DIR",
        "VCPKG_TARGET_TRIPLET",
        "DYLD_LIBRARY_PATH",
        "DYLD_FALLBACK_LIBRARY_PATH",
        "DYLD_INSERT_LIBRARIES",
        "LD_LIBRARY_PATH",
        "LD_PRELOAD",
    ] {
        command.env_remove(name);
    }
    #[cfg(windows)]
    {
        let root =
            PathBuf::from(std::env::var_os("SystemRoot").context("Windows SystemRoot is missing")?);
        command.env("PATH", std::env::join_paths([root.join("System32"), root])?);
    }
    #[cfg(not(windows))]
    command.env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin");
    Ok(command)
}

fn verify(options: &Options) -> Result<Value> {
    let sessions = SessionDirectory::create()?;
    let command = package_command(options, &sessions, false)?;
    let mut client = Client::start_command(command, Some(options.timeout))?;
    let initialization = client.initialization().clone();
    ensure!(
        initialization["protocolVersion"] == "2025-06-18",
        "Unexpected MCP protocol: {initialization}"
    );
    ensure!(
        initialization.pointer("/serverInfo/name") == Some(&json!("limo-cad")),
        "Unexpected MCP server: {initialization}"
    );
    ensure!(
        initialization.pointer("/capabilities/tools").is_some(),
        "MCP tools are not advertised"
    );
    let listing = client.rpc("tools/list", json!({}))?;
    let tools = listing["tools"]
        .as_array()
        .context("tools/list has no tool array")?;
    let mut names = HashSet::new();
    for tool in tools {
        let name = tool["name"].as_str().context("Tool has no name")?;
        ensure!(names.insert(name), "Duplicate tool {name}");
    }
    ensure!(
        names.contains("cad_interface"),
        "Canonical cad_interface tool is missing"
    );
    for legacy in ["cad_ui", "cad_view", "cad_launch"] {
        ensure!(
            !names.contains(legacy),
            "Retired interface alias is advertised: {legacy}"
        );
    }
    let catalog = client.call("cad_interface", json!({"action":"catalog"}))?;
    ensure!(
        catalog["groups"]
            .as_array()
            .is_some_and(|groups| !groups.is_empty()),
        "Product groups are missing"
    );
    let recipes = client.call("cad_interface", json!({"action":"recipes"}))?;
    let recipe = recipes
        .as_array()
        .context("Recipe catalog is not an array")?
        .iter()
        .find(|recipe| recipe["id"] == "fillet-basics")
        .context("The shipped first-part lesson is missing")?;
    let built = client.call(
        "cad_interface",
        json!({"action":"script","recipe":"fillet-basics","mode":"fast","validate":true}),
    )?;
    ensure!(
        built["steps_completed"] == recipe["step_count"]
            && built["checks_completed"] == recipe["check_count"],
        "The shipped lesson did not complete all construction and checks"
    );
    let model_text = client.call("cad_project_model", json!({}))?;
    let model: Value = serde_json::from_str(
        model_text
            .as_str()
            .context("Project-model export is not JSON text")?,
    )?;
    ensure!(
        model
            .pointer("/extrudes/0/extent/distance")
            .and_then(Value::as_f64)
            == Some(12.0),
        "The lesson did not retain its editable extrusion"
    );
    ensure!(
        model.pointer("/fillets/0/radius").and_then(Value::as_f64) == Some(2.0),
        "The lesson did not retain its editable fillet"
    );
    let sketches = client.call("sketch_finished", json!({}))?;
    ensure!(
        sketches.as_array().is_some_and(|items| items.len() == 1)
            && sketches.pointer("/0/dof/value") == Some(&json!(0)),
        "The lesson sketch is not fully constrained"
    );
    let scene = client.call("solid_scene", json!({}))?;
    ensure!(
        scene["errors"] == json!([]),
        "The lesson has geometry errors: {}",
        scene["errors"]
    );
    let bodies = scene["bodies"]
        .as_array()
        .context("The lesson has no body array")?;
    ensure!(bodies.len() == 1, "The lesson must produce one solid");
    let body_id = bodies[0]["id"].as_u64().context("Solid has no body ID")?;
    client.call(
        "set_body_appearance",
        json!({"body_id":body_id,"preset_id":"generic.pla.gray"}),
    )?;
    let exported = client.call(
        "solid_export_3mf",
        json!({"body_ids":[body_id],"slicer_target":"standard","include_appearance":true}),
    )?;
    let bytes = check_export(&exported)?;
    client.finish(Duration::from_secs(10))?;
    sessions.ensure_empty()?;
    Ok(
        json!({"passed":true,"initialization":initialization,"recipe":"fillet-basics",
        "steps_completed":built["steps_completed"],"checks_completed":built["checks_completed"],
        "fully_constrained_sketches":1,"solid_bodies":1,"export_3mf_bytes":bytes,
        "clean_eof_exit":true,"no_desktop_session":true}),
    )
}

/// A private registry and the owned child's PID must agree before any live
/// request is sent. Never select an arbitrary existing window by its label.
fn owned_window(sessions: &SessionDirectory, pid: u32) -> Result<Option<Value>> {
    let directory = sessions.0.join("_ui/processes");
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut leases: BTreeMap<String, (u64, Value)> = BTreeMap::new();
    for entry in entries {
        let entry = entry?;

        if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
            continue;
        }
        let Ok(body) = fs::read(entry.path()) else {
            continue;
        };
        let Ok(lease) = serde_json::from_slice::<Value>(&body) else {
            continue;
        };
        if lease["pid"].as_u64() != Some(u64::from(pid)) {
            continue;
        }
        let Some(instance) = lease["process_instance_id"]
            .as_str()
            .filter(|id| !id.is_empty())
        else {
            continue;
        };
        let Some(updated) = lease["updated_ms"].as_u64() else {
            continue;
        };

        if leases
            .get(instance)
            .is_none_or(|(previous, _)| *previous < updated)
        {
            leases.insert(instance.to_owned(), (updated, lease));
        }
    }
    ensure!(
        leases.len() <= 1,
        "Owned desktop PID published multiple process instances"
    );
    let Some((_, lease)) = leases.into_values().next() else {
        return Ok(None);
    };
    let mut windows = BTreeMap::new();
    for window in lease["windows"].as_array().into_iter().flatten() {
        if let (Some(window_id), Some(_)) = (
            window["window_id"].as_str(),
            window["active_session_id"].as_str(),
        ) {
            windows.insert(window_id.to_owned(), window.clone());
        }
    }
    ensure!(
        windows.len() <= 1,
        "Owned desktop published multiple active windows"
    );
    Ok(windows.into_values().next())
}

fn grouped_call(
    client: &mut Client,
    catalog: &Value,
    operation: &str,
    arguments: Value,
) -> Result<Value> {
    let group = catalog["groups"]
        .as_array()
        .context("Missing product groups")?
        .iter()
        .find(|group| {
            group["operations"]
                .as_array()
                .is_some_and(|operations| operations.iter().any(|name| name == operation))
        })
        .and_then(|group| group["id"].as_str())
        .with_context(|| format!("Missing product operation {operation}"))?;
    client.call(
        "cad_interface",
        json!({"action":"execute","group":group,"operation":operation,"arguments":arguments}),
    )
}

fn project_model(client: &mut Client) -> Result<Value> {
    let text = client.call("cad_project_model", json!({}))?;
    serde_json::from_str(text.as_str().context("Project model is not JSON text")?)
        .context("Read project model")
}

fn desktop_not_ready(reply: &Value) -> bool {
    if reply["isError"] != true {
        return false;
    }
    let Some(content) = reply["content"].as_array() else {
        return false;
    };
    let [message] = content.as_slice() else {
        return false;
    };
    if message["type"] != "text" {
        return false;
    }
    message["text"]
        .as_str()
        .and_then(|text| serde_json::from_str::<Value>(text).ok())
        .is_some_and(|error| error["code"] == "desktop_not_ready")
}

/// Keep startup failures useful on CI without copying a browser profile or an
/// entire CAD model. Read only this fixture's private publication directory.
fn startup_diagnostics(sessions: &SessionDirectory, pid: u32) -> Value {
    const MAX_ENTRIES: usize = 8;
    fn read_json(path: &Path) -> Value {
        const MAX_BYTES: u64 = 16 * 1024;
        let result = (|| -> Result<Value> {
            let mut bytes = Vec::new();
            fs::File::open(path)?
                .take(MAX_BYTES + 1)
                .read_to_end(&mut bytes)?;
            ensure!(
                bytes.len() as u64 <= MAX_BYTES,
                "Publication exceeds diagnostic byte limit"
            );
            Ok(serde_json::from_slice(&bytes)?)
        })();
        result.unwrap_or_else(|error| json!({"read_error":format!("{error:#}")}))
    }
    let registry_path = sessions.0.join("_ui/processes");
    let registry = fs::read_dir(&registry_path).map(|entries| {
        entries
            .take(MAX_ENTRIES)
            .map(|entry| match entry {
                Ok(entry) if entry.file_type().is_ok_and(|kind| kind.is_file()) => json!({
                    "file":entry.file_name().to_string_lossy(),
                    "publication":read_json(&entry.path()),
                }),
                Ok(entry) => json!({"file":entry.file_name().to_string_lossy(),"skipped":true}),
                Err(error) => json!({"read_error":error.to_string()}),
            })
            .collect::<Vec<_>>()
    });
    let registry = match registry {
        Ok(entries) => json!({"entries":entries,"entry_limit":MAX_ENTRIES}),
        Err(error) => json!({"read_error":error.to_string()}),
    };
    let documents: Vec<_> = fs::read_dir(&sessions.0)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name != "_ui"
                && !name.starts_with("native-profile-")
                && entry.file_type().is_ok_and(|kind| kind.is_dir())
        })
        .take(MAX_ENTRIES)
        .map(|entry| {
            json!({
                "session":entry.file_name().to_string_lossy(),
                "model_present":entry.path().join("model.json").is_file(),
                "heartbeat":read_json(&entry.path().join("heartbeat.json")),
            })
        })
        .collect();
    json!({"owned_pid":pid,"private_session_root":sessions.0,"registry":registry,
        "documents":documents,"entry_limit":MAX_ENTRIES})
}

#[cfg(any(target_os = "linux", test))]
mod stdout_diagnostics {
    use super::*;
    use std::collections::VecDeque;

    const MAX_PROCESSES: usize = 32;
    const MAX_THREADS: usize = 64;
    const MAX_FDS: usize = 256;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Identity {
        pid: u32,
        parent: u32,
        started: u64,
    }

    pub(super) struct Pipe {
        owner: Identity,
        target: PathBuf,
    }

    fn read_small(path: &Path) -> Result<String> {
        let mut text = String::new();
        fs::File::open(path)?.take(4097).read_to_string(&mut text)?;
        ensure!(
            text.len() <= 4096,
            "Process metadata exceeds diagnostic limit"
        );
        Ok(text)
    }

    fn identity(root: &Path, pid: u32) -> Result<Identity> {
        let text = read_small(&root.join(pid.to_string()).join("stat"))?;

        let (_, fields) = text.rsplit_once(')').context("Missing process identity")?;
        let fields: Vec<_> = fields.split_whitespace().collect();
        Ok(Identity {
            pid,
            parent: fields.get(1).context("Missing parent PID")?.parse()?,
            started: fields
                .get(19)
                .context("Missing process start time")?
                .parse()?,
        })
    }

    pub(super) fn capture(
        root: &Path,
        pid: u32,
        read_link: impl Fn(&Path) -> std::io::Result<PathBuf>,
    ) -> Result<Pipe> {
        let owner = identity(root, pid)?;
        let target = read_link(&root.join(pid.to_string()).join("fd/1"))?;
        ensure!(
            target
                .to_str()
                .and_then(|text| text.strip_prefix("pipe:["))
                .and_then(|text| text.strip_suffix(']'))
                .and_then(|inode| inode.parse::<u64>().ok())
                .is_some(),
            "Owned stdout did not identify a pipe"
        );
        ensure!(
            identity(root, pid)? == owner,
            "Owned process identity changed"
        );
        Ok(Pipe { owner, target })
    }

    pub(super) fn inspect(
        root: &Path,
        captured: &Result<Pipe>,
        read_link: impl Fn(&Path) -> std::io::Result<PathBuf>,
    ) -> Value {
        let pipe = match captured {
            Ok(pipe) => pipe,
            Err(error) => return json!({"capture_error":format!("{error:#}")}),
        };
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut pending = VecDeque::from([pipe.owner]);
        let mut seen = HashSet::from([pipe.owner.pid]);
        let mut writers = Vec::new();
        let mut skipped = Vec::new();
        let mut limited = false;
        let mut checked = 0;
        while let Some(process) = pending.pop_front() {
            if Instant::now() >= deadline {
                limited = true;
                break;
            }
            if identity(root, process.pid).ok() != Some(process) {
                skipped.push(json!({"pid":process.pid,"reason":"exited_or_identity_changed"}));
                continue;
            }
            checked += 1;
            let directory = root.join(process.pid.to_string());
            if let Ok(entries) = fs::read_dir(directory.join("fd")) {
                for (index, entry) in entries.enumerate() {
                    if index == MAX_FDS || Instant::now() >= deadline {
                        limited = true;
                        break;
                    }
                    let Ok(entry) = entry else { continue };
                    let Some(fd) = entry
                        .file_name()
                        .to_str()
                        .and_then(|fd| fd.parse::<u32>().ok())
                    else {
                        continue;
                    };
                    if read_link(&entry.path()).ok().as_ref() != Some(&pipe.target) {
                        continue;
                    }

                    let flags = read_small(&directory.join("fdinfo").join(fd.to_string()))
                        .ok()
                        .and_then(|text| {
                            text.lines().find_map(|line| {
                                line.strip_prefix("flags:")
                                    .map(str::trim)
                                    .map(str::to_owned)
                            })
                        })
                        .and_then(|flags| u32::from_str_radix(&flags, 8).ok());
                    if flags.is_none_or(|flags| flags & 3 != 0)
                        && identity(root, process.pid).ok() == Some(process)
                        && read_link(&entry.path()).ok().as_ref() == Some(&pipe.target)
                    {
                        writers.push(json!({"pid":process.pid,"parent_pid":process.parent,
                            "fd":fd,"flags_octal":flags.map(|flags| format!("{flags:o}")),
                            "writer":flags.map(|flags| flags & 3 != 0)}));
                    }
                }
            }

            if let Ok(threads) = fs::read_dir(directory.join("task")) {
                for (index, thread) in threads.enumerate() {
                    if index == MAX_THREADS || Instant::now() >= deadline {
                        limited = true;
                        break;
                    }
                    let Ok(thread) = thread else { continue };
                    let Ok(children) = read_small(&thread.path().join("children")) else {
                        continue;
                    };
                    for pid in children
                        .split_whitespace()
                        .filter_map(|pid| pid.parse::<u32>().ok())
                    {
                        if Instant::now() >= deadline {
                            limited = true;
                            break;
                        }
                        if seen.contains(&pid) {
                            continue;
                        }
                        if seen.len() == MAX_PROCESSES {
                            limited = true;
                            break;
                        }

                        if let Ok(child) = identity(root, pid) {
                            if child.parent == process.pid
                                && identity(root, process.pid).ok() == Some(process)
                            {
                                seen.insert(pid);
                                pending.push_back(child);
                            }
                        }
                    }
                }
            }
        }
        json!({"owned_pid":pipe.owner.pid,"original_stdout":pipe.target,
            "matching_writers":writers,"processes_checked":checked,"skipped":skipped,
            "limited":limited,"limits":{"processes":MAX_PROCESSES,"threads_per_process":MAX_THREADS,
                "fds_per_process":MAX_FDS,"elapsed_ms":2000}})
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn evidence_follows_only_current_owned_descendants_and_reports_its_bounds() {
            let sessions = SessionDirectory::create().unwrap();
            let root = &sessions.0;
            let process = |pid: u32, parent: u32, started: u64| {
                let directory = root.join(pid.to_string());
                fs::create_dir_all(directory.join("fd")).unwrap();
                fs::create_dir_all(directory.join("fdinfo")).unwrap();
                fs::create_dir_all(directory.join("task").join(pid.to_string())).unwrap();
                let mut fields = vec!["0".to_owned(); 20];
                fields[0] = "S".into();
                fields[1] = parent.to_string();
                fields[19] = started.to_string();
                fs::write(
                    directory.join("stat"),
                    format!("{pid} (private ) name) {}", fields.join(" ")),
                )
                .unwrap();
                directory
            };
            let fd = |directory: &Path, fd: u32, target: &str, flags: &str| {
                fs::write(directory.join("fd").join(fd.to_string()), target).unwrap();
                fs::write(
                    directory.join("fdinfo").join(fd.to_string()),
                    format!("flags:\t{flags}\n"),
                )
                .unwrap();
            };
            let owner = process(10, 1, 100);
            fd(&owner, 1, "pipe:[987]", "1");
            let child = process(11, 10, 101);
            fd(&child, 1, "pipe:[987]", "1");
            fd(&child, 2, "pipe:[987]", "0");
            fd(&child, 3, "pipe:[unrelated]", "1");
            let foreign = process(12, 999, 102);
            fd(&foreign, 1, "pipe:[987]", "1");
            fs::write(owner.join("task/10/children"), "11 12").unwrap();

            let read_link = |path: &Path| fs::read_to_string(path).map(PathBuf::from);
            let captured = capture(root, 10, read_link);
            fs::write(owner.join("fd/1"), "/dev/null").unwrap();
            let evidence = inspect(root, &captured, read_link);
            assert_eq!(
                evidence["matching_writers"],
                json!([{"pid":11,"parent_pid":10,"fd":1,"flags_octal":"1","writer":true}])
            );
            assert_eq!(evidence["processes_checked"], 2);
            assert!(!evidence.to_string().contains("private"));
            assert!(!evidence.to_string().contains("unrelated"));
            let children = (20..20 + MAX_PROCESSES as u32 + 2)
                .map(|pid| {
                    process(pid, 10, pid as u64);
                    pid.to_string()
                })
                .collect::<Vec<_>>()
                .join(" ");
            fs::write(owner.join("task/10/children"), children).unwrap();
            let limited = inspect(root, &captured, read_link);
            assert_eq!(limited["limited"], true);
            assert_eq!(limited["processes_checked"], MAX_PROCESSES);
            process(10, 1, 999);
            let recycled = inspect(root, &captured, read_link);
            assert_eq!(recycled["processes_checked"], 0);
            assert_eq!(recycled["matching_writers"], json!([]));
            fs::remove_dir_all(root).unwrap();
        }
    }
}

/// The process lease can appear before its matching heartbeat/model identity
/// finishes publishing. Retry that specific startup state, never modeling errors.
fn initial_project_model(client: &mut Client, timeout: Duration) -> Result<Value> {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        ensure!(
            !remaining.is_zero(),
            "Desktop initial document did not become ready before deadline"
        );
        ensure!(
            client.is_running()?,
            "Desktop exited before its initial document became ready"
        );
        let reply = client.rpc_with_timeout(
            "tools/call",
            json!({"name":"cad_project_model","arguments":{}}),
            remaining,
        )?;
        if !desktop_not_ready(&reply) {
            let text = Client::decode_call_result("cad_project_model", reply)?;
            return serde_json::from_str(text.as_str().context("Project model is not JSON text")?)
                .context("Read initial project model");
        }
        std::thread::sleep(
            Duration::from_millis(50).min(deadline.saturating_duration_since(Instant::now())),
        );
    }
}

fn wait_for_owned_window(
    desktop: &mut Client,
    sessions: &SessionDirectory,
    timeout: Duration,
) -> Result<Value> {
    let pid = desktop.process_id();
    eprintln!(
        "Verifying owned desktop PID {pid}; lifecycle evidence: {}",
        sessions.0.display()
    );
    let deadline = Instant::now() + timeout;
    loop {
        ensure!(desktop.is_running()?, "Desktop exited before UI readiness");
        if let Some(window) = owned_window(sessions, pid)? {
            let session = window["active_session_id"].as_str().unwrap();
            if sessions.0.join(session).join("model.json").is_file() {
                return Ok(window);
            }
        }
        if Instant::now() >= deadline {
            bail!(
                "Desktop did not publish an owned document before deadline; startup evidence: {}",
                startup_diagnostics(sessions, pid)
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn verify_desktop(options: &Options) -> Result<Value> {
    #[cfg(target_os = "macos")]
    ensure!(
        std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true")
            && std::env::var("RUNNER_ENVIRONMENT").as_deref() == Ok("github-hosted"),
        "--desktop on macOS requires a disposable GitHub-hosted runner"
    );
    let sessions = SessionDirectory::create()?;
    let result = verify_desktop_owned(options, &sessions);
    let retained = lifecycle_evidence::retain(&sessions.0, options.out.as_deref());
    match (result, retained) {
        (Ok(mut report), Ok(path)) => {
            report["lifecycle_evidence"] = json!(path);
            Ok(report)
        }
        (Err(error), Ok(path)) => Err(error.context(format!(
            "Owned desktop lifecycle evidence retained at {}",
            path.display()
        ))),
        (Ok(_), Err(error)) => Err(error.context("Retain desktop lifecycle evidence")),
        (Err(error), Err(retention)) => Err(error.context(format!(
            "Lifecycle evidence retention failed: {retention:#}; original session: {}",
            sessions.0.display()
        ))),
    }
}

fn verify_desktop_owned(options: &Options, sessions: &SessionDirectory) -> Result<Value> {
    let started = Instant::now();
    let command = package_command(options, sessions, true)?;
    let native_profile = command
        .get_envs()
        .find(|(name, _)| *name == "LIMO_CAD_CONFIG_DIR")
        .and_then(|(_, value)| value)
        .map(PathBuf::from)
        .context("Owned desktop command has no native profile")?;
    let mut desktop =
        lifecycle_evidence::start(command, options.timeout, &sessions.0, "default-desktop")?;
    let pid = desktop.process_id();
    #[cfg(target_os = "linux")]
    let original_stdout =
        stdout_diagnostics::capture(Path::new("/proc"), pid, |path| fs::read_link(path));
    let initialization = desktop.initialization().clone();
    let listing = desktop.rpc("tools/list", json!({}))?;
    ensure!(
        listing["tools"]
            .as_array()
            .is_some_and(|tools| tools.iter().any(|tool| tool["name"] == "cad_interface")),
        "Default desktop launch did not advertise MCP tools"
    );
    let catalog = desktop.call("cad_interface", json!({"action":"catalog"}))?;
    lifecycle_evidence::stage(&sessions.0, "waiting-default-desktop-window", Some(pid))?;
    let window = wait_for_owned_window(&mut desktop, sessions, options.timeout)?;
    let session = window["active_session_id"].as_str().unwrap();

    lifecycle_evidence::stage(&sessions.0, "modeling-default-desktop", Some(pid))?;
    let initial = initial_project_model(&mut desktop, options.timeout)?;
    ensure!(
        initial
            .pointer("/document/history/features")
            .and_then(Value::as_array)
            .is_some_and(Vec::is_empty),
        "Owned lifecycle fixture did not start with a blank document"
    );
    let inspected = desktop.call("cad_interface", json!({"action":"inspect"}))?;
    ensure!(
        inspected["status"] == "applied" && inspected["active_session_id"] == session,
        "Default stdio did not inspect its own visible session: {inspected}"
    );
    grouped_call(
        &mut desktop,
        &catalog,
        "sketch_begin",
        json!({"plane":{"type":"origin_plane","plane":"xy"}}),
    )?;
    let rectangle = grouped_call(
        &mut desktop,
        &catalog,
        "sketch_add_rectangle_locked",
        json!({"mode":"two_point","anchor":{"x":0,"y":0},"corner_hint":{"x":24,"y":16},"width_mm":24,"height_mm":16,"ctrl_held":true}),
    )?;

    let corners: Vec<_> = rectangle
        .pointer("/sketch/entities")
        .and_then(Value::as_array)
        .context("Rectangle response has no sketch entities")?
        .iter()
        .filter(|entity| {
            entity["kind"] == "point"
                && entity.pointer("/position/x").and_then(Value::as_f64) == Some(0.)
                && entity.pointer("/position/y").and_then(Value::as_f64) == Some(0.)
        })
        .collect();
    ensure!(
        corners.len() == 1,
        "Rectangle response did not locate exactly one origin corner: {rectangle}"
    );
    let corner = corners[0]["id"]
        .as_u64()
        .context("Rectangle corner has no entity ID")?;
    grouped_call(
        &mut desktop,
        &catalog,
        "sketch_add_constraint",
        json!({"type":"fix","entity":corner}),
    )?;
    grouped_call(&mut desktop, &catalog, "sketch_finish", json!({}))?;
    let sketches = desktop.call("sketch_finished", json!({}))?;
    ensure!(
        sketches.as_array().is_some_and(|items| items.len() == 1)
            && sketches.pointer("/0/dof/value") == Some(&json!(0)),
        "Default desktop stdio did not create one fully constrained editable sketch: {sketches}"
    );
    let saved_path = sessions.0.join("stdio-lifecycle.limo");
    let saved = desktop.call(
        "cad_interface",
        json!({"action":"file","command":"save","path":saved_path}),
    )?;
    ensure!(
        saved["status"] == "applied" && fs::metadata(&saved_path)?.len() > 0,
        "Lifecycle fixture was not saved through the normal file operation: {saved}"
    );
    let saved_model = project_model(&mut desktop)?;
    ensure!(
        saved_model["sketches"]
            .as_array()
            .is_some_and(|sketches| sketches.len() == 1),
        "Saved document did not retain the sketch"
    );
    let saved_bytes = fs::read(&saved_path)?;
    grouped_call(
        &mut desktop,
        &catalog,
        "cad_set_document_name",
        json!({"name":"stdio-lifecycle-unsaved"}),
    )?;
    let unsaved_model = project_model(&mut desktop)?;
    ensure!(
        unsaved_model != saved_model
            && unsaved_model.pointer("/document/name") == Some(&json!("stdio-lifecycle-unsaved")),
        "Fixture did not create an unsaved edit after Save"
    );
    lifecycle_evidence::stage(&sessions.0, "disconnecting-default-stdio", Some(pid))?;
    desktop.close_input();
    let stdout_eof = desktop.require_stdout_eof(Duration::from_secs(10));
    #[cfg(target_os = "linux")]
    let stdout_eof = stdout_eof.with_context(|| {
        format!(
            "Owned stdout EOF evidence: {}",
            stdout_diagnostics::inspect(Path::new("/proc"), &original_stdout, |path| {
                fs::read_link(path)
            })
        )
    });
    stdout_eof?;
    let survival_deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < survival_deadline {
        ensure!(
            desktop.is_running()?,
            "Closing agent stdin terminated the visible CAD app"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    let mut observer = lifecycle_evidence::start(
        package_command(options, sessions, false)?,
        options.timeout,
        &sessions.0,
        "headless-observer",
    )?;
    observer.call("cad_attach", json!({"session_id":session}))?;
    let after_eof = observer.call(
        "cad_interface",
        json!({"action":"inspect","session_id":session}),
    )?;
    ensure!(
        after_eof["status"] == "applied" && after_eof["active_session_id"] == session,
        "Saved desktop stopped responding after its stdio disconnected: {after_eof}"
    );
    ensure!(
        project_model(&mut observer)? == unsaved_model && fs::read(&saved_path)? == saved_bytes,
        "Unsaved desktop stdio edits were lost or silently saved during disconnect"
    );
    let retained_path = sessions.0.join("stdio-lifecycle-unsaved.limo");
    let saved_after_eof = observer.call(
        "cad_interface",
        json!({"action":"file","command":"save","path":retained_path,"session_id":session}),
    )?;
    ensure!(
        saved_after_eof["status"] == "applied" && fs::metadata(&retained_path)?.len() > 0,
        "Observer could not save the retained unsaved model after disconnect: {saved_after_eof}"
    );
    lifecycle_evidence::stage(&sessions.0, "awaiting-observer-close-receipt", Some(pid))?;
    let closed = observer.call(
        "cad_interface",
        json!({"action":"window","mode":"close","session_id":session}),
    )?;
    ensure!(
        clean_native_close_receipt(&closed, session),
        "Normal guarded window close was not acknowledged: {closed}"
    );
    lifecycle_evidence::stage(
        &sessions.0,
        "awaiting-observer-close-process-exit",
        Some(pid),
    )?;
    desktop
        .finish(Duration::from_secs(10))
        .context("Finish the first desktop after observer close")?;
    observer
        .finish(Duration::from_secs(10))
        .context("Finish the headless lifecycle observer")?;

    lifecycle_evidence::stage(&sessions.0, "starting-self-close-desktop", None)?;
    let mut self_closing = lifecycle_evidence::start(
        package_command(options, sessions, true)?,
        options.timeout,
        &sessions.0,
        "self-close-desktop",
    )?;
    let self_pid = self_closing.process_id();
    lifecycle_evidence::stage(&sessions.0, "waiting-self-close-window", Some(self_pid))?;
    let self_close_window = wait_for_owned_window(&mut self_closing, sessions, options.timeout)
        .context("Wait for the second owned desktop window")?;
    lifecycle_evidence::stage(
        &sessions.0,
        "reading-self-close-blank-document",
        Some(self_pid),
    )?;
    let empty_model = initial_project_model(&mut self_closing, options.timeout)
        .context("Read the second desktop's blank document before close")?;
    ensure!(
        empty_model
            .pointer("/document/history/features")
            .and_then(Value::as_array)
            .is_some_and(Vec::is_empty),
        "Self-close fixture did not start with a blank document"
    );
    lifecycle_evidence::stage(&sessions.0, "awaiting-self-close-receipt", Some(self_pid))?;
    let self_closed = self_closing.call("cad_interface", json!({"action":"window","mode":"close"}))
        .with_context(|| format!("Receive the second desktop's self-close receipt (PID {self_pid}, child_running={:?})", self_closing.is_running()))?;
    ensure!(
        clean_native_close_receipt(
            &self_closed,
            self_close_window["active_session_id"].as_str().unwrap()
        ),
        "The desktop exited before its own stdio acknowledged guarded close: {self_closed}"
    );
    lifecycle_evidence::stage(
        &sessions.0,
        "awaiting-self-close-process-exit",
        Some(self_pid),
    )?;
    self_closing
        .finish(Duration::from_secs(10))
        .context("Finish the second desktop after its self-close receipt")?;
    lifecycle_evidence::stage(&sessions.0, "complete", Some(self_pid))?;
    Ok(
        json!({"passed":true,"pid":pid,"window":window,"initialization":initialization,
        "baseline_project":saved_path,"saved_project":retained_path,"saved_model":unsaved_model,"session_directory":sessions.0,"native_profile":native_profile,"elapsed_ms":started.elapsed().as_millis(),
        "default_stdio":true,"automatic_live_document_binding":true,"fully_constrained_sketches":1,
        "survived_stdio_eof":true,"stdout_eof_before_gui_exit":true,"retained_unsaved_model":true,"retained_live_model":true,"guarded_close":true,"clean_exit_and_stdout":true,
        "self_stdio_close_acknowledged":true,"self_close_window":self_close_window}),
    )
}

fn clean_native_close_receipt(response: &Value, session: &str) -> bool {
    !session.is_empty()
        && response["status"] == "applied"
        && response["active_session_id"] == session
        && response["session_id"] == session
        && response["awaiting_input"] == false
        && response.pointer("/value/awaiting_input") == Some(&json!(false))
}

fn check_export(exported: &Value) -> Result<usize> {
    ensure!(
        exported["format"] == "3mf" && exported["encoding"] == "base64",
        "Unexpected export encoding"
    );
    let encoded = exported["bytes_base64"]
        .as_str()
        .context("3MF export has no bytes")?;
    ensure!(
        encoded.len() < 8 * 1024 * 1024,
        "First-part 3MF unexpectedly exceeds 8 MiB"
    );
    let bytes = STANDARD.decode(encoded).context("Invalid 3MF base64")?;
    ensure!(
        exported["byte_length"].as_u64() == Some(bytes.len() as u64),
        "3MF byte count mismatch"
    );
    let mut archive =
        zip::ZipArchive::new(Cursor::new(&bytes)).context("3MF is not a readable ZIP")?;
    for name in ["[Content_Types].xml", "_rels/.rels"] {
        archive
            .by_name(name)
            .with_context(|| format!("3MF is missing {name}"))?;
    }
    let mut model = archive
        .by_name("3D/3dmodel.model")
        .context("3MF model part is missing")?;
    ensure!(
        model.size() < 8 * 1024 * 1024,
        "First-part 3MF model is unexpectedly large"
    );
    let mut xml = String::new();
    model
        .read_to_string(&mut xml)
        .context("Read 3MF model XML")?;
    for fragment in [
        "unit=\"millimeter\"",
        "<vertex ",
        "<triangle ",
        "<item ",
        "<basematerials",
        "#B4B4B4",
    ] {
        ensure!(
            xml.contains(fragment),
            "3MF lacks expected geometry/material data: {fragment}"
        );
    }
    Ok(bytes.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_close_receipt_requires_clean_matching_document() {
        let accepted = json!({"status":"applied","active_session_id":"owned", "session_id":"owned",
            "awaiting_input":false,"value":{"awaiting_input":false}});
        assert!(clean_native_close_receipt(&accepted, "owned"));
        assert!(!clean_native_close_receipt(&accepted, "other"));
        assert!(!clean_native_close_receipt(&accepted, ""));
        for (pointer, value) in [
            ("/status", json!("failed")),
            ("/active_session_id", json!("foreign")),
            ("/session_id", json!("foreign")),
            ("/awaiting_input", json!(true)),
            ("/value/awaiting_input", json!(true)),
            ("/value/awaiting_input", Value::Null),
        ] {
            let mut rejected = accepted.clone();
            *rejected.pointer_mut(pointer).unwrap() = value;
            assert!(
                !clean_native_close_receipt(&rejected, "owned"),
                "{rejected}"
            );
        }
        assert!(!clean_native_close_receipt(
            &json!({"status":"applied", "active_session_id":"owned",
            "window":{"close_requested":true}}),
            "owned"
        ));
    }

    #[test]
    fn initial_readiness_retry_requires_the_exact_structured_tool_error() {
        let reply = |text: &str| json!({"isError":true,"content":[{"type":"text","text":text}]});
        assert!(desktop_not_ready(&reply(
            r#"{"code":"desktop_not_ready","writeback":false}"#
        )));
        for text in [
            "desktop_not_ready",
            r#"{"code":"model_invalid","hint":"desktop_not_ready"}"#,
            r#"{"code":"desktop_not_ready_later"}"#,
            "not JSON",
        ] {
            assert!(!desktop_not_ready(&reply(text)));
        }
        let mut success = reply(r#"{"code":"desktop_not_ready"}"#);
        success["isError"] = json!(false);
        assert!(!desktop_not_ready(&success));
        let mut multiple = reply(r#"{"code":"desktop_not_ready"}"#);
        multiple["content"]
            .as_array_mut()
            .unwrap()
            .push(json!({"type":"text","text":"another failure"}));
        assert!(!desktop_not_ready(&multiple));
    }

    #[test]
    fn server_arguments_keep_flag_values_and_reject_bad_timeout() {
        let options = Options::parse(
            [
                "--server",
                "desktop",
                "--server-arg",
                "--headless",
                "--server-arg",
                "literal",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap();
        assert_eq!(options.arguments, ["--headless", "literal"]);
        assert!(Options::parse(
            ["--server", "desktop", "--timeout-seconds", "0"]
                .into_iter()
                .map(str::to_owned)
        )
        .is_err());
    }

    #[test]
    fn export_receipt_rejects_wrong_length_and_non_zip_payloads() {
        let mut reply = json!({"format":"3mf","encoding":"base64","bytes_base64":STANDARD.encode("not a zip"),"byte_length":9});
        assert!(check_export(&reply)
            .unwrap_err()
            .to_string()
            .contains("readable ZIP"));
        reply["byte_length"] = json!(10);
        assert!(check_export(&reply)
            .unwrap_err()
            .to_string()
            .contains("byte count"));
    }

    #[test]
    fn desktop_verification_requires_headless_worker_and_rejects_ignored_app_flags() {
        let parse = |args: &[&str]| Options::parse(args.iter().map(|arg| (*arg).to_owned()));
        assert!(parse(&["--server", "cad", "--desktop"]).is_err());
        assert!(parse(&[
            "--server",
            "cad",
            "--desktop",
            "--server-arg",
            "--headless",
            "--desktop"
        ])
        .is_err());
        assert!(parse(&[
            "--server",
            "cad",
            "--desktop",
            "--server-arg",
            "--headless",
            "--server-arg",
            "other"
        ])
        .is_err());
        let options = parse(&[
            "--server",
            "cad",
            "--desktop",
            "--server-arg",
            "--appimage-extract-and-run",
            "--server-arg",
            "--headless",
        ])
        .unwrap();
        let sessions = SessionDirectory::create().unwrap();
        let visible = package_command(&options, &sessions, true).unwrap();
        assert_eq!(
            visible.get_args().collect::<Vec<_>>(),
            ["--appimage-extract-and-run"]
        );
        let worker = package_command(&options, &sessions, false).unwrap();
        assert_eq!(
            worker.get_args().collect::<Vec<_>>(),
            ["--appimage-extract-and-run", "--headless"]
        );
    }

    #[test]
    fn desktop_owner_selection_requires_exact_child_and_one_active_window() {
        let sessions = SessionDirectory::create().unwrap();
        let registry = sessions.0.join("_ui/processes");
        fs::create_dir_all(&registry).unwrap();
        let foreign = registry.join("foreign.json");
        let owned = registry.join("owned.json");
        let publishing = registry.join("owned.json.tmp");
        let lease = |pid, instance, updated, windows| json!({"pid":pid,"process_instance_id":instance,"updated_ms":updated,"windows":windows});
        let window = |id, session| json!({"window_id":id,"active_session_id":session});
        fs::write(
            &foreign,
            lease(11, "foreign", 1, json!([window("main", "foreign")])).to_string(),
        )
        .unwrap();
        assert!(owned_window(&sessions, 12).unwrap().is_none());
        fs::write(
            &owned,
            lease(
                12,
                "own",
                1,
                json!([window("main", "own"), window("main", "own")]),
            )
            .to_string(),
        )
        .unwrap();
        assert_eq!(
            owned_window(&sessions, 12).unwrap().unwrap()["active_session_id"],
            "own"
        );
        fs::copy(&owned, &publishing).unwrap();
        assert_eq!(
            owned_window(&sessions, 12).unwrap().unwrap()["active_session_id"],
            "own",
            "Atomic temp and final copies are one process/window"
        );
        fs::write(
            &publishing,
            lease(12, "own", 2, json!([window("main", "new")])).to_string(),
        )
        .unwrap();
        assert_eq!(
            owned_window(&sessions, 12).unwrap().unwrap()["active_session_id"],
            "new",
            "Newest publication of the same instance wins"
        );
        fs::write(
            &publishing,
            lease(
                12,
                "another-instance",
                3,
                json!([window("main", "foreign")]),
            )
            .to_string(),
        )
        .unwrap();
        assert!(
            owned_window(&sessions, 12).is_err(),
            "A distinct process instance must remain ambiguous even with the same PID"
        );
        fs::remove_file(publishing).unwrap();
        fs::write(
            &owned,
            lease(
                12,
                "own",
                4,
                json!([window("main", "own"), window("second", "ambiguous")]),
            )
            .to_string(),
        )
        .unwrap();
        assert!(owned_window(&sessions, 12).is_err());
        fs::remove_file(owned).unwrap();
        fs::remove_file(foreign).unwrap();
        fs::remove_dir(registry).unwrap();
        fs::remove_dir(sessions.0.join("_ui")).unwrap();
    }

    #[test]
    fn startup_evidence_distinguishes_missing_registry_foreign_pid_and_unpublished_model() {
        let sessions = SessionDirectory::create().unwrap();
        assert!(startup_diagnostics(&sessions, 12)["registry"]["read_error"].is_string());
        let registry = sessions.0.join("_ui/processes");
        fs::create_dir_all(&registry).unwrap();
        let lease = registry.join("foreign.json");
        fs::write(&lease, json!({"pid":11,"windows":[]}).to_string()).unwrap();
        let document = sessions.0.join("own-session");
        fs::create_dir(&document).unwrap();
        fs::write(
            document.join("heartbeat.json"),
            json!({"session_id":"own-session"}).to_string(),
        )
        .unwrap();
        let evidence = startup_diagnostics(&sessions, 12);
        assert_eq!(evidence["owned_pid"], 12);
        assert_eq!(evidence["registry"]["entries"][0]["publication"]["pid"], 11);
        assert_eq!(evidence["documents"][0]["model_present"], false);
        assert_eq!(
            evidence["documents"][0]["heartbeat"]["session_id"],
            "own-session"
        );
        fs::write(document.join("model.json"), "private model contents").unwrap();
        fs::write(&lease, vec![b'x'; 16 * 1024 + 1]).unwrap();
        let evidence = startup_diagnostics(&sessions, 12);
        assert_eq!(evidence["documents"][0]["model_present"], true);
        assert!(
            evidence["registry"]["entries"][0]["publication"]["read_error"]
                .as_str()
                .unwrap()
                .contains("byte limit")
        );
        assert!(!evidence.to_string().contains("private model contents"));
        fs::remove_file(document.join("model.json")).unwrap();
        fs::remove_file(document.join("heartbeat.json")).unwrap();
        fs::remove_dir(document).unwrap();
        fs::remove_file(lease).unwrap();
        fs::remove_dir(registry).unwrap();
        fs::remove_dir(sessions.0.join("_ui")).unwrap();
    }

    #[test]
    fn desktop_cases_use_distinct_private_native_profiles() {
        let options = Options::parse(
            ["--server", "cad", "--server-arg", "--headless", "--desktop"]
                .into_iter()
                .map(str::to_owned),
        )
        .unwrap();
        let sessions = SessionDirectory::create().unwrap();
        let first = package_command(&options, &sessions, true).unwrap();
        let second = package_command(&options, &sessions, true).unwrap();
        for command in [&first, &second] {
            for (key, path) in command
                .get_envs()
                .filter_map(|(key, value)| value.map(|value| (key, value)))
            {
                if matches!(
                    key.to_str(),
                    Some(
                        "LIMO_CAD_CONFIG_DIR"
                            | "XDG_DATA_HOME"
                            | "XDG_CACHE_HOME"
                            | "XDG_CONFIG_HOME"
                    )
                ) {
                    assert!(
                        Path::new(path).is_dir(),
                        "Native settings and xdg-mime need {key:?} to exist before launch"
                    );
                }
            }
        }
        let key = "LIMO_CAD_CONFIG_DIR";
        let profile = |command: &Command| {
            command
                .get_envs()
                .find(|(name, _)| *name == key)
                .and_then(|(_, value)| value)
                .map(PathBuf::from)
                .unwrap()
        };
        let first = profile(&first);
        let second = profile(&second);
        assert!(first.starts_with(&sessions.0));
        assert!(second.starts_with(&sessions.0));
        assert_ne!(
            first, second,
            "Sequential GUI cases must not recover one another's state"
        );
    }
}
