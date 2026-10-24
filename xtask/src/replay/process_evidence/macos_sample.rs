//! Best-effort, bounded sampling before the fixture owner's existing cleanup.
//! Sample failures are evidence, never a reason to skip killing the GUI child.
use serde_json::{json, Value};
use std::{
    io::{self, Read},
    process::{Child, Command, ExitStatus},
    time::Duration,
};

const MAX_OUTPUT_BYTES: usize = 256 * 1024;
const MAX_REPORT_BYTES: usize = 4096;
const MAX_WARNING_BYTES: usize = 768;
const POLL: Duration = Duration::from_millis(10);
const RUN_LIMIT: Duration = Duration::from_secs(5);
const REAP_LIMIT: Duration = Duration::from_secs(1);

fn sample_command(pid: u32) -> Command {
    let mut command = Command::new("/usr/bin/sample");
    // PID only: no name lookup, -wait, sudo, environment, heap or model dump.
    // -file avoids sample's otherwise implicit unbounded /tmp report.
    command.args([
        &pid.to_string(),
        "1",
        "10",
        "-mayDie",
        "-file",
        "/dev/stdout",
    ]);
    command
}

#[cfg(target_os = "macos")]
pub(super) fn capture(directory: &std::path::Path, target: &mut Child) {
    use std::{fs, os::fd::OwnedFd, os::unix::net::UnixStream, process::Stdio, time::Instant};
    let started = Instant::now();
    // Separate socketpair ends have separate nonblocking state. The sampler's
    // write end remains blocking; only our read end is nonblocking. Merging both
    // streams also prevents one full stderr pipe from stalling stdout draining.
    let start = |pid| -> io::Result<(Child, UnixStream)> {
        let (output, writer) = UnixStream::pair()?;
        output.set_nonblocking(true)?;
        let error_writer = writer.try_clone()?;
        let child = sample_command(pid)
            .stdin(Stdio::null())
            .stdout(Stdio::from(OwnedFd::from(writer)))
            .stderr(Stdio::from(OwnedFd::from(error_writer)))
            .spawn()?;
        Ok((child, output))
    };
    capture_with(
        target,
        start,
        || started.elapsed(),
        std::thread::sleep,
        |name, bytes| fs::write(directory.join(name), bytes),
        |warning| {
            use std::io::Write;
            // A closed stderr must not turn a best-effort diagnostic into a panic.
            let _ = writeln!(io::stderr().lock(), "{warning}");
        },
    );
}

fn capture_with<S: Sampler, R: Read>(
    target: &mut impl Sampler,
    start: impl FnOnce(u32) -> io::Result<(S, R)>,
    mut now: impl FnMut() -> Duration,
    pause: impl FnMut(Duration),
    write: impl FnMut(&str, &[u8]) -> io::Result<()>,
    warn: impl FnMut(&str),
) {
    // Absolute deadline: setup/spawn time consumes this budget rather than
    // granting the newly spawned sampler a fresh five seconds.
    let deadline = now() + RUN_LIMIT;
    let pid = target.id();
    let before = target_observation(target);
    let (bytes, mut report) = if before["state"] != "running" {
        (
            Vec::new(),
            json!({"skipped":true,"target_observation":before}),
        )
    } else {
        match start(pid) {
            Ok((mut sampler, mut output)) => collect(
                target,
                &mut sampler,
                &mut output,
                now,
                pause,
                deadline,
                REAP_LIMIT,
            ),
            Err(error) => (
                Vec::new(),
                json!({"spawn_error":error_text(&error),"target_observation":before}),
            ),
        }
    };
    report["target_before"] = before;
    report["target_pid"] = json!(pid);
    report["command"] = json!([
        "/usr/bin/sample",
        pid.to_string(),
        "1",
        "10",
        "-mayDie",
        "-file",
        "/dev/stdout"
    ]);
    report["limits"] = json!({"output_bytes":MAX_OUTPUT_BYTES,"report_bytes":MAX_REPORT_BYTES,"run_ms":RUN_LIMIT.as_millis(),"reap_ms":REAP_LIMIT.as_millis()});
    retain(&bytes, report, write, warn);
}

fn retain(
    bytes: &[u8],
    mut report: Value,
    mut write: impl FnMut(&str, &[u8]) -> io::Result<()>,
    mut warn: impl FnMut(&str),
) {
    let output_error = write("host-threads-before-cleanup.sample.txt", bytes).err();
    report["output_file_error"] = json!(output_error.as_ref().map(error_text));
    let result = report_bytes(&report)
        .and_then(|bytes| write("host-threads-before-cleanup.sample.json", &bytes));
    if let Err(error) = result {
        let warning = format!(
            "Owned macOS sampler evidence unavailable: {}; output: {}",
            error_text(&error),
            output_error
                .as_ref()
                .map(error_text)
                .as_deref()
                .unwrap_or("retained")
        );
        warn(&super::bounded_text(&warning, MAX_WARNING_BYTES));
    }
}

fn error_text(error: &io::Error) -> String {
    super::bounded_text(&error.to_string(), 256)
}

fn report_bytes(report: &Value) -> io::Result<Vec<u8>> {
    let bytes = serde_json::to_vec_pretty(report)?;
    if bytes.len() > MAX_REPORT_BYTES {
        return Err(io::Error::other("Sampler receipt exceeds byte limit"));
    }
    Ok(bytes)
}

trait Sampler {
    fn id(&self) -> u32;
    fn poll(&mut self) -> io::Result<Option<Value>>;
    fn kill(&mut self) -> io::Result<()>;
}

impl Sampler for Child {
    fn id(&self) -> u32 {
        Child::id(self)
    }
    fn poll(&mut self) -> io::Result<Option<Value>> {
        self.try_wait().map(|status| status.map(exit_value))
    }
    fn kill(&mut self) -> io::Result<()> {
        Child::kill(self)
    }
}

fn target_observation(target: &mut impl Sampler) -> Value {
    match target.poll() {
        Ok(Some(exit)) => json!({"state":"exited","exit":exit}),
        Ok(None) => json!({"state":"running"}),
        Err(error) => json!({"state":"unavailable","error":error_text(&error)}),
    }
}

fn exit_value(status: ExitStatus) -> Value {
    json!({"success":status.success(),"code":status.code(),"status":status.to_string()})
}

#[derive(Default)]
struct Output {
    bytes: Vec<u8>,
    seen: u64,
    eof: bool,
    error: Option<String>,
}
impl Output {
    fn drain(&mut self, reader: &mut impl Read) {
        if self.eof || self.error.is_some() {
            return;
        }
        // Bound each turn as well as retained bytes: endless ready output or
        // Interrupted reads cannot starve the child/deadline checks.
        let mut buffer = [0_u8; 4096];
        for _ in 0..16 {
            match reader.read(&mut buffer) {
                Ok(0) => {
                    self.eof = true;
                    break;
                }
                Ok(count) => {
                    self.seen = self.seen.saturating_add(count as u64);
                    let keep = count.min(MAX_OUTPUT_BYTES - self.bytes.len());
                    self.bytes.extend_from_slice(&buffer[..keep]);
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    break
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => {
                    self.error = Some(error_text(&error));
                    break;
                }
            }
        }
    }
}

/// The production reader must be nonblocking. No background reader/join can
/// outlive this function, even if another process retains an inherited writer.
fn collect(
    target: &mut impl Sampler,
    sampler: &mut impl Sampler,
    reader: &mut impl Read,
    mut now: impl FnMut() -> Duration,
    mut pause: impl FnMut(Duration),
    deadline: Duration,
    reap_limit: Duration,
) -> (Vec<u8>, Value) {
    let mut output = Output::default();
    let mut exit = None;
    let mut wait_error = None;
    let mut timed_out = false;
    let mut drain_timed_out = false;
    let mut target_observed;
    let mut stopped_for_target;
    loop {
        target_observed = target_observation(target);
        stopped_for_target = target_observed["state"] != "running";
        if stopped_for_target {
            break;
        }
        output.drain(reader);
        if exit.is_none() {
            match sampler.poll() {
                Ok(status) => exit = status,
                Err(error) => {
                    wait_error = Some(error_text(&error));
                    break;
                }
            }
        }
        if exit.is_some() && output.eof {
            break;
        }
        if output.error.is_some() {
            break;
        }
        let remaining = deadline.saturating_sub(now());
        if remaining.is_zero() {
            timed_out = exit.is_none();
            drain_timed_out = !output.eof;
            break;
        }
        pause(remaining.min(POLL));
    }
    // The GUI can exit before this slice's sampler poll. Re-observe the owned
    // sampler before considering kill; a cached/reaped exit never gets a kill.
    if exit.is_none() {
        match sampler.poll() {
            Ok(status) => exit = status,
            Err(error) => {
                wait_error.get_or_insert_with(|| error_text(&error));
            }
        }
    }
    let kill_attempted = exit.is_none();
    let kill_error = if kill_attempted {
        sampler.kill().err().map(|error| error_text(&error))
    } else {
        None
    };
    // try_wait reaps an exited child. Do not introduce an unbounded wait here;
    // inability to kill/reap is retained and the GUI cleanup must continue.
    let reap_deadline = now() + reap_limit;
    if kill_attempted {
        loop {
            if !stopped_for_target {
                target_observed = target_observation(target);
                stopped_for_target = target_observed["state"] != "running";
            }
            output.drain(reader);
            if exit.is_none() {
                match sampler.poll() {
                    Ok(status) => exit = status,
                    Err(error) => {
                        wait_error.get_or_insert_with(|| error_text(&error));
                    }
                }
            }
            if exit.is_some() && (output.eof || output.error.is_some() || stopped_for_target) {
                break;
            }
            let remaining = reap_deadline.saturating_sub(now());
            if remaining.is_zero() {
                drain_timed_out |= !output.eof;
                break;
            }
            pause(remaining.min(POLL));
        }
    }
    let report = json!({
        "sampler_pid":sampler.id(),"exit":exit,"reaped":exit.is_some(),
        "timed_out":timed_out,"drain_timed_out":drain_timed_out,
        "kill_attempted":kill_attempted,"kill_error":kill_error,"wait_error":wait_error,
        "target_observation":target_observed,"stopped_for_target":stopped_for_target,
        "output_bytes":output.bytes.len(),"observed_bytes":output.seen,
        "output_truncated":output.seen>output.bytes.len() as u64,
        "output_eof":output.eof,"read_error":output.error
    });
    (output.bytes, report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[derive(Default)]
    struct FakeSampler {
        polls: usize,
        exit_after: usize,
        exit_code: i32,
        kills: usize,
        kill_fails: bool,
        wait_fails: bool,
    }
    impl Sampler for FakeSampler {
        fn id(&self) -> u32 {
            731
        }
        fn poll(&mut self) -> io::Result<Option<Value>> {
            self.polls += 1;
            if self.wait_fails {
                return Err(io::Error::other("poll failed"));
            }
            Ok((self.polls >= self.exit_after)
                .then(|| json!({"code":self.exit_code,"success":self.exit_code==0})))
        }
        fn kill(&mut self) -> io::Result<()> {
            self.kills += 1;
            if self.kill_fails {
                Err(io::Error::other("kill failed"))
            } else {
                self.exit_after = self.polls + 1;
                Ok(())
            }
        }
    }
    fn run(sampler: &mut FakeSampler, reader: &mut impl Read) -> (Vec<u8>, Value, Duration) {
        let elapsed = Cell::new(Duration::ZERO);
        let mut target = live_target();
        let (bytes, report) = collect(
            &mut target,
            sampler,
            reader,
            || elapsed.get(),
            |duration| elapsed.set(elapsed.get() + duration),
            RUN_LIMIT,
            REAP_LIMIT,
        );
        assert_eq!(
            target.kills, 0,
            "Only the sampler may be killed by diagnostics"
        );
        (bytes, report, elapsed.get())
    }
    fn live_target() -> FakeSampler {
        FakeSampler {
            exit_after: usize::MAX,
            ..Default::default()
        }
    }
    struct ForeverReady;
    impl Read for ForeverReady {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            bytes.fill(b'x');
            Ok(bytes.len())
        }
    }
    struct Pending;
    impl Read for Pending {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::ErrorKind::WouldBlock.into())
        }
    }

    #[test]
    fn exact_owned_pid_command_has_no_wait_or_gui_flags() {
        let command = sample_command(731);
        assert_eq!(command.get_program(), "/usr/bin/sample");
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            ["731", "1", "10", "-mayDie", "-file", "/dev/stdout"]
        );
    }
    #[test]
    fn receipt_byte_cap_and_error_text_bounds_are_enforced() {
        let error = io::Error::other("語".repeat(4096));
        let text = error_text(&error);
        assert!(text.len() <= 256);
        let report =
            json!({"spawn_error":text,"wait_error":text,"kill_error":text,"read_error":text});
        assert!(report_bytes(&report).unwrap().len() <= MAX_REPORT_BYTES);
        assert!(
            report_bytes(&json!({"unexpected_unbounded_field":"x".repeat(MAX_REPORT_BYTES)}))
                .is_err()
        );
    }
    #[test]
    fn caps_retention_but_drains_excess_until_eof_and_reaps_success() {
        let mut sampler = FakeSampler {
            exit_after: 8,
            ..Default::default()
        };
        let mut reader = io::Cursor::new(vec![b'x'; MAX_OUTPUT_BYTES + 4096]);
        let (bytes, report, _) = run(&mut sampler, &mut reader);
        assert_eq!(bytes.len(), MAX_OUTPUT_BYTES);
        assert_eq!(reader.position(), (MAX_OUTPUT_BYTES + 4096) as u64);
        assert_eq!(report["output_truncated"], true);
        assert_eq!(report["output_eof"], true);
        assert_eq!(report["reaped"], true);
        assert_eq!(sampler.kills, 0);
    }
    #[test]
    fn nonzero_command_exit_is_retained_without_killing_or_failing_collection() {
        let mut sampler = FakeSampler {
            exit_after: 1,
            exit_code: 255,
            ..Default::default()
        };
        let (bytes, report, _) = run(&mut sampler, &mut io::Cursor::new(b"permission denied"));
        assert_eq!(bytes, b"permission denied");
        assert_eq!(report["exit"]["code"], 255);
        assert_eq!(report["exit"]["success"], false);
        assert_eq!(sampler.kills, 0);
    }
    #[test]
    fn endless_output_does_not_starve_deadline_or_owned_kill_and_reap() {
        let mut sampler = FakeSampler {
            exit_after: usize::MAX,
            ..Default::default()
        };
        let (bytes, report, elapsed) = run(&mut sampler, &mut ForeverReady);
        assert_eq!(bytes.len(), MAX_OUTPUT_BYTES);
        assert_eq!(report["timed_out"], true);
        assert_eq!(report["reaped"], true);
        assert_eq!(sampler.kills, 1);
        assert!(elapsed <= RUN_LIMIT + REAP_LIMIT);
    }
    #[test]
    fn inherited_open_writer_cannot_hold_cleanup_after_sampler_exit() {
        let mut sampler = FakeSampler {
            exit_after: 1,
            ..Default::default()
        };
        let (_, report, elapsed) = run(&mut sampler, &mut Pending);
        assert_eq!(report["timed_out"], false);
        assert_eq!(report["drain_timed_out"], true);
        assert_eq!(sampler.kills, 0);
        assert_eq!(elapsed, RUN_LIMIT);
    }
    #[test]
    fn kill_and_wait_failures_remain_bounded_receipts() {
        let mut sampler = FakeSampler {
            exit_after: usize::MAX,
            kill_fails: true,
            wait_fails: true,
            ..Default::default()
        };
        let (_, report, elapsed) = run(&mut sampler, &mut Pending);
        assert_eq!(sampler.kills, 1);
        assert_eq!(report["reaped"], false);
        assert_eq!(report["wait_error"], "poll failed");
        assert_eq!(report["kill_error"], "kill failed");
        assert_eq!(elapsed, REAP_LIMIT);
    }
    #[test]
    fn read_failure_cleans_up_sampler_and_is_not_reported_as_eof() {
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("read failed"))
            }
        }
        let mut sampler = FakeSampler {
            exit_after: usize::MAX,
            ..Default::default()
        };
        let (_, report, _) = run(&mut sampler, &mut Broken);
        assert_eq!(report["read_error"], "read failed");
        assert_eq!(report["output_eof"], false);
        assert_eq!(report["reaped"], true);
        assert_eq!(sampler.kills, 1);
    }
    #[test]
    fn already_exited_target_is_recorded_without_spawning_sampler() {
        let mut target = FakeSampler {
            exit_after: 1,
            ..Default::default()
        };
        let spawned = Cell::new(false);
        let mut receipt = Value::Null;
        capture_with(
            &mut target,
            |_| {
                spawned.set(true);
                Ok((live_target(), io::empty()))
            },
            || Duration::ZERO,
            |_| panic!("No wait for an exited GUI"),
            |name, bytes| {
                if name.ends_with(".json") {
                    receipt = serde_json::from_slice(bytes).unwrap();
                }
                Ok(())
            },
            |_| panic!("Receipt retention should succeed"),
        );
        assert!(!spawned.get());
        assert_eq!(receipt["skipped"], true);
        assert_eq!(receipt["target_before"]["state"], "exited");
        assert_eq!(target.kills, 0);
    }
    #[test]
    fn target_exit_mid_sample_stops_and_reaps_only_sampler_on_next_slice() {
        let elapsed = Cell::new(Duration::ZERO);
        let mut target = FakeSampler {
            exit_after: 4,
            ..Default::default()
        };
        let mut sampler = live_target();
        let (_, report) = collect(
            &mut target,
            &mut sampler,
            &mut Pending,
            || elapsed.get(),
            |duration| elapsed.set(elapsed.get() + duration),
            RUN_LIMIT,
            REAP_LIMIT,
        );
        assert_eq!(report["target_observation"]["state"], "exited");
        assert_eq!(report["stopped_for_target"], true);
        assert_eq!(report["reaped"], true);
        assert_eq!(sampler.kills, 1);
        assert_eq!(target.kills, 0);
        assert!(elapsed.get() <= POLL * 4);
    }
    #[test]
    fn setup_elapsed_consumes_absolute_sampler_deadline() {
        let elapsed = Cell::new(Duration::ZERO);
        let mut target = live_target();
        let mut receipt = Value::Null;
        capture_with(
            &mut target,
            |_| {
                elapsed.set(RUN_LIMIT + Duration::from_secs(1));
                Ok((live_target(), io::empty()))
            },
            || elapsed.get(),
            |duration| elapsed.set(elapsed.get() + duration),
            |name, bytes| {
                if name.ends_with(".json") {
                    receipt = serde_json::from_slice(bytes).unwrap();
                }
                Ok(())
            },
            |_| panic!("Receipt retention should succeed"),
        );
        assert_eq!(receipt["timed_out"], true);
        assert_eq!(receipt["kill_attempted"], true);
        assert_eq!(receipt["reaped"], true);
        assert_eq!(elapsed.get(), RUN_LIMIT + Duration::from_secs(1));
    }
    #[test]
    fn delayed_would_block_keeps_later_partial_bytes_and_eof() {
        struct Delayed<'a> {
            elapsed: &'a Cell<Duration>,
            part: usize,
        }
        impl Read for Delayed<'_> {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                if self.elapsed.get() < Duration::from_millis(1500) {
                    return Err(io::ErrorKind::WouldBlock.into());
                }
                let parts: [&[u8]; 3] = [b"par", b"tial", b""];
                let bytes = parts[self.part.min(2)];
                buffer[..bytes.len()].copy_from_slice(bytes);
                self.part += 1;
                Ok(bytes.len())
            }
        }
        let elapsed = Cell::new(Duration::ZERO);
        let mut target = live_target();
        let mut sampler = FakeSampler {
            exit_after: 153,
            ..Default::default()
        };
        let mut reader = Delayed {
            elapsed: &elapsed,
            part: 0,
        };
        let (bytes, report) = collect(
            &mut target,
            &mut sampler,
            &mut reader,
            || elapsed.get(),
            |duration| elapsed.set(elapsed.get() + duration),
            RUN_LIMIT,
            REAP_LIMIT,
        );
        assert_eq!(bytes, b"partial");
        assert_eq!(report["output_eof"], true);
        assert_eq!(report["timed_out"], false);
        assert_eq!(sampler.kills, 0);
        assert!(elapsed.get() >= Duration::from_millis(1500));
        assert!(elapsed.get() < RUN_LIMIT);
    }
    #[test]
    fn interrupted_bursts_preserve_bytes_without_starving_deadline() {
        struct Interrupted {
            count: usize,
            bytes: io::Cursor<&'static [u8]>,
        }
        impl Read for Interrupted {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                if self.count > 0 {
                    self.count -= 1;
                    return Err(io::ErrorKind::Interrupted.into());
                }
                self.bytes.read(buffer)
            }
        }
        let mut sampler = FakeSampler {
            exit_after: 1,
            ..Default::default()
        };
        let (bytes, report, elapsed) = run(
            &mut sampler,
            &mut Interrupted {
                count: 100,
                bytes: io::Cursor::new(b"after-interrupts"),
            },
        );
        assert_eq!(bytes, b"after-interrupts");
        assert_eq!(report["output_eof"], true);
        assert!(elapsed < RUN_LIMIT);
        assert_eq!(sampler.kills, 0);
    }
    #[test]
    fn read_failure_after_bytes_keeps_partial_evidence_and_reaps_sampler() {
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::ErrorKind::ConnectionReset.into())
            }
        }
        let mut sampler = live_target();
        let (bytes, report, _) = run(
            &mut sampler,
            &mut io::Cursor::new(b"ten bytes!").chain(Broken),
        );
        assert_eq!(bytes, b"ten bytes!");
        assert!(report["read_error"].is_string());
        assert_eq!(report["output_eof"], false);
        assert_eq!(report["reaped"], true);
        assert_eq!(sampler.kills, 1);
    }
    #[test]
    fn output_write_failure_is_retained_in_sidecar_without_returning_an_error() {
        let mut target = live_target();
        let mut receipt = Value::Null;
        capture_with(
            &mut target,
            |_| {
                Ok((
                    FakeSampler {
                        exit_after: 1,
                        ..Default::default()
                    },
                    io::empty(),
                ))
            },
            || Duration::ZERO,
            |_| panic!("Already exited sampler"),
            |name, bytes| {
                if name.ends_with(".txt") {
                    return Err(io::Error::other("output write failed"));
                }
                receipt = serde_json::from_slice(bytes).unwrap();
                Ok(())
            },
            |_| panic!("Sidecar retained the output failure"),
        );
        assert_eq!(receipt["output_file_error"], "output write failed");
        assert_eq!(target.kills, 0);
    }
    #[test]
    fn sidecar_and_command_failure_produce_only_bounded_warning() {
        let mut target = live_target();
        let mut warnings = Vec::new();
        capture_with(
            &mut target,
            |_| {
                Err::<(FakeSampler, io::Empty), _>(io::Error::new(
                    io::ErrorKind::NotFound,
                    "sampler unavailable",
                ))
            },
            || Duration::ZERO,
            |_| panic!("Failed spawn has no wait"),
            |_, _| Err(io::Error::other("write failed ".repeat(4096))),
            |warning| warnings.push(warning.to_owned()),
        );
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].len() <= MAX_WARNING_BYTES);
        assert_eq!(target.kills, 0);
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn owned_sampler_adapter_smoke() {
        use std::{fs, process::Stdio, time::Instant};
        struct OwnedTarget(Child);
        impl Drop for OwnedTarget {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let mut target = OwnedTarget(
            // Keep adapter qualification independent of the large debug test image.
            // This is our spawned child, never a name-based lookup or existing process.
            Command::new("/bin/sleep")
                .arg("30")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let pid = target.0.id();
        let started = Instant::now();
        super::capture(directory.path(), &mut target.0);
        let sampling_elapsed = started.elapsed();
        // Explicit CI-only opt-in: retain only these two owned files before
        // validation can fail and TempDir removes the local fixture evidence.
        let mut proof_errors = Vec::new();
        if let Some(output) = std::env::var_os("LIMO_CAD_MACOS_SAMPLER_PROOF_DIR") {
            let output = std::path::PathBuf::from(output);
            match fs::create_dir_all(&output) {
                Ok(()) => {
                    for name in [
                        "host-threads-before-cleanup.sample.txt",
                        "host-threads-before-cleanup.sample.json",
                    ] {
                        if let Err(error) = fs::copy(directory.path().join(name), output.join(name))
                        {
                            proof_errors.push(format!("{name}: {error}"));
                        }
                    }
                }
                Err(error) => proof_errors.push(error.to_string()),
            }
        }
        assert!(
            proof_errors.is_empty(),
            "Retain adapter proof: {proof_errors:?}"
        );
        assert!(sampling_elapsed < RUN_LIMIT + REAP_LIMIT);
        let sample = fs::read(
            directory
                .path()
                .join("host-threads-before-cleanup.sample.txt"),
        )
        .unwrap();
        let receipt_bytes = fs::read(
            directory
                .path()
                .join("host-threads-before-cleanup.sample.json"),
        )
        .unwrap();
        assert!(!sample.is_empty() && sample.len() <= MAX_OUTPUT_BYTES);
        assert!(sample
            .windows(b"Call graph:".len())
            .any(|part| part == b"Call graph:"));
        assert!(!receipt_bytes.is_empty() && receipt_bytes.len() <= MAX_REPORT_BYTES);
        let receipt: Value = serde_json::from_slice(&receipt_bytes).unwrap();
        assert_eq!(receipt["target_pid"], pid);
        assert_eq!(receipt["exit"]["success"], true);
        assert_eq!(receipt["reaped"], true);
        assert_eq!(receipt["timed_out"], false);
        assert_eq!(receipt["kill_attempted"], false);
        assert_eq!(receipt["output_eof"], true);
        assert!(receipt["read_error"].is_null());
        assert_eq!(receipt["command"][5], "-file");
        assert_eq!(receipt["command"][6], "/dev/stdout");
        assert!(
            target.0.try_wait().unwrap().is_none(),
            "Sampling must leave the owned fixture running"
        );
        // The owned target guard cleans only this helper; TempDir cleans only
        // its explicit fixture files. No global tmp scan or cleanup is used.
    }
    #[test]
    fn actual_owned_stalled_cli_is_killed_and_reaped() {
        use std::{process::Stdio, time::Instant};
        #[cfg(windows)]
        let mut command = {
            use std::os::windows::process::CommandExt;
            let mut command = Command::new("powershell.exe");
            command.args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Start-Sleep -Seconds 30",
            ]);
            command.creation_flags(0x08000000);
            command
        };
        #[cfg(not(windows))]
        let mut command = {
            let mut command = Command::new("/bin/sleep");
            command.arg("30");
            command
        };
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let started = Instant::now();
        let mut target = live_target();
        let (_, report) = collect(
            &mut target,
            &mut child,
            &mut io::empty(),
            || started.elapsed(),
            std::thread::sleep,
            Duration::from_millis(50),
            REAP_LIMIT,
        );
        assert_eq!(report["sampler_pid"], child.id());
        assert_eq!(report["timed_out"], true);
        assert_eq!(report["kill_attempted"], true);
        assert_eq!(report["reaped"], true);
        assert!(child.try_wait().unwrap().is_some());
        assert!(started.elapsed() < Duration::from_secs(4));
    }
}
