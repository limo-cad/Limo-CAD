use super::*;
use std::{
    fs::{self, File},
    io::Read,
    os::windows::process::CommandExt,
    path::PathBuf,
    process::Command,
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use windows_sys::Win32::{
    Foundation::{SetHandleInformation, HANDLE_FLAG_INHERIT},
    Security::SECURITY_ATTRIBUTES,
    System::{Pipes::CreatePipe, Threading::TerminateProcess},
};

struct TestDirectory(PathBuf);
impl TestDirectory {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "nbcad desktop spawn Ω {} {nonce}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }
}
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct OwnedChild(DesktopChild);
impl Drop for OwnedChild {
    fn drop(&mut self) {

        if self.0.try_wait().ok().flatten().is_none() {
            unsafe {
                TerminateProcess(self.0.process.as_raw_handle(), 1);
                WaitForSingleObject(self.0.process.as_raw_handle(), 5000);
            }
        }
    }
}

fn fixture(directory: &Path) -> PathBuf {
    let source = directory.join("desktop_fixture.rs");
    let executable = directory.join("Desktop fixture Ω.exe");
    let code = r#"
use std::{env, fs, path::Path, thread, time::{Duration, Instant}};
fn main() {
    let root = Path::new(ROOT_PATH);
    fs::write(root.join("ready.tmp"), format!("{}\n{}\n{}",
        env::args_os().count(), env::current_dir().unwrap().display(),
        env::var("PATH").unwrap_or_default())).unwrap();
    fs::rename(root.join("ready.tmp"), root.join("ready")).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while !root.join("stop").exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    std::process::exit(259);
}
"#;
    fs::write(
        &source,
        code.replace("ROOT_PATH", &format!("{:?}", directory.to_str().unwrap())),
    )
    .unwrap();
    let compile = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
        .args(["--edition=2021", "--crate-name", "desktop_fixture"])
        .arg(&source)
        .arg("-o")
        .arg(&executable)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    executable
}

#[test]
fn desktop_does_not_retain_launcher_pipes_and_exit_259_is_observed() {
    let temp = TestDirectory::new();
    let executable = fixture(&temp.0);
    let mut read = ptr::null_mut();
    let mut write = ptr::null_mut();
    let security = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: ptr::null_mut(),
        bInheritHandle: 1,
    };
    assert_ne!(
        unsafe { CreatePipe(&mut read, &mut write, &security, 0) },
        0,
        "{}",
        io::Error::last_os_error()
    );
    let read = unsafe { OwnedHandle::from_raw_handle(read) };
    let write = unsafe { OwnedHandle::from_raw_handle(write) };
    assert_ne!(
        unsafe { SetHandleInformation(read.as_raw_handle(), HANDLE_FLAG_INHERIT, 0) },
        0
    );

    let mut child = OwnedChild(spawn(&executable).unwrap());
    assert!(child.0.id() > 0);
    drop(write);
    let (sent, received) = mpsc::channel();
    let reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = File::from(read).read_to_end(&mut bytes);
        let _ = sent.send(result);
    });
    assert_eq!(
        received
            .recv_timeout(Duration::from_secs(2))
            .expect("Desktop retained an inherited pipe after the launcher closed it")
            .unwrap(),
        0
    );
    reader.join().unwrap();
    assert!(child.0.try_wait().unwrap().is_none());
    let deadline = Instant::now() + Duration::from_secs(5);
    while !temp.0.join("ready").exists() {
        assert!(Instant::now() < deadline, "Fixture did not become ready");
        thread::sleep(Duration::from_millis(10));
    }
    let expected = format!(
        "1\n{}\n{}",
        temp.0.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    assert_eq!(fs::read_to_string(temp.0.join("ready")).unwrap(), expected);
    fs::write(temp.0.join("stop"), "").unwrap();
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert_eq!(status.code(), Some(259));
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Exited fixture still appears active"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn desktop_spawn_preserves_windows_launch_errors() {
    let temp = TestDirectory::new();
    let error = spawn(&temp.0.join("missing.exe")).err().unwrap();
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    let invalid = temp.0.join("invalid.exe");
    fs::write(&invalid, "This is not a Windows executable").unwrap();
    let os_error = Command::new(&invalid)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .err()
        .unwrap();
    assert!(os_error.raw_os_error().is_some());
    assert_eq!(
        spawn(&invalid).err().unwrap().raw_os_error(),
        os_error.raw_os_error()
    );
}
