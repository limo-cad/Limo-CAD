use anyhow::{anyhow, bail, ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

const DEFAULT_INITIALIZATION_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) struct Client {
    child: Child,
    input: Option<ChildStdin>,
    replies: Receiver<Result<Value, String>>,
    id: u64,
    request_timeout: Option<Duration>,
    initialization: Value,
}
impl Client {
    pub(crate) fn worker_command(executable: impl AsRef<std::ffi::OsStr>) -> Command {
        let mut command = Command::new(executable);
        command.arg("--headless");
        command
    }
    pub(crate) fn start_worker(executable: &str) -> Result<Self> {
        Self::start_command(Self::worker_command(executable), None)
    }
    fn start_with_arguments(
        executable: &str,
        arguments: &[String],
        initialization_timeout: Duration,
    ) -> Result<Self> {
        let mut command = Command::new(executable);
        command.args(arguments);
        Self::start_with_timeouts(command, initialization_timeout, None)
    }
    /// Package checks keep their per-request deadline, including initialization.
    /// Replays bound initialization separately from long modeling operations.
    pub(crate) fn start_command(
        command: Command,
        request_timeout: Option<Duration>,
    ) -> Result<Self> {
        Self::start_with_timeouts(
            command,
            request_timeout.unwrap_or(DEFAULT_INITIALIZATION_TIMEOUT),
            request_timeout,
        )
    }
    fn start_with_timeouts(
        command: Command,
        initialization_timeout: Duration,
        request_timeout: Option<Duration>,
    ) -> Result<Self> {
        Self::start_with_logs(command, initialization_timeout, request_timeout, None)
    }
    /// Retain only the explicitly owned fixture child's output. Stdout remains
    /// the same parsed MCP transport; the copy must never replace that pipe.
    pub(crate) fn start_command_logged(
        command: Command,
        request_timeout: Option<Duration>,
        directory: &Path,
    ) -> Result<Self> {
        let stdout = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join("host-stdout.jsonl"))?;
        let stderr = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join("host-stderr.log"))?;
        Self::start_with_logs(
            command,
            request_timeout.unwrap_or(DEFAULT_INITIALIZATION_TIMEOUT),
            request_timeout,
            Some((stdout, stderr)),
        )
    }
    fn start_with_logs(
        mut command: Command,
        initialization_timeout: Duration,
        request_timeout: Option<Duration>,
        logs: Option<(fs::File, fs::File)>,
    ) -> Result<Self> {
        crate::deploy_native::configure_runtime_environment(&mut command)?;
        let (mut stdout_log, stderr) = match logs {
            Some((stdout, stderr)) => (Some(stdout), Stdio::from(stderr)),
            None => (None, Stdio::inherit()),
        };
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(stderr);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn().context("Start MCP server")?;
        let input = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        let (sender, replies) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                if let (Ok(line), Some(log)) = (&line, stdout_log.as_mut()) {
                    if let Err(error) = writeln!(log, "{line}").and_then(|_| log.flush()) {
                        let _ = sender.send(Err(format!("Retain owned host stdout: {error}")));
                        break;
                    }
                }
                let result = line
                    .map_err(|e| e.to_string())
                    .and_then(|s| parse_reply_line(&s));
                if sender.send(result).is_err() {
                    break;
                }
            }
        });
        let mut client = Self {
            child,
            input: Some(input),
            replies,
            id: 0,
            request_timeout: Some(initialization_timeout),
            initialization: Value::Null,
        };
        client.initialization = client.rpc("initialize",json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"limo-cad-rust-replay","version":"1"}}))
            .with_context(|| format!(
                "MCP initialization failed (deadline {initialization_timeout:?}). The executable must expose stdio MCP; use --server-arg --headless for packaged CAD workers without a window"
            ))?;
        if !client.initialization["protocolVersion"].is_string()
            || !client.initialization["capabilities"].is_object()
            || !client.initialization["serverInfo"]["name"].is_string()
            || !client.initialization["serverInfo"]["version"].is_string()
        {
            bail!(
                "Invalid MCP initialization result: {}",
                client.initialization
            );
        }
        let input = client.input.as_mut().unwrap();
        writeln!(
            input,
            "{}",
            json!({"jsonrpc":"2.0","method":"notifications/initialized"})
        )?;
        input.flush()?;

        client.request_timeout = request_timeout;
        Ok(client)
    }
    pub(crate) fn initialization(&self) -> &Value {
        &self.initialization
    }
    pub(crate) fn process_id(&self) -> u32 {
        self.child.id()
    }
    pub(crate) fn close_input(&mut self) {
        drop(self.input.take());
    }
    pub(crate) fn is_running(&mut self) -> Result<bool> {
        Ok(self.child.try_wait()?.is_none())
    }
    /// Observe transport EOF separately from process exit: a visible app must
    /// retire its agent's pipe while preserving the user's open document.
    pub(crate) fn require_stdout_eof(&mut self, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                bail!("MCP stdout did not close before its EOF deadline");
            }
            match self.replies.recv_timeout(remaining) {
                Ok(Ok(reply)) if reply.get("method").is_some() => {}
                Ok(Ok(reply)) => bail!("Unexpected MCP response after final request: {reply}"),
                Ok(Err(error)) => bail!("Invalid MCP output before EOF: {error}"),
                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    bail!("MCP stdout did not close before its EOF deadline")
                }
            }
        }
    }
    pub(crate) fn rpc(&mut self, method: &str, params: Value) -> Result<Value> {
        let response = self.rpc_response(method, params)?;
        if let Some(error) = response.get("error") {
            if self.request_timeout.is_some() {
                self.terminate();
            }
            bail!("MCP error: {error}");
        }
        Ok(response["result"].clone())
    }
    /// Preserve a valid server error response without treating it as a broken pipe.
    fn rpc_response(&mut self, method: &str, params: Value) -> Result<Value> {
        let deadline = self.request_timeout.map(|timeout| Instant::now() + timeout);
        self.id += 1;
        let id = self.id;
        let input = self.input.as_mut().context("MCP input is closed")?;
        writeln!(
            input,
            "{}",
            json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
        )?;
        input.flush()?;
        let response = self.receive_reply(id, deadline, method);
        if response.is_err() && self.request_timeout.is_some() {
            self.terminate();
        }
        response
    }
    pub(crate) fn rpc_with_timeout(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value> {
        let previous = self.request_timeout.replace(timeout);
        let result = self.rpc(method, params);
        self.request_timeout = previous;
        result
    }
    fn receive_reply(&mut self, id: u64, deadline: Option<Instant>, method: &str) -> Result<Value> {
        loop {
            let wait = reply_wait(deadline, method)?;
            match self.replies.recv_timeout(wait) {
                Ok(Ok(reply)) => {
                    if reply["id"] != id {
                        continue;
                    }
                    ensure!(
                        reply.get("result").is_some() != reply.get("error").is_some(),
                        "Invalid MCP reply: exactly one of result or error is required"
                    );
                    if let Some(error) = reply.get("error") {
                        ensure!(
                            error["code"].is_i64() && error["message"].is_string(),
                            "Invalid MCP error response: {error}"
                        );
                    }
                    return Ok(reply);
                }
                Ok(Err(error)) => bail!("Invalid MCP reply: {error}"),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    reply_wait(deadline, method)?;
                    if let Some(status) = self.child.try_wait()? {
                        bail!("MCP exited: {status}");
                    }
                    eprintln!("Replay is running; live playback controls remain available.");
                }
                Err(error) => bail!("MCP connection closed: {error}"),
            }
        }
    }
    /// Closing the agent's input must let the stdio server exit normally. Drop
    /// still kills/reaps this owned child on any failure, including a deadline.
    pub(crate) fn finish(mut self, timeout: Duration) -> Result<()> {
        self.close_input();
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.child.try_wait()? {
                if !status.success() {
                    bail!("MCP exited unsuccessfully after EOF: {status}");
                }

                return self.require_stdout_eof(deadline.saturating_duration_since(Instant::now()));
            }
            if Instant::now() >= deadline {
                bail!("MCP did not exit within {timeout:?} after EOF");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn terminate(&mut self) {
        drop(self.input.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
    pub(crate) fn call(&mut self, name: &str, args: Value) -> Result<Value> {
        let result = self.rpc("tools/call", json!({"name":name,"arguments":args}))?;
        Self::decode_call_result(name, result)
    }
    pub(crate) fn decode_call_result(name: &str, result: Value) -> Result<Value> {
        if result["isError"] == true {
            bail!("{name}: {}", result["content"]);
        }
        let text = result["content"]
            .as_array()
            .and_then(|a| a.iter().find(|v| v["type"] == "text"))
            .and_then(|v| v["text"].as_str())
            .ok_or_else(|| anyhow!("MCP response has no text"))?;
        let result: Value = serde_json::from_str(text)?;
        if result["status"] == "failed" {
            bail!("{name}: {result}");
        }
        Ok(result)
    }
}
fn parse_reply_line(line: &str) -> Result<Value, String> {
    let reply: Value = serde_json::from_str(line).map_err(|error| error.to_string())?;
    if reply["jsonrpc"] != "2.0"
        || !(reply["method"].is_string()
            || reply.get("id").is_some()
                && (reply.get("result").is_some() || reply.get("error").is_some()))
    {
        return Err("stdout must contain JSON-RPC responses or notifications only".into());
    }
    Ok(reply)
}
fn reply_wait(deadline: Option<Instant>, method: &str) -> Result<Duration> {
    match deadline {
        Some(deadline) => {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                bail!("MCP request {method} exceeded its deadline");
            }
            Ok(remaining.min(Duration::from_secs(30)))
        }
        None => Ok(Duration::from_secs(30)),
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        self.terminate();
    }
}

#[derive(Debug)]
struct Options {
    values: HashMap<String, String>,
    file: Option<String>,
    server_arguments: Vec<String>,
}
impl Options {
    fn initialization_timeout(&self) -> Result<Duration> {
        match self.values.get("--init-timeout-seconds") {
            Some(value) => {
                let seconds: u64 = value
                    .parse()
                    .context("--init-timeout-seconds must be whole seconds")?;
                if !(1..=600).contains(&seconds) {
                    bail!("--init-timeout-seconds must be 1–600 seconds");
                }
                Ok(Duration::from_secs(seconds))
            }
            None => Ok(DEFAULT_INITIALIZATION_TIMEOUT),
        }
    }
}

fn options(args: impl Iterator<Item = String>) -> Result<Options> {
    let mut args = args.peekable();
    let mut values = HashMap::new();
    let mut file = None;
    let mut server_arguments = Vec::new();
    while let Some(mut arg) = args.next() {
        if arg == "-h" {
            arg = "--help".into();
        }
        if arg == "--server-arg" {
            server_arguments.push(
                args.next()
                    .ok_or_else(|| anyhow!("Missing value for --server-arg"))?,
            );
        } else if arg.starts_with("--") {
            if values.contains_key(&arg) {
                bail!("Duplicate option {arg}");
            }
            if matches!(
                arg.as_str(),
                "--present" | "--new" | "--help" | "--interactive"
            ) {
                values.insert(arg, "true".into());
            } else {
                values.insert(
                    arg.clone(),
                    args.next()
                        .filter(|v| !v.starts_with("--"))
                        .ok_or_else(|| anyhow!("Missing value for {arg}"))?,
                );
            }
        } else if file.replace(arg).is_some() {
            bail!("Only one script path is accepted");
        }
    }
    Ok(Options {
        values,
        file,
        server_arguments,
    })
}
fn known_options(args: &HashMap<String, String>, allowed: &[&str]) -> Result<()> {
    for key in args.keys() {
        if !allowed.contains(&key.as_str()) {
            bail!("Unknown option {key}");
        }
    }
    Ok(())
}
fn required<'a>(args: &'a HashMap<String, String>, name: &str) -> Result<&'a str> {
    args.get(name)
        .map(String::as_str)
        .ok_or_else(|| anyhow!("Missing {name}"))
}
pub fn call(args: impl Iterator<Item = String>) -> Result<()> {
    let options = options(args)?;
    if options.values.contains_key("--help") {
        print_usage(false);
        return Ok(());
    }
    let initialization_timeout = options.initialization_timeout()?;
    let args = &options.values;
    known_options(
        args,
        &[
            "--server",
            "--init-timeout-seconds",
            "--session",
            "--tool",
            "--args",
            "--args-file",
            "--out",
            "--interactive",
        ],
    )?;
    if options.file.is_some() || args.contains_key("--args") && args.contains_key("--args-file") {
        bail!("Supply exactly one --args or --args-file");
    }
    let interactive = args.contains_key("--interactive");
    if interactive {
        ensure!(
            !["--args", "--args-file", "--tool", "--session"]
                .iter()
                .any(|option| args.contains_key(*option)),
            "Interactive calls specify their tool and arguments on stdin; do not attach implicitly"
        );
    }
    let mut client = Client::start_with_arguments(
        required(args, "--server")?,
        &options.server_arguments,
        initialization_timeout,
    )?;
    if interactive {
        return interactive_calls(client, args.get("--out").map(Path::new));
    }
    if let Some(session) = args.get("--session") {
        client.call("cad_attach", json!({"session_id":session}))?;
    }
    let arguments = if let Some(path) = args.get("--args-file") {
        fs::read_to_string(path)?
    } else {
        required(args, "--args")?.to_owned()
    };
    let result = client.call(
        args.get("--tool")
            .map(String::as_str)
            .unwrap_or("cad_interface"),
        serde_json::from_str(&arguments)?,
    )?;
    if let Some(path) = args.get("--out") {
        fs::write(path, serde_json::to_string_pretty(&result)?)?;
    } else {
        println!("{}", serde_json::to_string_pretty(&result)?);
    }
    Ok(())
}

/// Keep observations in one MCP connection while the operator chooses each next call.
fn interactive_calls(mut client: Client, output: Option<&Path>) -> Result<()> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Call {
        tool: String,
        arguments: Value,
    }

    if let Some(output) = output {
        fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(output)
            .with_context(|| format!("Prepare interactive MCP output {}", output.display()))?;
    }
    let mut stdout = std::io::stdout().lock();
    writeln!(
        stdout,
        "{}",
        json!({"status":"ready","server":client.initialization(),"pid":client.process_id()})
    )?;
    stdout.flush()?;
    for line in std::io::stdin().lock().lines() {
        let line = line.context("Read interactive MCP request")?;
        if line.trim().is_empty() {
            continue;
        }
        let result = match serde_json::from_str::<Call>(&line) {
            Ok(call) if !call.tool.trim().is_empty() && call.arguments.is_object() => {
                let response = client.rpc_response(
                    "tools/call",
                    json!({"name":call.tool,"arguments":call.arguments}),
                )?;
                if response.get("error").is_some() {
                    json!({"status":"tool_error","error":response["error"],"response":response})
                } else {
                    let result = &response["result"];
                    json!({
                        "status":if result["isError"] == true {"tool_error"} else {"received"},
                        "result":result
                    })
                }
            }
            Ok(_) => {
                json!({"status":"request_error","error":"tool must be nonempty and arguments must be an object"})
            }
            Err(error) => json!({"status":"request_error","error":error.to_string()}),
        };
        if let Some(output) = output {
            if let Err(error) = fs::write(output, serde_json::to_vec_pretty(&result)?) {
                writeln!(
                    stdout,
                    "{}",
                    json!({"status":"output_error","error":error.to_string(),"response":result})
                )?;
                stdout.flush()?;
                return Err(error)
                    .context("MCP response was printed to stdout; the call was not retried");
            }
            writeln!(
                stdout,
                "{}",
                json!({"status":result["status"],"output":output})
            )?;
        } else {
            writeln!(stdout, "{result}")?;
        }
        stdout.flush()?;
    }
    client.finish(Duration::from_secs(10))
}
fn launched_session(launch: &Value) -> Result<String> {
    if launch["status"] != "ready" {
        bail!("CAD launch is not ready: {launch}. Inspect that process before retrying; no script has run and no duplicate window was launched.");
    }
    launch["session_id"]
        .as_str()
        .filter(|id| !id.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("Ready CAD launch did not identify a document; no script has run"))
}

#[derive(Debug)]
struct ReplayOutputs {
    directory: Option<PathBuf>,
    save: Option<PathBuf>,
}
impl ReplayOutputs {
    fn prepare(out: Option<&str>, save: Option<&str>, repeat: usize) -> Result<Self> {
        let directory = out
            .map(|path| prepare_directory(Path::new(path), "replay output"))
            .transpose()?;
        let save = save
            .map(|path| -> Result<PathBuf> {
                let path = Path::new(path);
                let filename = path.file_name().ok_or_else(|| {
                    anyhow!("Save destination must name a file: {}", path.display())
                })?;
                let parent = path
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(Path::new("."));
                let path = prepare_directory(parent, "CAD save")?.join(filename);
                validate_file_destination(&path)?;
                Ok(path)
            })
            .transpose()?;
        if let Some(out) = &directory {
            for iteration in 1..=repeat {
                for name in [
                    format!("run-{iteration}.json"),
                    format!("model-{iteration}.json"),
                ] {
                    let path = out.join(name);
                    validate_file_destination(&path)?;
                    if save
                        .as_ref()
                        .is_some_and(|save| same_destination(save, &path))
                    {
                        bail!(
                            "CAD save destination conflicts with replay output: {}",
                            path.display()
                        );
                    }
                }
            }
        }
        Ok(Self { directory, save })
    }

    fn complete(
        &self,
        iteration: usize,
        report: &Value,
        save: impl FnOnce(&Path) -> Result<Value>,
    ) -> Result<()> {
        let report_path = if let Some(out) = &self.directory {
            let path = out.join(format!("run-{iteration}.json"));
            fs::write(&path, serde_json::to_vec_pretty(report)?)
                .with_context(|| format!("Write completed replay report {}", path.display()))?;
            if let Some(model) = report.pointer("/exports/final_model") {
                let model_path = out.join(format!("model-{iteration}.json"));
                fs::write(&model_path, serde_json::to_vec_pretty(model)?).with_context(|| {
                    format!(
                        "Write model snapshot {}; replay report retained at {}",
                        model_path.display(),
                        path.display()
                    )
                })?;
            }
            Some(path)
        } else {
            None
        };
        if let Some(path) = &self.save {
            save(path).with_context(|| {
                let retained = report_path
                    .as_ref()
                    .map(|report| format!("; replay report retained at {}", report.display()))
                    .unwrap_or_default();
                format!(
                    "Replay completed, but saving CAD file {} failed{retained}",
                    path.display()
                )
            })?;
        }
        Ok(())
    }
}

fn prepare_directory(path: &Path, purpose: &str) -> Result<PathBuf> {
    if path.as_os_str().is_empty() {
        bail!("The {purpose} directory must not be empty");
    }
    fs::create_dir_all(path)
        .with_context(|| format!("Prepare {purpose} directory {}", path.display()))?;
    let path = fs::canonicalize(path)
        .with_context(|| format!("Resolve {purpose} directory {}", path.display()))?;

    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    for _ in 0..100 {
        let probe = path.join(format!(
            ".limo-replay-write-check-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&probe)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("Verify writable {purpose} directory {}", path.display())
                })
            }
        };
        let written = file
            .write_all(b"Limo CAD replay output preflight\n")
            .and_then(|_| file.sync_all());
        drop(file);
        let removed = fs::remove_file(&probe);
        written
            .with_context(|| format!("Verify writable {purpose} directory {}", path.display()))?;
        removed.with_context(|| format!("Remove output preflight file {}", probe.display()))?;
        return Ok(path);
    }
    bail!(
        "Could not allocate a write check in {purpose} directory {}",
        path.display()
    )
}

fn validate_file_destination(path: &Path) -> Result<()> {
    match fs::metadata(path) {
        Ok(metadata) => {
            if !metadata.is_file() || metadata.permissions().readonly() {
                bail!(
                    "Output destination must be a writable regular file: {}",
                    path.display()
                );
            }

            fs::OpenOptions::new()
                .write(true)
                .open(path)
                .with_context(|| format!("Verify writable output file {}", path.display()))?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error)
                .with_context(|| format!("Inspect output destination {}", path.display()))
        }
    }
    Ok(())
}

fn same_destination(a: &Path, b: &Path) -> bool {
    let a = fs::canonicalize(a).unwrap_or_else(|_| a.to_owned());
    let b = fs::canonicalize(b).unwrap_or_else(|_| b.to_owned());
    if cfg!(windows) {
        a.as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case(&b.as_os_str().to_string_lossy())
    } else {
        a == b
    }
}

pub fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let options = options(args)?;
    if options.values.contains_key("--help") {
        print_usage(true);
        return Ok(());
    }
    let initialization_timeout = options.initialization_timeout()?;
    let args = &options.values;
    known_options(
        args,
        &[
            "--server",
            "--init-timeout-seconds",
            "--session",
            "--desktop",
            "--new",
            "--present",
            "--speed",
            "--repeat",
            "--save",
            "--out",
            "--compare",
            "--recipe",
        ],
    )?;
    let live = args.contains_key("--session") || args.contains_key("--desktop");
    if !live && (args.contains_key("--new") || args.contains_key("--present")) {
        bail!("New-tab and presentation options require a desktop session");
    }
    if args.contains_key("--session") && args.contains_key("--desktop") {
        bail!("Choose existing session or desktop launch");
    }
    let speed = args
        .get("--speed")
        .map(|s| s.parse::<f64>())
        .transpose()?
        .unwrap_or(1.0);
    if !speed.is_finite() || !(0.1..=16.0).contains(&speed) {
        bail!("Speed must be from 0.1 to 16");
    }
    if options.file.is_some() && args.contains_key("--recipe") {
        bail!("Choose a script path or --recipe ID");
    }
    let path = if args.contains_key("--recipe") {
        None
    } else {
        Some(fs::canonicalize(options.file.as_ref().ok_or_else(
            || anyhow!("Supply a script path or --recipe ID"),
        )?)?)
    };
    let server = required(args, "--server")?;
    let repeat = args
        .get("--repeat")
        .map(|v| v.parse::<usize>())
        .transpose()?
        .unwrap_or(1);
    if repeat == 0 {
        bail!("repeat must be positive");
    }
    if repeat > 1 && (args.contains_key("--session") || args.contains_key("--desktop")) {
        bail!("Determinism repeats use independent headless processes; run the visible demonstration separately");
    }
    let mut baseline = args
        .get("--compare")
        .map(|p| -> Result<Value> {
            semantic_result(&serde_json::from_str(&fs::read_to_string(p)?)?)
        })
        .transpose()?;
    let outputs = ReplayOutputs::prepare(
        args.get("--out").map(String::as_str),
        args.get("--save").map(String::as_str),
        repeat,
    )?;
    for iteration in 1..=repeat {
        let mut client = Client::start_with_arguments(
            server,
            &options.server_arguments,
            initialization_timeout,
        )?;

        if let Some(recipe) = args.get("--recipe") {
            let catalog = client.call("cad_interface", json!({"action":"recipes"}))?;
            if !catalog
                .as_array()
                .into_iter()
                .flatten()
                .any(|entry| entry["id"] == *recipe)
            {
                bail!("Unknown recipe '{recipe}'; inspect the server's recipe catalog");
            }
        }
        let mut session = args.get("--session").cloned();
        if let Some(desktop) = args.get("--desktop") {
            if session.is_some() {
                bail!("Choose existing session or desktop launch");
            }
            let launch = client.call(
                "cad_interface",
                json!({"action":"launch","executable":desktop}),
            )?;
            session = Some(launched_session(&launch)?);
        }
        if let Some(id) = &session {
            client.call("cad_attach", json!({"session_id":id}))?;
        }
        if args.contains_key("--new") {
            if session.is_none() {
                bail!("--new requires an existing desktop session");
            }
            let inspect = client.call("cad_interface", json!({"action":"inspect"}))?;
            let target = inspect["ui"]["surfaces"]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|v| v["controls"].as_array().into_iter().flatten())
                .find(|v| v["label"] == "New design" && v["disabled"] == false)
                .and_then(|v| v["id"].as_str())
                .ok_or_else(|| anyhow!("New design control unavailable"))?;
            let reply = client.call("cad_interface", json!({"action":"click","target":target}))?;
            session = reply["active_session_id"].as_str().map(str::to_owned);
            client.call("cad_attach", json!({"session_id":session}))?;
        }
        let label = args
            .get("--recipe")
            .cloned()
            .unwrap_or_else(|| path.as_ref().unwrap().display().to_string());
        eprintln!("Running {label} ({iteration}/{repeat})");
        let mut request = json!({"action":"script","mode":if args.contains_key("--present"){"present"}else{"fast"},"speed":speed,"validate":true});
        if let Some(recipe) = args.get("--recipe") {
            request["recipe"] = json!(recipe);
        } else {
            request["path"] = json!(path);
        }
        let mut report = client.call("cad_interface", request)?;
        if let Some(session) = session {
            report["session_id"] = json!(session);
        }
        outputs.complete(iteration, &report, |save| {
            if live {
                client.call(
                    "cad_interface",
                    json!({"action":"file","command":"save","path":save}),
                )
            } else {
                save_headless(
                    &mut client,
                    save,
                    server,
                    &options.server_arguments,
                    initialization_timeout,
                )?;
                Ok(json!({"saved":true}))
            }
        })?;
        if repeat > 1 || args.contains_key("--compare") {
            let semantic = semantic_result(&report)?;
            if let Some(previous) = &baseline {
                if previous != &semantic {
                    let difference = first_difference(previous, &semantic, "").unwrap_or_default();
                    bail!("Replay {iteration} differs from the comparison model at {difference}");
                }
            } else {
                baseline = Some(semantic);
            }
        }
        eprintln!(
            "Completed {} steps and {} end checks in {} ms",
            report["steps_completed"], report["checks_completed"], report["elapsed_ms"]
        );
    }
    if repeat > 1 {
        println!("PASS: {repeat} independent runs produced identical model, sketches, assembly solution and geometry.");
    }
    if args.contains_key("--compare") {
        println!("PASS: replay matches the comparison model, sketches, assembly and geometry.");
    }
    Ok(())
}

fn print_usage(script: bool) {
    if script {
        println!(
            "Usage: cargo xtask run-script [FILE.limo.jsonc | --recipe ID] --server PATH [OPTIONS]\n\
  --session UUID --new --present  Replay visibly in a new design of an existing window.\n\
  --desktop PATH                 Launch a desktop instead of attaching to --session.\n\
  --speed N                      Presentation speed, 0.1–16 (default: 1).\n\
  --repeat N                     Compare independent headless runs (default: 1).\n\
  --compare REPORT.json          Compare the final model with a previous replay.\n\
  --out DIRECTORY                Retain replay reports and model snapshots.\n\
  --save FILE.limo               Save after replay (headless or live).\n\
                                 Headless exports are reopened in a fresh native engine before writing."
        );
    } else {
        println!(
            "Usage: cargo xtask cad-call [--server PATH | --installed] [--tool NAME] [--args JSON | --args-file FILE] [OPTIONS]\n\
  --tool NAME                    MCP tool name (default: cad_interface).\n\
  --session UUID                 Attach to an explicitly selected live design.\n\
  --out FILE.json                Write the tool result instead of stdout.\n\
  --interactive                  Keep one MCP connection; accept one {{\"tool\":NAME,\"arguments\":OBJECT}} per stdin line.\n\
                                 With --out, replace that file with each response and print its receipt.\n\
                                 Transport failure stops the connection; calls are never retried.\n\
  --installed                    Verify and reconnect to the clean canonical native-control runtime.\n\
                                 Never build, deploy or stop CAD; this does not qualify current source."
        );
    }
    println!(
        "\nServer options:\n\
  --server-arg ARG                Pass one literal argument to the server; repeat to preserve order.\n\
  --init-timeout-seconds N        Bound MCP initialization only (1–600, default: 30).\n\
                                 Modeling and presentation waits remain unbounded.\n\
\nPackaged CAD worker (no extra window): --server PATH --server-arg --headless\n\
AppImage without FUSE: --server PATH --server-arg --appimage-extract-and-run --server-arg --headless\n\
Standalone limo-cad-mcp: --server PATH (no server argument required)"
    );
}

fn save_headless(
    client: &mut Client,
    path: &Path,
    server: &str,
    server_arguments: &[String],
    initialization_timeout: Duration,
) -> Result<()> {
    let exported = client.call("cad_project_model", json!({}))?;
    let model_json = exported
        .as_str()
        .context("Project export is not JSON text")?;
    let expected: Value = serde_json::from_str(model_json)?;
    let version = client.initialization()["serverInfo"]["_meta"]["limo-cad/build"]["version"]
        .as_str()
        .or_else(|| client.initialization()["serverInfo"]["version"].as_str())
        .context("MCP server did not identify its application version")?;
    let bytes = crate::project_archive::encode(model_json, version)?;
    let archived_model = crate::project_archive::model(&bytes)?;
    let body_ids = |scene: &Value| -> Result<Vec<Value>> {
        ensure!(
            scene["errors"] == json!([]),
            "Project has geometry errors: {}",
            scene["errors"]
        );
        scene["bodies"]
            .as_array()
            .context("Project has no body array")?
            .iter()
            .map(|body| body.get("id").cloned().context("Project body has no ID"))
            .collect()
    };
    let expected_bodies = body_ids(&client.call("solid_scene", json!({}))?)?;

    let mut restored =
        Client::start_with_arguments(server, server_arguments, initialization_timeout)?;
    restored
        .call(
            "cad_load_project_model",
            json!({"model_json":archived_model}),
        )
        .context("Reopen generated project in a fresh native engine")?;
    let resaved = restored.call("cad_project_model", json!({}))?;
    let actual: Value = serde_json::from_str(
        resaved
            .as_str()
            .context("Reopened project is not JSON text")?,
    )?;
    ensure!(
        actual == expected,
        "Reopening the generated project changed its model at {}",
        first_difference(&expected, &actual, "").unwrap_or_default()
    );
    ensure!(
        body_ids(&restored.call("solid_scene", json!({}))?)? == expected_bodies,
        "Reopening the generated project changed its solid bodies"
    );
    restored.finish(Duration::from_secs(10))?;

    fs::write(path, bytes).with_context(|| format!("Write headless project {}", path.display()))?;
    eprintln!("Saved and reopened headless project {}", path.display());
    Ok(())
}

fn semantic_result(report: &Value) -> Result<Value> {
    let exports = report
        .get("exports")
        .ok_or_else(|| anyhow!("Script did not return exports"))?;
    let mut selected = serde_json::Map::new();
    for key in [
        "final_model",
        "final_scene",
        "final_solution",
        "final_sketches",
    ] {
        if let Some(value) = exports.get(key) {
            let mut value = value.clone();

            if let Some(object) = value.as_object_mut() {
                object.remove("_disclosure");
            }
            if key == "final_model" {
                normalize_saved_layout_ids(&mut value);
            }
            if key == "final_sketches" {
                if let Some(sketches) = value.as_array_mut() {
                    for sketch in sketches {
                        if let Some(sketch) = sketch.as_object_mut() {
                            for transient in ["can_undo", "can_redo", "reference_midpoints"] {
                                sketch.remove(transient);
                            }
                        }
                    }
                }
            }
            selected.insert(key.into(), value);
        }
    }
    if selected.is_empty() {
        bail!("Script must export a final model or scene for determinism comparison");
    }
    Ok(Value::Object(selected))
}

/// Independent documents mint different layout UUIDs. Compare saved layouts and
/// their print-height ownership links by order; keep every configuration field.
fn normalize_saved_layout_ids(model: &mut Value) {
    let ids: HashMap<_, _> = model
        .get("views")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(index, view)| {
            view.get("id")
                .and_then(Value::as_str)
                .map(|id| (id.to_owned(), format!("saved-layout-{index}")))
        })
        .collect();
    if ids.is_empty() {
        return;
    }
    fn remap(value: &mut Value, ids: &HashMap<String, String>) {
        match value {
            Value::Object(object) => {
                if let Some(replacement) = object
                    .get("id")
                    .and_then(Value::as_str)
                    .and_then(|id| ids.get(id))
                {
                    object.insert("id".into(), Value::String(replacement.clone()));
                }
                for value in object.values_mut() {
                    remap(value, ids);
                }
            }
            Value::Array(array) => {
                for value in array {
                    remap(value, ids);
                }
            }
            _ => {}
        }
    }
    remap(model, &ids);
}

fn first_difference(a: &Value, b: &Value, path: &str) -> Option<String> {
    if a == b {
        return None;
    }
    match (a, b) {
        (Value::Object(a), Value::Object(b)) => {
            for (key, value) in a {
                let next = format!("{path}/{key}");
                if let Some(other) = b.get(key) {
                    if let Some(found) = first_difference(value, other, &next) {
                        return Some(found);
                    }
                } else {
                    return Some(next);
                }
            }
            b.keys()
                .find(|key| !a.contains_key(*key))
                .map(|key| format!("{path}/{key}"))
        }
        (Value::Array(a), Value::Array(b)) => {
            if a.len() != b.len() {
                return Some(format!("{path}/length"));
            }
            a.iter()
                .zip(b)
                .enumerate()
                .find_map(|(i, (a, b))| first_difference(a, b, &format!("{path}/{i}")))
        }
        _ => Some(path.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "child-process fixture invoked only by transport tests"]
    fn transport_child_waits_for_eof() {
        if std::env::var_os("LIMO_CAD_TRANSPORT_TEST_CHILD").is_some() {
            println!("LIMO_CAD_TRANSPORT_READY");
            std::io::stdout().flush().unwrap();
            for line in std::io::stdin().lock().lines() {
                if line.is_err() {
                    break;
                }
            }
        }
    }

    fn transport_client(
        timeout: Option<Duration>,
    ) -> (Client, mpsc::Sender<Result<Value, String>>) {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "replay::tests::transport_child_waits_for_eof",
                "--ignored",
                "--nocapture",
            ])
            .env("LIMO_CAD_TRANSPORT_TEST_CHILD", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn().unwrap();
        let output = child.stdout.take().unwrap();
        let (ready_tx, ready_rx) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                match line {
                    Ok(line) if line == "LIMO_CAD_TRANSPORT_READY" => {
                        let _ = ready_tx.send(());
                    }
                    Ok(_) => {}
                    Err(_) => return,
                }
            }
        });
        let ready = ready_rx.recv_timeout(Duration::from_secs(30));
        if ready.is_err() {
            let _ = child.kill();
            let _ = child.wait();
            reader.join().unwrap();
        }
        assert!(
            ready.is_ok(),
            "Transport fixture did not become ready: {ready:?}"
        );
        let input = child.stdin.take();
        let (sender, replies) = mpsc::channel();
        (
            Client {
                child,
                input,
                replies,
                id: 0,
                request_timeout: timeout,
                initialization: Value::Null,
            },
            sender,
        )
    }

    #[test]
    fn deadline_ignores_notification_traffic_and_reaps_owned_process() {
        let (mut client, sender) = transport_client(Some(Duration::from_millis(50)));
        let traffic = std::thread::spawn(move || {
            while sender
                .send(Ok(
                    json!({"jsonrpc":"2.0","method":"notifications/tools/list_changed"}),
                ))
                .is_ok()
            {
                std::thread::sleep(Duration::from_millis(5));
            }
        });
        let error = client.rpc("ping", json!({})).unwrap_err();
        assert!(error.to_string().contains("deadline"), "{error:#}");
        assert!(
            client.child.try_wait().unwrap().is_some(),
            "Timed-out child was not reaped"
        );
        drop(client);
        traffic.join().unwrap();
    }

    #[test]
    fn ordinary_replay_keeps_waiting_and_eof_exits_cleanly() {
        let (mut client, sender) = transport_client(None);
        let reply = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            sender
                .send(Ok(json!({"jsonrpc":"2.0","id":1,"result":{"pong":true}})))
                .unwrap();
        });
        assert_eq!(client.rpc("ping", json!({})).unwrap(), json!({"pong":true}));
        reply.join().unwrap();
        client.finish(Duration::from_secs(5)).unwrap();
    }

    #[test]
    fn eof_rejects_trailing_non_protocol_stdout() {
        let (client, sender) = transport_client(None);
        sender
            .send(parse_reply_line("{\"log\":\"unexpected shutdown log\"}"))
            .unwrap();
        drop(sender);
        let error = client.finish(Duration::from_secs(5)).unwrap_err();
        assert!(
            error.to_string().contains("Invalid MCP output before EOF"),
            "{error:#}"
        );
    }

    #[test]
    fn stdout_eof_is_observed_without_requiring_process_exit() {
        let (mut client, sender) = transport_client(None);
        sender
            .send(Ok(
                json!({"jsonrpc":"2.0","method":"notifications/tools/list_changed"}),
            ))
            .unwrap();
        drop(sender);
        client.require_stdout_eof(Duration::from_secs(1)).unwrap();
        assert!(
            client.is_running().unwrap(),
            "Observing transport EOF must not terminate the GUI owner"
        );
        client.finish(Duration::from_secs(5)).unwrap();
    }

    #[test]
    fn stdout_eof_rejects_a_pipe_retained_by_a_live_process() {
        let (mut client, sender) = transport_client(None);
        let error = client
            .require_stdout_eof(Duration::from_millis(25))
            .unwrap_err();
        assert!(error.to_string().contains("stdout did not close"));
        assert!(client.is_running().unwrap());
        drop(sender);
        client.finish(Duration::from_secs(5)).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn owned_fixture_logs_preserve_protocol_and_stderr_without_overwriting_evidence() {
        let root = TestDirectory::new();
        let mut command = Command::new("sh");
        command.args(["-c", r#"read request
printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-06-18","capabilities":{},"serverInfo":{"name":"owned-fixture","version":"1"}}}'
printf '%s\n' 'owned renderer diagnostic' >&2
cat >/dev/null"#]);
        let client =
            Client::start_command_logged(command, Some(Duration::from_secs(5)), &root.0).unwrap();
        assert_eq!(
            client.initialization()["serverInfo"]["name"],
            "owned-fixture"
        );
        client.finish(Duration::from_secs(5)).unwrap();
        let stdout = fs::read(root.0.join("host-stdout.jsonl")).unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&stdout).unwrap()["id"], 1);
        assert!(fs::read_to_string(root.0.join("host-stderr.log"))
            .unwrap()
            .contains("owned renderer diagnostic"));
        assert!(Client::start_command_logged(Command::new("sh"), None, &root.0).is_err());
        assert_eq!(fs::read(root.0.join("host-stdout.jsonl")).unwrap(), stdout);
    }

    struct TestDirectory(PathBuf);
    impl TestDirectory {
        fn new() -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static SEQUENCE: AtomicU64 = AtomicU64::new(0);
            for _ in 0..100 {
                let path = std::env::temp_dir().join(format!(
                    "limo-cad-replay-test-{}-{}",
                    std::process::id(),
                    SEQUENCE.fetch_add(1, Ordering::Relaxed)
                ));
                match fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("Create owned test directory: {error}"),
                }
            }
            panic!("Could not allocate an owned test directory")
        }
    }
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn creates_shared_output_and_save_parent_without_creating_the_save_file() {
        let temp = TestDirectory::new();
        let out = temp.0.join("new/nested/output");
        let save = out.join("design.limo");
        let prepared = ReplayOutputs::prepare(out.to_str(), save.to_str(), 2).unwrap();
        let absolute = fs::canonicalize(&out).unwrap();
        assert_eq!(prepared.directory, Some(absolute.clone()));
        assert_eq!(prepared.save, Some(absolute.join("design.limo")));
        assert_eq!(
            fs::read_dir(&out).unwrap().count(),
            0,
            "write probes must be removed and real outputs must not be created early"
        );
    }

    #[test]
    fn output_preflight_runs_before_starting_the_server_or_desktop() {
        let temp = TestDirectory::new();
        let out = temp.0.join("reports");
        let save = temp.0.join("separate/save/design.limo");
        let args = || {
            vec![
                "--server".into(),
                temp.0
                    .join("missing-mcp-executable")
                    .to_string_lossy()
                    .into_owned(),
                "--desktop".into(),
                "must-not-launch".into(),
                "--recipe".into(),
                "unused".into(),
                "--out".into(),
                out.to_string_lossy().into_owned(),
                "--save".into(),
                save.to_string_lossy().into_owned(),
            ]
        };
        let error = run(args().into_iter()).unwrap_err();
        assert!(error.to_string().contains("Start MCP server"), "{error:#}");
        assert!(out.is_dir());
        assert!(save.parent().unwrap().is_dir());
        assert!(!save.exists());

        fs::remove_dir(&out).unwrap();
        fs::write(&out, b"existing user file").unwrap();
        let error = run(args().into_iter()).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Prepare replay output directory"),
            "{error:#}"
        );
        assert_eq!(fs::read(&out).unwrap(), b"existing user file");
    }

    #[test]
    fn preflight_preserves_existing_files_and_rejects_bad_destinations() {
        let temp = TestDirectory::new();
        let report = temp.0.join("run-1.json");
        let save = temp.0.join("design.limo");
        fs::write(&report, b"previous report").unwrap();
        fs::write(&save, b"existing CAD work").unwrap();
        ReplayOutputs::prepare(temp.0.to_str(), save.to_str(), 1).unwrap();
        assert_eq!(fs::read(&report).unwrap(), b"previous report");
        assert_eq!(fs::read(&save).unwrap(), b"existing CAD work");
        assert_eq!(fs::read_dir(&temp.0).unwrap().count(), 2);
        assert!(ReplayOutputs::prepare(temp.0.to_str(), report.to_str(), 1)
            .unwrap_err()
            .to_string()
            .contains("conflicts with replay output"));

        let original = fs::metadata(&save).unwrap().permissions();
        let mut readonly = original.clone();
        readonly.set_readonly(true);
        fs::set_permissions(&save, readonly).unwrap();
        let result = ReplayOutputs::prepare(None, save.to_str(), 1);
        fs::set_permissions(&save, original).unwrap();
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("writable regular file"));
        assert_eq!(fs::read(&save).unwrap(), b"existing CAD work");

        fs::create_dir(temp.0.join("model-1.json")).unwrap();
        assert!(ReplayOutputs::prepare(temp.0.to_str(), None, 1).is_err());
    }

    #[test]
    fn relative_save_filename_is_resolved_for_the_desktop_process() {
        let temp = TestDirectory::new();
        let filename = format!("{}.limo", temp.0.file_name().unwrap().to_string_lossy());
        let outputs = ReplayOutputs::prepare(None, Some(&filename), 1).unwrap();
        assert_eq!(
            outputs.save,
            Some(fs::canonicalize(".").unwrap().join(filename))
        );
    }

    #[test]
    fn completed_report_and_model_survive_a_later_save_failure() {
        let temp = TestDirectory::new();
        let save = temp.0.join("saved/design.limo");
        let outputs = ReplayOutputs::prepare(temp.0.to_str(), save.to_str(), 1).unwrap();
        let model = json!({"schema_version":6,"name":"completed work"});
        let report = json!({"session_id":"live-document","steps_completed":3,"checks_completed":2,"exports":{"final_model":model}});
        let error = outputs
            .complete(1, &report, |destination| {
                assert_eq!(destination, outputs.save.as_ref().unwrap());
                let retained: Value =
                    serde_json::from_slice(&fs::read(temp.0.join("run-1.json"))?)?;
                assert_eq!(retained, report, "report must exist before Save is called");
                let retained_model: Value =
                    serde_json::from_slice(&fs::read(temp.0.join("model-1.json"))?)?;
                assert_eq!(retained_model, model);
                bail!("simulated late save failure")
            })
            .unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("Replay completed"));
        assert!(message.contains("replay report retained at"));
        assert!(message.contains("simulated late save failure"));
        assert_eq!(
            serde_json::from_slice::<Value>(&fs::read(temp.0.join("run-1.json")).unwrap()).unwrap(),
            report
        );
    }

    #[test]
    fn failure_to_retain_the_report_stops_before_save() {
        let temp = TestDirectory::new();
        let save = temp.0.join("design.limo");
        let outputs = ReplayOutputs::prepare(temp.0.to_str(), save.to_str(), 1).unwrap();

        fs::create_dir(temp.0.join("run-1.json")).unwrap();
        let called = std::cell::Cell::new(false);
        let error = outputs
            .complete(1, &json!({"steps_completed":3}), |_| {
                called.set(true);
                Ok(json!({}))
            })
            .unwrap_err();
        assert!(!called.get());
        assert!(error.to_string().contains("Write completed replay report"));
    }

    #[test]
    fn live_launch_cannot_fall_back_to_headless_replay() {
        assert_eq!(
            launched_session(&json!({"status":"ready","session_id":"document"})).unwrap(),
            "document"
        );
        for reply in [
            json!({"status":"starting","pid":123}),
            json!({"status":"starting","session_id":"document"}),
            json!({"status":"failed","session_id":"document"}),
            json!({"status":"ready"}),
            json!({"status":"ready","session_id":" "}),
        ] {
            assert!(launched_session(&reply).is_err(), "{reply}");
        }
    }

    #[test]
    fn reports_model_difference_without_dropping_geometry() {
        let a =
            json!({"exports":{"final_scene":{"_disclosure":{"focus":"a"},"bodies":[{"id":1}]}}});
        let b =
            json!({"exports":{"final_scene":{"_disclosure":{"focus":"b"},"bodies":[{"id":1}]}}});
        assert_eq!(semantic_result(&a).unwrap(), semantic_result(&b).unwrap());
        let b = json!({"exports":{"final_scene":{"bodies":[{"id":2}]}}});
        assert_eq!(
            first_difference(
                &semantic_result(&a).unwrap(),
                &semantic_result(&b).unwrap(),
                ""
            ),
            Some("/final_scene/bodies/0/id".into())
        );
    }
    #[test]
    fn rejects_ignored_and_duplicate_options() {
        let args = HashMap::from([("--speed".to_owned(), "2".to_owned())]);
        assert!(known_options(&args, &["--server"]).is_err());
        assert!(options(
            ["--server", "a", "--server", "b"]
                .into_iter()
                .map(str::to_owned)
        )
        .is_err());
    }
    #[test]
    fn server_arguments_are_literal_ordered_and_repeatable() {
        let options = options(
            [
                "--server",
                "CAD folder/Limo-CAD.exe",
                "--server-arg",
                "--headless",
                "--server-arg",
                "--appimage-extract-and-run",
                "--server-arg",
                "a b",
                "--server-arg",
                "",
                "--recipe",
                "fillet-basics",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap();
        assert_eq!(
            options.server_arguments,
            ["--headless", "--appimage-extract-and-run", "a b", ""]
        );
        assert_eq!(options.values["--recipe"], "fillet-basics");
        assert_eq!(
            options.initialization_timeout().unwrap(),
            Duration::from_secs(30)
        );
        assert!(options.file.is_none());
        assert!(super::options(["--server-arg".into()].into_iter()).is_err());
    }

    #[test]
    fn initialization_timeout_rejects_ambiguous_or_unbounded_values() {
        for value in ["0", "601", "-1", "1.5", "NaN"] {
            let options = options(
                ["--init-timeout-seconds", value]
                    .into_iter()
                    .map(str::to_owned),
            )
            .unwrap();
            assert!(options.initialization_timeout().is_err(), "{value}");
        }
        let options = options(
            ["--init-timeout-seconds", "600"]
                .into_iter()
                .map(str::to_owned),
        )
        .unwrap();
        assert_eq!(
            options.initialization_timeout().unwrap(),
            Duration::from_secs(600)
        );
        assert!(super::options(
            ["--init-timeout-seconds", "1", "--init-timeout-seconds", "2"]
                .into_iter()
                .map(str::to_owned)
        )
        .is_err());
    }
    #[test]
    fn comparison_preserves_small_geometry_normals_across_json_roundtrips() {
        let value = json!({"normal":-1.1728120758078999e-17_f64});
        let restored: Value =
            serde_json::from_str(&serde_json::to_string_pretty(&value).unwrap()).unwrap();
        assert_eq!(value, restored);
    }
    #[test]
    fn comparison_keeps_persisted_sketch_references_but_ignores_editing_cache() {
        let constraint =
            json!({"type":"reference_midpoint","edge_id":42,"position":{"x":2.0,"y":3.0}});
        let a = json!({"exports":{"final_sketches":[{"can_undo":true,"reference_midpoints":[42],"constraints":[constraint]}]}});
        let b = json!({"exports":{"final_sketches":[{"can_undo":false,"reference_midpoints":[],"constraints":[constraint]}]}});
        assert_eq!(semantic_result(&a).unwrap(), semantic_result(&b).unwrap());
        let mut changed = b;
        changed["exports"]["final_sketches"][0]["constraints"][0]["position"]["x"] = json!(2.1);
        assert_ne!(
            semantic_result(&a).unwrap(),
            semantic_result(&changed).unwrap()
        );
    }

    #[test]
    fn comparison_retains_layout_configuration_and_binding_ownership_across_documents() {
        let a = json!({"exports":{"final_model":{
            "views":[
                {"id":"ca2faff1-eded-48eb-b102-1ce13fe9b0bb","name":"Assembly","camera":{"position":[200,-200,200]},"visible_body_ids":[1]},
                {"id":"6a9dfb7c-d618-494d-bf89-e7094d6b2632","name":"Print","print_layout":true,"occurrence_offsets":[{"occurrence_id":2,"translation":[0,180,0]}]}
            ],
            "print_intent":{"height_ranges":[{"binding":{"layout":{"kind":"named_layout","id":"6a9dfb7c-d618-494d-bf89-e7094d6b2632"}}}]},
            "document":{"history":{"features":[{"id":1,"name":"Named stock"}]}}
        }}});
        let mut b = a.clone();
        b["exports"]["final_model"]["views"][0]["id"] =
            json!("13067e89-15c2-4fd8-819b-e20394ed6899");
        b["exports"]["final_model"]["views"][1]["id"] =
            json!("2e53b282-7656-425e-baa3-2c230126311d");
        b["exports"]["final_model"]["print_intent"]["height_ranges"][0]["binding"]["layout"]
            ["id"] = b["exports"]["final_model"]["views"][1]["id"].clone();
        assert_eq!(semantic_result(&a).unwrap(), semantic_result(&b).unwrap());
        for (pointer, changed) in [
            ("/views/0/name", json!("Changed assembly")),
            ("/views/0/camera/position/0", json!(201)),
            ("/views/0/visible_body_ids/0", json!(2)),
            ("/views/1/occurrence_offsets/0/translation/1", json!(181)),
            ("/document/history/features/0/id", json!(2)),
            (
                "/print_intent/height_ranges/0/binding/layout/id",
                b["exports"]["final_model"]["views"][0]["id"].clone(),
            ),
        ] {
            let mut different = b.clone();
            *different["exports"]["final_model"]
                .pointer_mut(pointer)
                .unwrap() = changed;
            assert_ne!(
                semantic_result(&a).unwrap(),
                semantic_result(&different).unwrap(),
                "The comparison hid {pointer}"
            );
        }
        assert_ne!(a, b, "Persisted identities must stay distinct");
    }
}
