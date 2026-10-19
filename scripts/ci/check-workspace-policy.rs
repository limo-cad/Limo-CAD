//! Offline boundary check; this is not a credential scanner or access control.
use std::{fs, path::Path, process::Command};

fn private_path(path: &str) -> bool {
    if path == "AGENTS.md" || path == ".vscode/extensions.json" {
        return false;
    }
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    let parts: Vec<_> = normalized.split('/').collect();
    let name = parts.last().copied().unwrap_or("");
    parts.iter().any(|part| {
        matches!(
            *part,
            ".codex" | ".cursor" | ".claude" | ".agents" | ".t3" | ".idea" | ".vscode"
        )
    }) || matches!(
        name,
        "agents.md"
            | "agents.override.md"
            | "agents.local.md"
            | "claude.md"
            | "claude.local.md"
            | "codex.md"
            | ".mcp.json"
            | ".mcp.local.json"
            | "credentials.json"
            | "secrets.json"
            | "secrets.yaml"
            | "secrets.yml"
            | "secrets.toml"
            | "id_rsa"
            | "id_ed25519"
    ) || (name.starts_with(".env") && name != ".env.example")
        || [".code-workspace", ".pem", ".p12", ".pfx", ".p8", ".local"]
            .iter()
            .any(|suffix| name.ends_with(suffix))
}

fn shared_guidance(path: &str) -> bool {
    matches!(path, "AGENTS.md" | "CONTRIBUTING.md" | ".gitignore")
        || path.starts_with("docs/contributing/")
        || path.starts_with("docs/agentic/")
        || path.starts_with(".github/")
        || path.starts_with(".cargo/")
}

fn private_context(line: &str) -> bool {
    let normalized = line.replace('\\', "/").to_ascii_lowercase();
    if [
        "<!-- agent-board startup begin -->",
        "codex_thread_id",
        "\"mailbox\":\"",
        "\"mailbox\": \"",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
    {
        return true;
    }
    ["/users/", "/home/"].iter().any(|prefix| {
        normalized.match_indices(prefix).any(|(index, _)| {
            let user = normalized[index + prefix.len()..]
                .split(['/', ' ', '`', '"', '\''])
                .next()
                .unwrap_or("");
            !user.is_empty()
                && !user.starts_with(['<', '$', '{'])
                && !matches!(user, "user" | "username" | "your-user" | "runner")
        })
    })
}

fn check(root: &Path) -> Result<(), String> {
    let tracked = Command::new("git")
        .current_dir(root)
        .args(["ls-files", "--cached", "--full-name", "-z"])
        .output()
        .map_err(|_| "cannot inventory tracked files".to_owned())?;
    if !tracked.status.success() {
        return Err("git tracked-file inventory failed".into());
    }
    let paths = std::str::from_utf8(&tracked.stdout)
        .map_err(|_| "tracked paths must be UTF-8".to_owned())?;
    let mut failures = Vec::new();
    for path in paths.split('\0').filter(|path| !path.is_empty()) {
        if private_path(path) {
            failures.push(format!("{path}: private configuration must stay local"));
        } else if shared_guidance(path) {
            let absolute = root.join(path);
            let metadata = fs::symlink_metadata(&absolute)
                .map_err(|_| format!("{path}: cannot inspect shared guidance"))?;
            if metadata.file_type().is_symlink() {
                failures.push(format!("{path}: shared guidance must be a regular file"));
                continue;
            }
            let source = fs::read_to_string(absolute)
                .map_err(|_| format!("{path}: shared guidance must be readable UTF-8"))?;
            for (index, line) in source.lines().enumerate() {
                if private_context(line) {
                    // Report a location, never the potentially private contents.
                    failures.push(format!("{path}:{}: private host context", index + 1));
                }
            }
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n"))
    }
}

fn main() {
    if let Err(error) = check(Path::new(".")) {
        eprintln!("{error}");
        std::process::exit(1);
    }
    println!("Tracked workspace privacy boundary passed.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forced_adds_of_private_configuration_are_rejected() {
        for path in [
            ".codex/config.toml",
            "nested/.cursor/rules/private.mdc",
            ".agents/receipts.json",
            ".t3/session.json",
            ".vscode/settings.json",
            "project.code-workspace",
            "AGENTS.override.md",
            "agents.md",
            "nested/AGENTS.md",
            "CLAUDE.local.md",
            ".env.production",
            "keys/signing.p8",
            "nested\\.codex\\config.toml",
        ] {
            assert!(private_path(path), "accepted {path}");
        }
    }

    #[test]
    fn portable_project_configuration_stays_shareable() {
        for path in [
            "AGENTS.md",
            "CONTRIBUTING.md",
            "docs/contributing/workspace-and-ci.md",
            ".cargo/config.toml",
            ".github/workflows/rust-web.yml",
            ".vscode/extensions.json",
            ".env.example",
            "mcp-server/src/session.rs",
        ] {
            assert!(!private_path(path), "rejected {path}");
        }
    }

    #[test]
    fn host_context_is_rejected_without_printing_it() {
        for line in [
            "C:/Users/private-person/AppData/Local/tool.exe",
            r"C:\Users\private-person\tool.exe",
            "/Users/private-person/project",
            "/home/private-person/project",
            "<!-- agent-board startup begin -->",
            "CODEX_THREAD_ID=session",
            r#"{"mailbox": "internal-id"}"#,
        ] {
            assert!(private_context(line));
        }
    }

    #[test]
    fn portable_examples_and_client_documentation_are_allowed() {
        for line in [
            "%LOCALAPPDATA%/limo-cad/bevy/Limo-CAD.exe",
            "C:/Users/<user>/tool.exe",
            "/home/<user>/project",
            "$CODEX_HOME/config.toml",
            "~/.codex/config.toml",
            "${{ runner.temp }}/diagnostics",
            "Run cargo test for the affected crate.",
        ] {
            assert!(!private_context(line));
        }
    }

    #[test]
    fn tracked_inventory_catches_forced_adds_without_disclosing_contents() {
        let temporary = std::env::temp_dir();
        let root = temporary.join(format!(
            "limo-workspace-policy-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let git = |arguments: &[&str]| {
            let result = Command::new("git")
                .current_dir(&root)
                .args(arguments)
                .output()
                .unwrap();
            assert!(result.status.success());
        };
        git(&["init", "--quiet"]);
        fs::write(root.join("AGENTS.md"), "Read CONTRIBUTING.md.\n").unwrap();
        fs::write(root.join(".gitignore"), ".codex/\n").unwrap();
        git(&["add", "AGENTS.md", ".gitignore"]);
        assert!(check(&root).is_ok());
        fs::create_dir(root.join(".codex")).unwrap();
        fs::write(root.join(".codex/config.toml"), "private fixture content").unwrap();
        git(&["add", "--force", ".codex/config.toml"]);
        let failure = check(&root).unwrap_err();
        assert!(failure.contains(".codex/config.toml"));
        assert!(!failure.contains("private fixture content"));
        // This fixture was created by this test; validate its exact boundary.
        assert!(root
            .canonicalize()
            .unwrap()
            .starts_with(temporary.canonicalize().unwrap()));
        fs::remove_dir_all(root).unwrap();
    }
}
