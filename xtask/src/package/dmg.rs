use super::common;
use anyhow::{bail, ensure, Context, Result};
use std::{fs, io, path::Path, process::Command, time::Duration};

pub(crate) fn detach(mount: &Path) -> Result<()> {
    ensure!(cfg!(target_os = "macos"), "DMG detach requires macOS");
    ensure!(mount.is_absolute(), "DMG mountpoint must be absolute");
    ensure!(
        fs::symlink_metadata(mount)?.is_dir(),
        "DMG mountpoint must be an ordinary directory"
    );
    // Resolve system aliases such as /var before passing a mountpoint to hdiutil.
    let mount = mount.canonicalize()?;
    detach_with_busy_retry(
        &mount,
        |mount| {
            let output = Command::new("hdiutil")
                .env("LC_ALL", "C")
                .arg("detach")
                .arg(mount)
                .output()
                .context("run hdiutil detach")?;
            print!("{}", String::from_utf8_lossy(&output.stdout));
            eprint!("{}", String::from_utf8_lossy(&output.stderr));
            Ok(output.status.code())
        },
        std::thread::sleep,
    )
}

/// The caller owns this mount and has finished its readers. Retry Darwin EBUSY
/// (16) only; never force-unmount a volume or hide a persistent cleanup failure.
fn detach_with_busy_retry(
    mount: &Path,
    mut detach: impl FnMut(&Path) -> Result<Option<i32>>,
    mut wait: impl FnMut(Duration),
) -> Result<()> {
    for attempt in 1..=4 {
        let code = detach(mount)?;
        if code == Some(0) {
            return Ok(());
        }
        if code != Some(16) || attempt == 4 {
            bail!(
                "hdiutil detach failed for {} (exit {code:?}, attempt {attempt})",
                mount.display()
            );
        }
        eprintln!("DMG is busy; retrying normal detach after attempt {attempt}");
        wait(Duration::from_secs(1 << (attempt - 1)));
    }
    unreachable!()
}

pub(super) struct CreateResult {
    pub code: Option<i32>,
    pub stderr: Vec<u8>,
}

impl CreateResult {
    fn resource_busy(&self) -> bool {
        self.code == Some(1)
            && self.stderr.split(|byte| *byte == b'\n').any(|line| {
                line.strip_suffix(b"\r").unwrap_or(line)
                    == b"hdiutil: create failed - Resource busy"
            })
    }
}

fn check_destination(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => common::ordinary_file(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// Retry only hdiutil's observed EBUSY failure, without exposing a partial image
/// at the final artifact path or rebuilding/mutating the signed source stage.
pub(super) fn create_with_busy_retry(
    destination: &Path,
    source: &Path,
    mut create: impl FnMut(&Path, &Path) -> Result<CreateResult>,
    mut wait: impl FnMut(Duration),
) -> Result<()> {
    let parent = destination.parent().context("DMG output directory")?;
    common::ordinary_directory(parent)?;
    let output = parent.canonicalize()?;
    let destination = output.join(destination.file_name().context("DMG file name")?);
    check_destination(&destination)?;
    for attempt in 1..=3 {
        let temporary = tempfile::Builder::new()
            .prefix(".limo-cad-dmg-")
            .tempdir_in(&output)?;
        common::ordinary_directory(temporary.path())?;
        ensure!(
            temporary.path().canonicalize()?.parent() == Some(output.as_path()),
            "DMG candidate escaped its output directory"
        );
        let candidate = temporary.path().join(destination.file_name().unwrap());
        let result = create(source, &candidate)?;
        if result.code == Some(0) {
            common::ordinary_file(&candidate)?;
            ensure!(
                fs::metadata(&candidate)?.len() > 0,
                "hdiutil created an empty DMG"
            );
            check_destination(&destination)?;
            fs::rename(&candidate, &destination).context("publish completed DMG")?;
            temporary.close()?;
            return Ok(());
        }
        temporary.close()?;
        if !result.resource_busy() || attempt == 3 {
            bail!(
                "hdiutil create failed (exit {:?}, attempt {attempt})",
                result.code
            );
        }
        eprintln!("hdiutil create reported Resource busy; retrying owned DMG candidate");
        wait(Duration::from_millis(100 * attempt));
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detach_retries_only_busy_and_propagates_exhaustion() {
        let cases: &[(&[Option<i32>], bool)] = &[
            (&[Some(0)], true),
            (&[Some(16), Some(0)], true),
            (&[Some(16), Some(16), Some(16), Some(0)], true),
            (&[Some(16), Some(16), Some(16), Some(16)], false),
            (&[Some(1)], false),
            (&[Some(16), Some(13)], false),
            (&[None], false),
        ];
        let mount = Path::new("/owned/mountpoint with spaces");
        for &(codes, succeeds) in cases {
            let mut calls = 0;
            let mut waits = Vec::new();
            let result = detach_with_busy_retry(
                mount,
                |path| {
                    assert_eq!(path, mount);
                    let code = codes[calls];
                    calls += 1;
                    Ok(code)
                },
                |delay| waits.push(delay),
            );
            assert_eq!(result.is_ok(), succeeds);
            assert_eq!(calls, codes.len());
            let expected: Vec<_> = (0..calls - 1)
                .map(|index| Duration::from_secs(1 << index))
                .collect();
            assert_eq!(waits, expected);
        }
        let mut calls = 0;
        let error = detach_with_busy_retry(
            mount,
            |_| {
                calls += 1;
                Err(io::Error::new(io::ErrorKind::NotFound, "hdiutil missing").into())
            },
            |_| panic!("a failed command must not be retried"),
        )
        .unwrap_err();
        assert_eq!(calls, 1);
        assert!(error.to_string().contains("hdiutil missing"));
    }

    type Outcome<'a> = (Option<i32>, &'a [u8]);
    type Case<'a> = (&'a [Outcome<'a>], usize, bool);

    #[test]
    fn exact_busy_retry_preserves_staging_and_final_artifact() {
        let busy = b"hdiutil: create failed - Resource busy\n";
        let cases: &[Case<'_>] = &[
            (&[(Some(1), busy), (Some(0), b"")], 2, true),
            (
                &[(Some(1), b"hdiutil: create failed - Permission denied\n")],
                1,
                false,
            ),
            (&[(Some(2), busy)], 1, false),
            (
                &[(
                    Some(1),
                    b"hdiutil: create failed - Resource busy elsewhere\n",
                )],
                1,
                false,
            ),
            (&[(None, busy)], 1, false),
            (
                &[(Some(1), busy), (Some(1), busy), (Some(1), busy)],
                3,
                false,
            ),
            (&[(Some(0), b"")], 1, true),
        ];
        for (outcomes, expected_attempts, succeeds) in cases {
            let root = tempfile::tempdir().unwrap();
            let root_path = root.path().canonicalize().unwrap();
            let output = root_path.join("output");
            let stage = root_path.join("signed-stage");
            fs::create_dir(&output).unwrap();
            fs::create_dir(&stage).unwrap();
            let signed_app = stage.join("signed-app");
            fs::write(&signed_app, b"immutable signed application").unwrap();
            let destination = output.join("Limo.CAD_test.dmg");
            fs::write(&destination, b"previous valid DMG").unwrap();
            let unrelated = output.join("keep.txt");
            fs::write(&unrelated, b"unrelated output").unwrap();
            let mut attempts = 0;
            let mut candidates = Vec::new();
            let mut waits = Vec::new();
            let result = create_with_busy_retry(
                &destination,
                &stage,
                |source, candidate| {
                    assert_eq!(source, stage);
                    assert_eq!(
                        fs::read(&signed_app).unwrap(),
                        b"immutable signed application"
                    );
                    assert_eq!(fs::read(&destination).unwrap(), b"previous valid DMG");
                    assert!(candidate.starts_with(&output));
                    assert!(!candidate.exists());
                    candidates.push(candidate.to_owned());
                    let (code, stderr) = outcomes[attempts];
                    attempts += 1;
                    fs::write(
                        candidate,
                        if code == Some(0) {
                            b"complete new DMG".as_slice()
                        } else {
                            b"partial DMG".as_slice()
                        },
                    )?;
                    Ok(CreateResult {
                        code,
                        stderr: stderr.to_vec(),
                    })
                },
                |delay| waits.push(delay),
            );
            assert_eq!(result.is_ok(), *succeeds);
            assert_eq!(attempts, *expected_attempts);
            let expected_waits: Vec<_> = (1..*expected_attempts)
                .map(|attempt| Duration::from_millis(100 * attempt as u64))
                .collect();
            assert_eq!(waits, expected_waits);
            assert_eq!(
                fs::read(&destination).unwrap(),
                if *succeeds {
                    b"complete new DMG".as_slice()
                } else {
                    b"previous valid DMG".as_slice()
                }
            );
            assert_eq!(
                fs::read(&signed_app).unwrap(),
                b"immutable signed application"
            );
            assert_eq!(fs::read(&unrelated).unwrap(), b"unrelated output");
            assert!(candidates.iter().all(|candidate| !candidate.exists()));
            assert_eq!(fs::read_dir(&output).unwrap().count(), 2);
        }
    }
}
