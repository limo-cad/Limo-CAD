//! Fail-closed opt-in for OS input on this repository's disposable runners.

pub(super) fn enabled(
    platform: &str,
    expected_platform: &str,
    runner: &str,
    opt_in: (&str, &str),
    read: impl Fn(&str) -> Option<String>,
) -> bool {
    platform == expected_platform
        && [
            opt_in,
            ("GITHUB_ACTIONS", "true"),
            ("RUNNER_OS", runner),
            ("RUNNER_ENVIRONMENT", "github-hosted"),
            ("GITHUB_REPOSITORY_ID", crate::repository::id()),
        ]
        .into_iter()
        .all(|(key, expected)| read(key).as_deref() == Some(expected))
        && read("GITHUB_RUN_ID")
            .is_some_and(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn hosted_arm_preparation_only_closes_identity_verified_observed_windows() {
        use std::os::windows::process::CommandExt;

        let scripts = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/ci");
        for (script, marker) in [
            (
                "arm-runner-preflight.test.ps1",
                "8 managed preflight cases passed",
            ),
            (
                "arm-runner-shell-preflight.test.ps1",
                "PASS: hosted ARM shell preflight;",
            ),
        ] {
            let output = std::process::Command::new("powershell.exe")
                .creation_flags(0x08000000)
                .args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-File",
                ])
                .arg(scripts.join(script))
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{script}: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                String::from_utf8_lossy(&output.stdout).contains(marker),
                "{script} omitted {marker}: {}",
                String::from_utf8_lossy(&output.stdout)
            );
        }
    }

    #[test]
    fn renamed_repository_preserves_every_disposable_runner_guard() {
        for (platform, runner, opt_in) in [
            (
                "windows",
                "Windows",
                ("LIMO_CAD_NATIVE_IME_TEST", "windows-japanese"),
            ),
            (
                "macos",
                "macOS",
                ("LIMO_CAD_NATIVE_IME_TEST", "macos-japanese"),
            ),
            (
                "windows",
                "Windows",
                ("LIMO_CAD_NATIVE_PRINT_TEST", "windows-cancel"),
            ),
        ] {
            let environment = |key: &str| {
                Some(if key == opt_in.0 {
                    opt_in.1.into()
                } else {
                    match key {
                        "GITHUB_ACTIONS" => "true".into(),
                        "RUNNER_OS" => runner.into(),
                        "RUNNER_ENVIRONMENT" => "github-hosted".into(),
                        "GITHUB_REPOSITORY_ID" => crate::repository::id().into(),
                        "GITHUB_RUN_ID" => "123".into(),
                        _ => return None,
                    }
                })
            };
            assert!(enabled(platform, platform, runner, opt_in, environment));
            assert!(!enabled("linux", platform, runner, opt_in, environment));
            for key in [
                opt_in.0,
                "GITHUB_ACTIONS",
                "RUNNER_OS",
                "RUNNER_ENVIRONMENT",
                "GITHUB_REPOSITORY_ID",
                "GITHUB_RUN_ID",
            ] {
                assert!(
                    !enabled(platform, platform, runner, opt_in, |name| {
                        if name == key {
                            None
                        } else {
                            environment(name)
                        }
                    }),
                    "{platform}: missing {key}"
                );
            }
            for (key, bad) in [
                (opt_in.0, "another-mode"),
                ("GITHUB_ACTIONS", "false"),
                ("RUNNER_OS", "Linux"),
                ("RUNNER_ENVIRONMENT", "self-hosted"),
                ("GITHUB_REPOSITORY_ID", "1313334316"),
                ("GITHUB_REPOSITORY_ID", "01313334315"),
                ("GITHUB_RUN_ID", ""),
                ("GITHUB_RUN_ID", "local"),
                ("GITHUB_RUN_ID", "12 34"),
            ] {
                assert!(
                    !enabled(platform, platform, runner, opt_in, |name| {
                        if name == key {
                            Some(bad.into())
                        } else {
                            environment(name)
                        }
                    }),
                    "{platform}: invalid {key}={bad}"
                );
            }
        }
    }
}
