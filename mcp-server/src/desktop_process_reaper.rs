//! Keep launched desktops independent while collecting their eventual exits.
//! One reusable worker polls dropped children; no request waits for a window.

use std::{
    io::{self, Write},
    process::{Child, Command, ExitStatus},
    sync::{
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
        Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

const POLL_INTERVAL: Duration = Duration::from_millis(250);
static REAPER: Mutex<Option<Sender<Child>>> = Mutex::new(None);

pub(crate) struct DesktopChild {
    child: Option<Child>,
    pid: u32,
    exited: bool,
    reaper: Sender<Child>,
}

impl DesktopChild {
    pub(crate) fn id(&self) -> u32 {
        self.pid
    }

    pub(crate) fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        let status = self.child.as_mut().unwrap().try_wait()?;
        self.exited = status.is_some();
        Ok(status)
    }
}

impl Drop for DesktopChild {
    fn drop(&mut self) {
        if !self.exited {
            if let Some(child) = self.child.take() {
                // The worker exits only after every sender has gone away. This
                // sender keeps it available while ownership is transferred.
                if self.reaper.send(child).is_err() {
                    let _ = writeln!(
                        io::stderr(),
                        "CAD child reaper stopped for PID {}",
                        self.pid
                    );
                }
            }
        }
    }
}

pub(crate) fn spawn(command: &mut Command) -> io::Result<DesktopChild> {
    // Establish the worker before creating a process which needs its owner.
    spawn_with_reaper(command, shared_reaper())
}

fn spawn_with_reaper(
    command: &mut Command,
    reaper: io::Result<Sender<Child>>,
) -> io::Result<DesktopChild> {
    let reaper = reaper?;
    let child = command.spawn()?;
    Ok(DesktopChild {
        pid: child.id(),
        child: Some(child),
        exited: false,
        reaper,
    })
}

fn shared_reaper() -> io::Result<Sender<Child>> {
    let mut shared = REAPER
        .lock()
        .map_err(|_| io::Error::other("CAD child reaper initialization lock was poisoned"))?;
    if let Some(reaper) = shared.as_ref() {
        return Ok(reaper.clone());
    }
    let (reaper, worker) = start_reaper(|_, _| {})?;
    drop(worker);
    *shared = Some(reaper.clone());
    Ok(reaper)
}

fn start_reaper(
    on_exit: impl FnMut(u32, ExitStatus) + Send + 'static,
) -> io::Result<(Sender<Child>, JoinHandle<()>)> {
    let (sender, receiver) = mpsc::channel();
    let worker = thread::Builder::new()
        .name("limo-cad-desktop-reaper".into())
        .spawn(move || reap_children(receiver, on_exit))?;
    Ok((sender, worker))
}

fn reap_children(receiver: Receiver<Child>, mut on_exit: impl FnMut(u32, ExitStatus)) {
    let mut children: Vec<Child> = Vec::new();
    let mut disconnected = false;
    loop {
        if disconnected && children.is_empty() {
            return;
        }
        if !disconnected {
            let next = if children.is_empty() {
                receiver.recv().map_err(|_| RecvTimeoutError::Disconnected)
            } else {
                receiver.recv_timeout(POLL_INTERVAL)
            };
            match next {
                Ok(child) => children.push(child),
                Err(RecvTimeoutError::Disconnected) => disconnected = true,
                Err(RecvTimeoutError::Timeout) => {}
            }
            // Bound intake between sweeps so a busy launcher cannot postpone
            // collection of an already exited desktop.
            children.extend(receiver.try_iter().take(63));
        }
        children.retain_mut(|child| match child.try_wait() {
            Ok(Some(status)) => {
                on_exit(child.id(), status);
                false
            }
            Ok(None) => true,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => true,
            Err(error) => {
                let _ = writeln!(
                    io::stderr(),
                    "Could not collect launched CAD process {}: {error}",
                    child.id()
                );
                false
            }
        });
        if disconnected && !children.is_empty() {
            thread::sleep(POLL_INTERVAL);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::PathBuf,
        process::Stdio,
        time::{Instant, SystemTime, UNIX_EPOCH},
    };

    struct FixtureDirectory(PathBuf);
    impl FixtureDirectory {
        fn new(name: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "limo-cad-reaper-{name}-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn command(&self) -> Command {
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args(["subprocess_reaper_fixture", "--ignored", "--nocapture"])
                .env("LIMO_CAD_REAPER_FIXTURE", &self.0)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x08000000);
            }
            command
        }

        fn release(&self) {
            fs::write(self.0.join("release"), b"").unwrap();
        }

        fn wait_ready(&self) {
            let deadline = Instant::now() + Duration::from_secs(5);
            while !self.0.join("ready").exists() {
                assert!(
                    Instant::now() < deadline,
                    "Child fixture did not become ready"
                );
                thread::sleep(Duration::from_millis(5));
            }
        }
    }

    impl Drop for FixtureDirectory {
        fn drop(&mut self) {
            // Release the exclusively owned helper even if an assertion fails.
            let _ = fs::write(self.0.join("release"), b"");
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    #[ignore = "owned child fixture invoked only by reaper tests"]
    fn subprocess_reaper_fixture() {
        let Some(directory) = std::env::var_os("LIMO_CAD_REAPER_FIXTURE") else {
            return;
        };
        let directory = PathBuf::from(directory);
        fs::write(directory.join("ready"), b"").unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !directory.join("release").exists() {
            assert!(Instant::now() < deadline, "Child fixture was not released");
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn dropped_children_survive_and_are_collected_independently() {
        let (exits, observed) = mpsc::channel();
        let (reaper, worker) = start_reaper(move |pid, status| {
            let _ = exits.send((pid, status));
        })
        .unwrap();
        let first_fixture = FixtureDirectory::new("first");
        let second_fixture = FixtureDirectory::new("second");
        let first = spawn_with_reaper(&mut first_fixture.command(), Ok(reaper.clone())).unwrap();
        let second = spawn_with_reaper(&mut second_fixture.command(), Ok(reaper.clone())).unwrap();
        let first_pid = first.id();
        let second_pid = second.id();
        first_fixture.wait_ready();
        second_fixture.wait_ready();
        drop(first);
        drop(second);
        assert!(matches!(
            observed.recv_timeout(Duration::from_millis(50)),
            Err(RecvTimeoutError::Timeout)
        ));

        second_fixture.release();
        let (pid, status) = observed.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(
            pid, second_pid,
            "The first live desktop must not block reaping"
        );
        assert!(status.success());
        first_fixture.release();
        let (pid, status) = observed.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(pid, first_pid);
        assert!(status.success());
        drop(reaper);
        worker.join().unwrap();
    }

    #[test]
    fn try_wait_preserves_exit_status_and_does_not_transfer_reaped_children() {
        let (exits, observed) = mpsc::channel();
        let (reaper, worker) = start_reaper(move |pid, _| {
            let _ = exits.send(pid);
        })
        .unwrap();
        let fixture = FixtureDirectory::new("waited");
        fixture.release();
        let mut child = spawn_with_reaper(&mut fixture.command(), Ok(reaper.clone())).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            assert!(Instant::now() < deadline, "Released child did not exit");
            thread::sleep(Duration::from_millis(5));
        };
        assert!(status.success());
        assert_eq!(child.try_wait().unwrap(), Some(status));
        drop(child);
        drop(reaper);
        worker.join().unwrap();
        assert!(matches!(
            observed.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }

    #[test]
    fn reaper_start_failure_is_returned_before_spawning_a_child() {
        let mut command = Command::new("limo-cad-reaper-nonexistent-executable");
        let error = spawn_with_reaper(
            &mut command,
            Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "reaper unavailable",
            )),
        )
        .err()
        .unwrap();
        assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
        assert_eq!(error.to_string(), "reaper unavailable");
    }
}
