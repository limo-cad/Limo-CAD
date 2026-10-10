//! Std-only subprocess fixture, compiled by replay_cli.rs using the same Rust
//! toolchain as xtask. It never opens CAD, accesses sessions, or invokes a shell.
use std::{
    fs,
    io::{BufRead, Write},
    thread,
    time::Duration,
};

fn main() {

    thread::spawn(|| {
        thread::sleep(Duration::from_secs(15));
        std::process::exit(98);
    });
    if let Some(path) = std::env::var_os("LIMO_CAD_FIXTURE_ARGUMENTS") {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        writeln!(
            file,
            "{}",
            std::env::args().skip(1).collect::<Vec<_>>().join("\0")
        )
        .unwrap();
    }
    if let Some(path) = std::env::var_os("LIMO_CAD_FIXTURE_HEARTBEAT") {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)
            .unwrap();
        write!(file, "{}:", std::process::id()).unwrap();
        file.flush().unwrap();
        thread::spawn(move || loop {
            file.write_all(b".").unwrap();
            file.flush().unwrap();
            thread::sleep(Duration::from_millis(20));
        });
    }
    let mode = std::env::var("LIMO_CAD_FIXTURE_MODE").unwrap_or_default();
    if mode == "silent" || mode == "sentinel" {
        loop {
            thread::park();
        }
    }
    if mode == "non-mcp" {
        println!("This executable does not speak MCP.");
        std::io::stdout().flush().unwrap();
        loop {
            thread::park();
        }
    }
    let mut loaded = false;
    for line in std::io::stdin().lock().lines() {
        let line = line.unwrap();
        if let Some(path) = std::env::var_os("LIMO_CAD_FIXTURE_REQUESTS") {
            let mut file = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .unwrap();
            writeln!(file, "{line}").unwrap();
        }
        let Some(id) = line.split("\"id\":").nth(1) else {
            continue;
        };
        let id: String = id
            .trim_start()
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        assert!(!id.is_empty());
        let result = if line.contains("\"method\":\"initialize\"") {
            if mode == "invalid-init" {
                "{\"not_an_mcp_handshake\":true}".to_owned()
            } else {
                r#"{"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}}"#.to_owned()
            }
        } else {
            let exported_model;
            let payload = if line.contains("\"name\":\"cad_project_model\"") {
                let model = if mode == "invalid-model" {
                    "{}"
                } else if mode == "changed-model" && loaded {
                    r#"{"format":"limo-cad-project","schema_version":7,"document":{"name":"changed"},"sketches":[1],"drawings":{"sheets":[1]},"assembly":{"joints":[1]}}"#
                } else {
                    r#"{"format":"limo-cad-project","schema_version":7,"document":{"name":"headless fixture"},"sketches":[1],"drawings":{"sheets":[1]},"assembly":{"joints":[1]}}"#
                };
                exported_model = format!("{model:?}");
                &exported_model
            } else if line.contains("\"name\":\"cad_load_project_model\"") {
                loaded = true;
                r#"{"loaded":true}"#
            } else if line.contains("\"name\":\"solid_scene\"") {
                if mode == "broken-geometry" && loaded {
                    r#"{"bodies":[{"id":1}],"errors":["recompute failed"]}"#
                } else if mode == "missing-body" && loaded {
                    r#"{"bodies":[],"errors":[]}"#
                } else {
                    r#"{"bodies":[{"id":1}],"errors":[]}"#
                }
            } else if line.contains("\"action\":\"recipes\"") {
                if mode == "unknown-recipe" {
                    "[]"
                } else {
                    r#"[{"id":"fixture"}]"#
                }
            } else if line.contains("\"action\":\"script\"") {
                if mode == "slow-modeling" {
                    thread::sleep(Duration::from_millis(1500));
                }
                r#"{"steps_completed":1,"checks_completed":1,"elapsed_ms":1,"exports":{"final_model":{"solid":1}}}"#
            } else {
                r#"{"ok":true}"#
            };


            let is_error = mode == "failed-reload"
                && line.contains("\"name\":\"cad_load_project_model\"")
                || mode == "failed-script" && line.contains("\"action\":\"script\"");
            format!("{{\"isError\":{is_error},\"content\":[{{\"type\":\"text\",\"text\":{payload:?}}}]}}")
        };
        println!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":{result}}}");
        std::io::stdout().flush().unwrap();
    }


    if mode == "unknown-recipe" {
        loop {
            thread::park();
        }
    }
}
