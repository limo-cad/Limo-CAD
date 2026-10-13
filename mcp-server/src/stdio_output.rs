//! End a disconnected desktop's MCP output without ending its native window.
//! Only pipes are retired; terminals, files, and absent GUI stdio stay untouched.

#[cfg(unix)]
pub(super) use platform::prepare_stdout_pipe;
pub(super) use platform::retire_stdout_pipe;

#[cfg(windows)]
mod platform {
    use std::{
        fs::File,
        io,
        os::windows::io::{FromRawHandle, IntoRawHandle},
    };
    use windows_sys::Win32::{
        Foundation::{CloseHandle, CompareObjectHandles, HANDLE, INVALID_HANDLE_VALUE},
        Storage::FileSystem::{GetFileType, FILE_TYPE_PIPE},
        System::Console::{GetStdHandle, SetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE},
    };

    pub(crate) fn retire_stdout_pipe() -> io::Result<()> {
        let output = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
        let error = unsafe { GetStdHandle(STD_ERROR_HANDLE) };
        retire_pipe(output, error, |stream, handle| {
            if unsafe { SetStdHandle(stream, handle) } == 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        })?;
        Ok(())
    }

    fn retire_pipe(
        output: HANDLE,
        error: HANDLE,
        mut install: impl FnMut(u32, HANDLE) -> io::Result<()>,
    ) -> io::Result<Option<HANDLE>> {
        if output.is_null()
            || output == INVALID_HANDLE_VALUE
            || unsafe { GetFileType(output) } != FILE_TYPE_PIPE
        {
            return Ok(None);
        }
        let aliased_error = !error.is_null()
            && error != INVALID_HANDLE_VALUE
            && (error == output || unsafe { CompareObjectHandles(output, error) } != 0);
        let sink = File::options().write(true).open("NUL")?.into_raw_handle();
        if let Err(error) = install(STD_OUTPUT_HANDLE, sink) {
            drop(unsafe { File::from_raw_handle(sink) });
            return Err(error);
        }
        if aliased_error {
            install(STD_ERROR_HANDLE, sink)?;
        }
        if unsafe { CloseHandle(output) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if aliased_error && error != output && unsafe { CloseHandle(error) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Some(sink))
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::{
            io::{Read, Write},
            os::windows::io::AsRawHandle,
            sync::mpsc,
            thread,
            time::Duration,
        };
        use windows_sys::Win32::System::Pipes::CreatePipe;

        fn pipe() -> (File, File) {
            let (mut read, mut write) = (std::ptr::null_mut(), std::ptr::null_mut());
            assert_ne!(
                unsafe { CreatePipe(&mut read, &mut write, std::ptr::null(), 0) },
                0
            );
            unsafe { (File::from_raw_handle(read), File::from_raw_handle(write)) }
        }

        fn expect_eof(mut reader: File) {
            let (tx, rx) = mpsc::channel();
            thread::spawn(move || {
                let _ = tx.send(reader.read(&mut [0; 1]));
            });
            assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap().unwrap(), 0);
        }

        #[test]
        fn retiring_owned_output_pipe_delivers_eof_and_leaves_an_inert_sink() {
            for alias in [false, true] {
                let (reader, writer) = pipe();
                let output = writer.into_raw_handle();
                let error = if alias { output } else { std::ptr::null_mut() };
                let mut installed = Vec::new();
                let sink = retire_pipe(output, error, |stream, handle| {
                    installed.push((stream, handle));
                    Ok(())
                })
                .unwrap()
                .unwrap();
                let mut sink = unsafe { File::from_raw_handle(sink) };
                sink.write_all(b"late output is discarded").unwrap();
                assert_eq!(installed.len(), if alias { 2 } else { 1 });
                assert!(installed
                    .iter()
                    .all(|(_, handle)| *handle == sink.as_raw_handle()));
                expect_eof(reader);
            }
        }

        #[test]
        fn duplicated_stderr_alias_is_retired_but_separate_stderr_is_preserved() {
            let (reader, writer) = pipe();
            let alias = writer.try_clone().unwrap().into_raw_handle();
            let output = writer.into_raw_handle();
            let mut installed = Vec::new();
            let sink = retire_pipe(output, alias, |stream, handle| {
                installed.push((stream, handle));
                Ok(())
            })
            .unwrap()
            .unwrap();
            let _sink = unsafe { File::from_raw_handle(sink) };
            assert_eq!(installed.len(), 2);
            expect_eof(reader);

            let (reader, writer) = pipe();
            let (_error_reader, mut error_writer) = pipe();
            let mut installed = Vec::new();
            let sink = retire_pipe(
                writer.into_raw_handle(),
                error_writer.as_raw_handle(),
                |stream, handle| {
                    installed.push((stream, handle));
                    Ok(())
                },
            )
            .unwrap()
            .unwrap();
            let _sink = unsafe { File::from_raw_handle(sink) };
            assert_eq!(installed.len(), 1);
            error_writer.write_all(b"diagnostic").unwrap();
            expect_eof(reader);
        }

        #[test]
        fn absent_or_non_pipe_output_is_unchanged() {
            let mut sink = File::options().write(true).open("NUL").unwrap();
            for handle in [
                std::ptr::null_mut(),
                INVALID_HANDLE_VALUE,
                sink.as_raw_handle(),
            ] {
                assert!(retire_pipe(handle, std::ptr::null_mut(), |_, _| panic!(
                    "replaced non-pipe output"
                ))
                .unwrap()
                .is_none());
            }
            sink.write_all(b"still valid").unwrap();
        }
    }
}

#[cfg(unix)]
mod platform {
    use std::{
        ffi::c_int,
        fs::{File, Metadata},
        io,
        os::fd::{AsRawFd, FromRawFd},
        os::unix::fs::{FileTypeExt, MetadataExt},
    };

    fn descriptor_control(fd: c_int, command: c_int, argument: c_int) -> io::Result<c_int> {
        loop {
            let result = unsafe { libc::fcntl(fd, command, argument) };
            if result >= 0 {
                return Ok(result);
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
    }

    fn inspect(fd: c_int) -> io::Result<Option<File>> {
        match descriptor_control(fd, libc::F_DUPFD_CLOEXEC, 3) {
            Ok(duplicate) => Ok(Some(unsafe { File::from_raw_fd(duplicate) })),
            Err(error) if error.raw_os_error() == Some(libc::EBADF) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn aliased_pipe(error: Option<c_int>, output: &Metadata) -> io::Result<bool> {
        let Some(fd) = error else { return Ok(false) };
        let Some(file) = inspect(fd)? else {
            return Ok(false);
        };
        let metadata = file.metadata()?;
        Ok(metadata.file_type().is_fifo()
            && metadata.dev() == output.dev()
            && metadata.ino() == output.ino())
    }

    fn prepare_pipe(output: c_int, error: Option<c_int>) -> io::Result<bool> {
        let Some(output_file) = inspect(output)? else {
            return Ok(false);
        };
        let metadata = output_file.metadata()?;
        if !metadata.file_type().is_fifo() {
            return Ok(false);
        }
        let alias = aliased_pipe(error, &metadata)?;
        for fd in [Some(output), error.filter(|_| alias)]
            .into_iter()
            .flatten()
        {
            let flags = descriptor_control(fd, libc::F_GETFD, 0)?;
            descriptor_control(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC)?;
        }
        Ok(true)
    }

    pub(crate) fn prepare_stdout_pipe() -> io::Result<()> {
        prepare_pipe(libc::STDOUT_FILENO, Some(libc::STDERR_FILENO)).map(|_| ())
    }

    fn replace(sink: &File, target: c_int) -> io::Result<()> {
        loop {
            if unsafe { libc::dup2(sink.as_raw_fd(), target) } >= 0 {
                return Ok(());
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
    }

    pub(crate) fn retire_stdout_pipe() -> io::Result<()> {
        retire_pipe(1, Some(2)).map(|_| ())
    }

    fn retire_pipe(output: c_int, error: Option<c_int>) -> io::Result<bool> {
        let Some(output_file) = inspect(output)? else {
            return Ok(false);
        };
        let metadata = output_file.metadata()?;
        if !metadata.file_type().is_fifo() {
            return Ok(false);
        }
        let aliased_error = aliased_pipe(error, &metadata)?;
        let sink = File::options().write(true).open("/dev/null")?;
        replace(&sink, output)?;
        if aliased_error {
            replace(&sink, error.unwrap())?;
        }

        Ok(true)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::{
            io::{BufRead, BufReader, Read, Write},
            process::{Child, Command, Stdio},
            sync::mpsc,
            thread,
            time::{Duration, Instant},
        };

        fn owned_pipe() -> (File, File) {
            let mut fds = [-1; 2];
            assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
            unsafe { (File::from_raw_fd(fds[0]), File::from_raw_fd(fds[1])) }
        }

        fn expect_eof(mut reader: File) {
            let (tx, rx) = mpsc::channel();
            thread::spawn(move || {
                let _ = tx.send(reader.read(&mut [0; 1]));
            });
            assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap().unwrap(), 0);
        }

        #[test]
        fn retiring_owned_output_pipe_delivers_eof_and_leaves_an_inert_sink() {
            let (reader, mut writer) = owned_pipe();
            let mut error = writer.try_clone().unwrap();
            assert!(retire_pipe(writer.as_raw_fd(), Some(error.as_raw_fd())).unwrap());
            writer.write_all(b"discard").unwrap();
            error.write_all(b"discard aliased error").unwrap();
            expect_eof(reader);
        }

        #[test]
        fn non_pipe_output_and_separate_stderr_are_unchanged() {
            let mut sink = File::options().write(true).open("/dev/null").unwrap();
            assert!(!retire_pipe(sink.as_raw_fd(), None).unwrap());
            sink.write_all(b"still valid").unwrap();
            let (reader, writer) = owned_pipe();
            let (_error_reader, mut error) = owned_pipe();
            assert!(retire_pipe(writer.as_raw_fd(), Some(error.as_raw_fd())).unwrap());
            error.write_all(b"diagnostic").unwrap();
            expect_eof(reader);
        }

        #[test]
        fn inheritance_preparation_preserves_non_pipes_and_unrelated_stderr() {
            assert!(inspect(-1).unwrap().is_none());
            assert!(!prepare_pipe(-1, Some(-1)).unwrap());
            assert!(!retire_pipe(-1, Some(-1)).unwrap());
            let sink = File::options().write(true).open("/dev/null").unwrap();
            let before = descriptor_control(sink.as_raw_fd(), libc::F_GETFD, 0).unwrap();
            assert!(!prepare_pipe(sink.as_raw_fd(), None).unwrap());
            assert_eq!(
                descriptor_control(sink.as_raw_fd(), libc::F_GETFD, 0).unwrap(),
                before
            );

            let (_reader, writer) = owned_pipe();
            let (_error_reader, error) = owned_pipe();
            let error_flags = descriptor_control(error.as_raw_fd(), libc::F_GETFD, 0).unwrap();
            assert!(prepare_pipe(writer.as_raw_fd(), Some(error.as_raw_fd())).unwrap());
            assert_eq!(
                descriptor_control(error.as_raw_fd(), libc::F_GETFD, 0).unwrap(),
                error_flags
            );
            let duplicate = inspect(writer.as_raw_fd()).unwrap().unwrap();
            assert_ne!(
                descriptor_control(duplicate.as_raw_fd(), libc::F_GETFD, 0).unwrap()
                    & libc::FD_CLOEXEC,
                0
            );
        }

        struct OwnedChild(Child);
        impl Drop for OwnedChild {
            fn drop(&mut self) {
                if self.0.try_wait().ok().flatten().is_none() {
                    let _ = self.0.kill();
                    let _ = self.0.wait();
                }
            }
        }

        #[test]
        fn subprocess_stdio_helper() {
            let Ok(mode) = std::env::var("LIMO_CAD_STDIO_PIPE_HELPER") else {
                return;
            };
            let mut diagnostics = inspect(libc::STDERR_FILENO).unwrap().unwrap();
            if mode == "alias" {
                assert_eq!(
                    unsafe { libc::dup2(libc::STDOUT_FILENO, libc::STDERR_FILENO) },
                    libc::STDERR_FILENO
                );
            }
            prepare_stdout_pipe().unwrap();

            let mut helper = OwnedChild(
                Command::new("/bin/sh")
                    .args(["-c", "IFS= read -r release || :"])
                    .stdin(Stdio::piped())
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::inherit())
                    .spawn()
                    .unwrap(),
            );
            println!("mcp-response");
            io::stdout().flush().unwrap();
            writeln!(diagnostics, "helper-ready").unwrap();
            diagnostics.flush().unwrap();
            let mut control = io::stdin().lock();
            let mut line = String::new();
            control.read_line(&mut line).unwrap();
            assert_eq!(line.trim(), "retire");
            retire_stdout_pipe().unwrap();
            assert!(helper.0.try_wait().unwrap().is_none());
            writeln!(diagnostics, "retired-with-helper-alive").unwrap();
            diagnostics.flush().unwrap();
            line.clear();
            control.read_line(&mut line).unwrap();
            assert_eq!(line.trim(), "release");
            drop(helper.0.stdin.take());
            assert!(helper.0.wait().unwrap().success());
        }

        #[test]
        fn desktop_helpers_cannot_keep_retired_stdout_open() {
            for mode in ["separate", "alias"] {
                let mut desktop = OwnedChild(
                    Command::new(std::env::current_exe().unwrap())
                        .args(["subprocess_stdio_helper", "--nocapture"])
                        .env("LIMO_CAD_STDIO_PIPE_HELPER", mode)
                        .stdin(Stdio::piped())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped())
                        .spawn()
                        .unwrap(),
                );
                let mut output = desktop.0.stdout.take().unwrap();
                let diagnostics = desktop.0.stderr.take().unwrap();
                let (output_tx, output_rx) = mpsc::channel();
                let output_reader = thread::spawn(move || {
                    let mut bytes = Vec::new();
                    let result = output.read_to_end(&mut bytes).map(|_| bytes);
                    let _ = output_tx.send(result);
                });
                let (diagnostic_tx, diagnostic_rx) = mpsc::channel();
                let diagnostic_reader = thread::spawn(move || {
                    for line in BufReader::new(diagnostics).lines() {
                        if diagnostic_tx.send(line).is_err() {
                            break;
                        }
                    }
                });
                assert_eq!(
                    diagnostic_rx
                        .recv_timeout(Duration::from_secs(5))
                        .unwrap()
                        .unwrap(),
                    "helper-ready"
                );
                writeln!(desktop.0.stdin.as_mut().unwrap(), "retire").unwrap();
                desktop.0.stdin.as_mut().unwrap().flush().unwrap();
                assert_eq!(
                    diagnostic_rx
                        .recv_timeout(Duration::from_secs(5))
                        .unwrap()
                        .unwrap(),
                    "retired-with-helper-alive"
                );
                let bytes = output_rx
                    .recv_timeout(Duration::from_secs(3))
                    .expect("A GUI helper retained the MCP pipe after desktop output retirement")
                    .unwrap();
                assert!(String::from_utf8_lossy(&bytes).contains("mcp-response"));
                assert!(
                    desktop.0.try_wait().unwrap().is_none(),
                    "EOF must not require desktop exit"
                );
                writeln!(desktop.0.stdin.as_mut().unwrap(), "release").unwrap();
                drop(desktop.0.stdin.take());
                let deadline = Instant::now() + Duration::from_secs(5);
                let status = loop {
                    if let Some(status) = desktop.0.try_wait().unwrap() {
                        break status;
                    }
                    assert!(Instant::now() < deadline, "Owned helper did not finish");
                    thread::sleep(Duration::from_millis(10));
                };
                assert!(status.success());
                output_reader.join().unwrap();
                diagnostic_reader.join().unwrap();
            }
        }
    }
}

#[cfg(not(any(windows, unix)))]
mod platform {
    pub(crate) fn retire_stdout_pipe() -> std::io::Result<()> {
        Ok(())
    }
}
