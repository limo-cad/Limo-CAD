use super::*;

#[test]
fn ci_guards_retarget_exact_repository_literals_without_relaxing_conditions() {
    let from = "jackControls/noBS-CAD";
    let to = "new-owner/new-cad";
    for text in [
        "$env:GITHUB_REPOSITORY -ne 'jackControls/noBS-CAD' -or $env:RUNNER_OS -ne 'Windows'",
        "$env:GITHUB_REPOSITORY = 'jackControls/noBS-CAD'",
        "test \"${GITHUB_REPOSITORY:-}\" = jackControls/noBS-CAD",
        "${GITHUB_REPOSITORY:-} == jackControls/noBS-CAD && exit 1",
        "env[\"GITHUB_REPOSITORY\"] == \"jackControls/noBS-CAD\" && allowed",
        "Environment.GetEnvironmentVariable(\"GITHUB_REPOSITORY\") != \"jackControls/noBS-CAD\"",
    ] {
        assert_eq!(
            retarget(text, from, to, None).unwrap(),
            text.replace(from, to)
        );
    }
    for text in [
        "GITHUB_REPOSITORY used to be jackControls/noBS-CAD",
        "env[\"OTHER\"] == \"jackControls/noBS-CAD\"",
        "env[\"GITHUB_REPOSITORY\"] == \"jackControls/noBS-CAD-fork\"",
        "env[\"GITHUB_REPOSITORY\"] == \"another/repo\" && other == \"jackControls/noBS-CAD\"",
    ] {
        assert_eq!(retarget(text, from, to, None).unwrap(), text);
    }
}

#[test]
fn hosted_desktop_guards_and_the_arm_fixtures_use_the_stable_repository_id() {
    let guard = Regex::new(
        r#"\bGITHUB_REPOSITORY_ID\b[^A-Za-z0-9_\r\n]*?(?:==|!=|-eq|-ne|=)\s*["']?([0-9]+)($|["'\s;,)])"#,
    )
    .unwrap();
    for file in [
        "scripts/prepare-hosted-arm-desktop.ps1",
        "scripts/ci/arm-runner-preflight.test.ps1",
        "scripts/ci/arm-runner-shell-preflight.test.ps1",
        "xtask/platform/native-print-cancel-windows.ps1",
        "xtask/platform/native-windows-ime-session.ps1",
        "xtask/platform/native-input-macos.swift",
        "xtask/platform/macos-ime-probe.swift",
        "xtask/platform/run-macos-ime-probe.sh",
        "xtask/platform/windows-ime-probe.cs",
        "xtask/platform/windows-ime-probe.ps1",
    ] {
        let source = fs::read_to_string(crate::build_tools::root().join(file)).unwrap();
        let guards = guard
            .captures_iter(&source)
            .map(|capture| capture[1].to_owned())
            .collect::<Vec<_>>();
        assert_eq!(guards, [id()], "{file}");
    }
}

#[test]
fn links_preserve_artifacts_and_distinct_repositories() {
    let from = "jackControls/noBS-CAD";
    let to = "limo-cad/limo-cad";
    for prefix in [
        "https://github.com/",
        "git+https://github.com/",
        "https://raw.githubusercontent.com/",
        "https://api.github.com/repos/",
        "git@github.com:",
        "https://img.shields.io/github/actions/workflow/status/",
        "https://img.shields.io/github/v/release/",
        "`",
    ] {
        for tail in ["", "/main/a.rs", ".git", ".git\"", "?label=release", "`"] {
            assert_eq!(
                retarget(&format!("{prefix}{from}{tail}"), from, to, None).unwrap(),
                format!("{prefix}{to}{tail}")
            );
        }
        for tail in ["-fork", ".gitx", "_more"] {
            let text = format!("{prefix}{from}{tail}");
            assert_eq!(retarget(&text, from, to, None).unwrap(), text);
        }
    }
    let artifact = format!("https://github.com/{from}/releases/download/v1/noBS-CAD-1-windows.zip");
    assert_eq!(
        retarget(&artifact, from, to, None).unwrap(),
        artifact.replace(from, to)
    );
    assert_eq!(
        retarget(
            "https://JACKCONTROLS.github.io/noBS-CAD/open.html",
            from,
            to,
            Some("limo.example/cad")
        )
        .unwrap(),
        "https://limo.example/cad/open.html"
    );
}

#[test]
fn migration_skips_history_binaries_lockfiles_and_its_fixtures() {
    for file in [
        "docs/release-notes/v1.md",
        "Cargo.lock",
        "a/Cargo.lock",
        "a.png",
        "a.PDF",
        "xtask/src/repository/tests.rs",
    ] {
        assert!(skipped(file));
    }
    for file in ["REPOSITORY", "README.md", "Cargo.toml"] {
        assert!(!skipped(file));
    }
    for slug in ["o/r", "owner/repo.name"] {
        assert!(valid_slug(slug));
    }
    for slug in ["../repo", "o/../r", "o/", "/r", "o/re po"] {
        assert!(!valid_slug(slug));
    }
}
