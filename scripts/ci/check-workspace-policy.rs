//! Offline boundary check; this is not a credential scanner or access control.
use std::{
    fs,
    io::{BufRead, BufReader},
    path::Path,
    process::Command,
};

fn private_path(path: &str) -> bool {
    if path == ".vscode/extensions.json" {
        return false;
    }
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    let parts: Vec<_> = normalized.split('/').collect();
    let name = parts.last().copied().unwrap_or("");
    parts.iter().any(|part| {
        matches!(
            *part,
            ".codex"
                | ".cursor"
                | ".claude"
                | ".agents"
                | ".t3"
                | ".gemini"
                | ".opencode"
                | ".copilot"
                | ".continue"
                | ".windsurf"
                | ".cline"
                | ".clinerules"
                | ".roo"
                | ".grok"
                | ".junie"
                | ".kiro"
                | ".amazonq"
                | ".factory"
                | ".idea"
                | ".vscode"
        )
    }) || parts.iter().any(|part| part.starts_with(".aider"))
        || matches!(parts.first(), Some(&"agents" | &"skills"))
        || parts.windows(2).any(|pair| {
            pair[0] == ".github"
                && matches!(pair[1], "agents" | "instructions" | "prompts" | "skills")
        })
        || ["agents", "claude", "codex", "gemini"]
            .iter()
            .any(|prefix| {
                name.strip_prefix(*prefix).is_some_and(|tail| {
                    tail == ".md" || (tail.starts_with('.') && tail.ends_with(".md"))
                })
            })
        || [".agent.md", ".instructions.md", ".prompt.md"]
            .iter()
            .any(|suffix| name.ends_with(suffix))
        || name.starts_with(".mcp.")
        || name.starts_with(".claude.")
        || matches!(
            name,
            "skill.md"
                | "copilot-instructions.md"
                | "claude_desktop_config.json"
                | ".cursorrules"
                | ".cursorignore"
                | ".cursorindexingignore"
                | ".windsurfrules"
                | ".clinerules"
                | ".roomodes"
                | ".rooignore"
                | "opencode.json"
                | "opencode.jsonc"
                | "mcp.json"
                | "mcp.jsonc"
                | "credentials.json"
                | "secrets.json"
                | "secrets.yaml"
                | "secrets.yml"
                | "secrets.toml"
                | "id_rsa"
                | "id_rsa.pub"
                | "id_ed25519"
                | "id_ed25519.pub"
        )
        || (name.starts_with(".env") && name != ".env.example")
        || [".code-workspace", ".pem", ".p12", ".pfx", ".p8", ".local"]
            .iter()
            .any(|suffix| name.ends_with(suffix))
}

fn shared_guidance(path: &str) -> bool {
    matches!(path, "CONTRIBUTING.md" | ".gitignore" | ".dockerignore")
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
    private_host_context(line)
}

fn private_host_context(line: &str) -> bool {
    let normalized = line.replace('\\', "/").to_ascii_lowercase();
    let home = ["/users/", "/home/"].iter().any(|prefix| {
        normalized.match_indices(prefix).any(|(index, _)| {
            // A URL's /users endpoint is not a local home directory.
            if normalized[..index]
                .chars()
                .next_back()
                .is_some_and(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_'))
            {
                return false;
            }
            let user = normalized[index + prefix.len()..]
                .trim_start_matches('/')
                .split(['/', ' ', '`', '"', '\''])
                .next()
                .unwrap_or("");
            !user.is_empty()
                && !user.starts_with(['<', '$', '{'])
                && !matches!(
                    user,
                    "user" | "username" | "your-user" | "runner" | "runneradmin"
                )
        })
    });
    // Account names in identity fields and task-list logs need redaction too.
    // Arbitrary DOMAIN\\token matching misreads NC programs, escaped newlines,
    // SDK paths and PowerShell variables as accounts.
    let words = normalized
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
        .collect::<Vec<_>>();
    let identity = words
        .iter()
        .any(|field| matches!(*field, "username" | "user_name" | "account"))
        || words.windows(2).any(|pair| pair == ["user", "name"]);
    home || identity
        && line
            .split(|ch: char| {
                !ch.is_ascii_alphanumeric() && !matches!(ch, '\\' | '-' | '_' | '.' | '$')
            })
            .any(|token| {
                let Some((domain, account)) = token.split_once('\\') else {
                    return false;
                };
                let account = account.trim_start_matches('\\');
                domain.len() > 1
                    && domain.chars().all(|ch| {
                        ch.is_ascii_uppercase() || ch.is_ascii_digit() || matches!(ch, '-' | '_')
                    })
                    && !matches!(
                        domain,
                        "DOMAIN"
                            | "HOST"
                            | "COMPUTERNAME"
                            | "USERPROFILE"
                            | "SYSTEMROOT"
                            | "BUILD_ROOT"
                            | "WORKSPACE"
                    )
                    && !account.is_empty()
                    && !account.contains('\\')
                    && !matches!(
                        account.to_ascii_lowercase().as_str(),
                        "user"
                            | "username"
                            | "your-user"
                            | "runner"
                            | "runneradmin"
                            | "system"
                            | "administrators"
                    )
            })
}

fn absolute_drive_path(line: &str) -> bool {
    line.as_bytes().windows(3).enumerate().any(|(index, part)| {
        part[0].is_ascii_alphabetic()
            && part[1] == b':'
            && matches!(part[2], b'/' | b'\\')
            && (index == 0
                || !(line.as_bytes()[index - 1].is_ascii_alphanumeric()
                    || line.as_bytes()[index - 1] == b'_'))
    })
}

// Extract JSON string values without decoding raw CSV domain separators.
// Stop at each closing quote so a final row needs no trailing newline.
fn log_text(line: &str) -> Vec<String> {
    if !line.contains("\\\"") || csv_record(line).is_some() {
        return Vec::new();
    }
    let mut values = Vec::new();
    let mut chars = line.chars();
    while let Some(ch) = chars.next() {
        if ch != '"' {
            continue;
        }
        let mut value = String::new();
        while let Some(ch) = chars.next() {
            match ch {
                '"' => break,
                '\\' => match chars.next() {
                    Some('n') => value.push('\n'),
                    Some('r') => value.push('\r'),
                    Some('t') => value.push('\t'),
                    Some('"') => value.push('"'),
                    Some('\\') => value.push('\\'),
                    Some(other) => {
                        value.push('\\');
                        value.push(other);
                    }
                    None => value.push('\\'),
                },
                other => value.push(other),
            }
        }
        values.push(value);
    }
    values
}

fn csv_record(line: &str) -> Option<Vec<String>> {
    let mut chars = line.trim().chars().peekable();
    let mut fields = Vec::new();
    loop {
        if chars.next()? != '"' {
            return None;
        }
        let mut field = String::new();
        loop {
            match chars.next()? {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => break,
                ch => field.push(ch),
            }
        }
        fields.push(field);
        match chars.next() {
            None => return Some(fields),
            Some(',') => {}
            Some(_) => return None,
        }
    }
}

fn private_tasklist_account(line: &str, account_column: &mut Option<usize>) -> bool {
    let values = log_text(line);
    for row in std::iter::once(line)
        .chain(values.iter().map(String::as_str))
        .flat_map(str::lines)
    {
        let header = row.find("\"Image Name\",").map(|index| &row[index..]);
        let Some(fields) = csv_record(header.unwrap_or(row)) else {
            continue;
        };
        if fields.first().is_some_and(|field| field == "Image Name")
            && fields.iter().any(|field| field == "PID")
        {
            *account_column = fields.iter().position(|field| field == "User Name");
            continue;
        }
        if let Some(account) = account_column.and_then(|column| fields.get(column)) {
            let account = account.trim().to_ascii_lowercase();
            if !account.starts_with(['<', '$'])
                && !matches!(
                    account.as_str(),
                    "" | "n/a"
                        | "user"
                        | "username"
                        | "runner"
                        | "runneradmin"
                        | "system"
                        | "nt authority\\system"
                        | "nt authority\\local service"
                        | "nt authority\\network service"
                )
            {
                return true;
            }
        }
    }
    false
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
        } else {
            let guidance = shared_guidance(path);
            let absolute = root.join(path);
            let metadata = fs::symlink_metadata(&absolute)
                .map_err(|_| format!("{path}: cannot inspect shared guidance"))?;
            if metadata.file_type().is_symlink() {
                if guidance {
                    failures.push(format!("{path}: shared guidance must be a regular file"));
                }
                continue;
            }
            if !metadata.is_file() {
                continue;
            }
            let mut reader = BufReader::new(
                fs::File::open(absolute)
                    .map_err(|_| format!("{path}: cannot read tracked file"))?,
            );
            if !guidance
                && reader
                    .fill_buf()
                    .map_err(|_| format!("{path}: cannot inspect text"))?
                    .contains(&0)
            {
                continue;
            }
            let mut line = String::new();
            let mut number = 0;
            let mut account_column = None;
            let evidence = path.starts_with("docs/qualification/")
                || path.starts_with("docs/startup-measurements/");
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => number += 1,
                    Err(_) if !guidance => break,
                    Err(_) => {
                        return Err(format!("{path}: shared guidance must be readable UTF-8"))
                    }
                }
                if private_host_context(&line)
                    || private_tasklist_account(&line, &mut account_column)
                    || (evidence && absolute_drive_path(&line))
                    || (guidance && private_context(&line))
                {
                    // Report a location, never the potentially private contents.
                    failures.push(format!("{path}:{number}: private host context"));
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
            "AGENTS.md",
            "agents.md",
            "nested/AGENTS.md",
            "CLAUDE.local.md",
            "nested/GeMiNi.Md",
            "skills/example/SKILL.md",
            "agents/reviewer/config.yaml",
            "nested/.github/agents/reviewer.agent.md",
            ".github/instructions/rust.instructions.md",
            ".github/prompts/review.prompt.md",
            ".github/skills/review/scripts/check.py",
            "nested/.continue/config.yaml",
            ".gemini/settings.json",
            ".roo/rules/local.md",
            ".grok/settings.json",
            ".junie/guidelines.md",
            ".kiro/steering/project.md",
            ".amazonq/rules/project.md",
            ".factory/settings.json",
            ".claude.json",
            ".aider.conf.yml",
            "nested/.clinerules/local.md",
            "nested/.cursorrules",
            ".cursorindexingignore",
            "mcp.json",
            "nested/opencode.jsonc",
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
            ["C:/Users/", "private-person/AppData/Local/tool.exe"].concat(),
            [r"C:\Users\", r"private-person\tool.exe"].concat(),
            [r#"{"source":"C:\\Users\\"#, r#"private-person\\tool.exe"}"#].concat(),
            ["/Users/", "private-person/project"].concat(),
            ["/home/", "private-person/project"].concat(),
            format!(
                r#"{{"UserName":"{}\\{}"}}"#,
                "EXAMPLE-HOST", "private-person"
            ),
            format!(r#"User Name: {}\\{}"#, "EXAMPLE-HOST", "private-person"),
            "<!-- agent-board startup begin -->".into(),
            "CODEX_THREAD_ID=session".into(),
            r#"{"mailbox": "internal-id"}"#.into(),
        ] {
            assert!(private_context(&line));
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
            "https://api.example.com/users/example",
            r"DOMAIN\user",
            r#""G90\nG1 X10\n""#,
            r#""$PWD\vcpkg_installed""#,
            r"PATH_TO_OCCT_SDK\bin",
        ] {
            assert!(!private_context(line));
        }
    }

    #[test]
    fn evidence_paths_and_tasklist_account_columns_are_checked() {
        assert!(absolute_drive_path(r"D:\build-output\proof.json"));
        assert!(absolute_drive_path("C:/model.limo"));
        assert!(!absolute_drive_path("https://example.com/artifact.json"));
        assert!(!absolute_drive_path("<local-root>/proof.json"));
        let header = [r#""Image"#, r#" Name","PID","User Name""#].concat();
        let private = format!(r#""worker.exe","42","{}""#, "fixture-person");
        let mut column = None;
        assert!(!private_tasklist_account(&header, &mut column));
        assert!(private_tasklist_account(&private, &mut column));
        for name in ["nora", "ruth"] {
            let account = format!("{}\\{name}", "EXAMPLE-HOST");
            let row = format!(r#""worker.exe","42","{account}""#);
            assert!(private_tasklist_account(&row, &mut column));
        }
        for account in ["<local-account>", "N/A", "SYSTEM"] {
            let row = format!(r#""worker.exe","42","{account}""#);
            assert!(!private_tasklist_account(&row, &mut column));
        }
        let embedded = format!(
            "{{\"output\":\"{}\\r\\n{}\\r\\n\"}}",
            header.replace('"', "\\\""),
            private.replace('"', "\\\"")
        );
        assert!(private_tasklist_account(&embedded, &mut None));
        let final_row = embedded.replace("\\r\\n\"}", "\"}");
        assert!(private_tasklist_account(&final_row, &mut None));
        assert!(!private_tasklist_account(r#""G90\nG1 X10\n""#, &mut None));
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
        fs::write(
            root.join("CONTRIBUTING.md"),
            "Shared contribution policy.\n",
        )
        .unwrap();
        fs::write(root.join(".gitignore"), include_str!("../../.gitignore")).unwrap();
        git(&["add", "CONTRIBUTING.md", ".gitignore"]);
        assert!(check(&root).is_ok());
        let evidence = root.join("evidence.json");
        let private_home = ["/home/", "fixture-person/project"].concat();
        fs::write(&evidence, format!(r#"{{"source":"{private_home}"}}"#)).unwrap();
        git(&["add", "evidence.json"]);
        let failure = check(&root).unwrap_err();
        assert!(failure.contains("evidence.json:1"));
        assert!(!failure.contains(&private_home));
        fs::write(&evidence, r#"{"source":"/home/<user>/project"}"#).unwrap();
        assert!(check(&root).is_ok());
        let private = [
            "AGENTS.md",
            ".codex/config.toml",
            ".github/agents/reviewer.agent.md",
            "nested/.cursorrules",
            "nested/.gemini/settings.json",
            "nested/SKILL.md",
            ".grok/settings.json",
            ".junie/guidelines.md",
            ".kiro/steering/project.md",
            ".amazonq/rules/project.md",
            ".factory/settings.json",
            ".claude.json",
            "id_ed25519.pub",
        ];
        for path in private {
            let absolute = root.join(path);
            fs::create_dir_all(absolute.parent().unwrap()).unwrap();
            fs::write(absolute, "private fixture content").unwrap();
            git(&["check-ignore", "--quiet", "--no-index", path]);
            git(&["add", "--force", path]);
        }
        let failure = check(&root).unwrap_err();
        for path in private {
            assert!(failure.contains(path), "missed forced-add path {path}");
        }
        assert!(!failure.contains("private fixture content"));
        // This fixture was created by this test; validate its exact boundary.
        assert!(root
            .canonicalize()
            .unwrap()
            .starts_with(temporary.canonicalize().unwrap()));
        fs::remove_dir_all(root).unwrap();
    }
}
