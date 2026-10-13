//! One stdio transport for rendered and headless CAD. The desktop owns its
//! lifetime: a disconnected client never exits or closes the application.
use std::io::{self, BufRead, Write};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Condvar, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use serde_json::Value;

use crate::{error_response, handle_message, idle_due_messages, CadServer, DesktopBinding};

#[path = "stdio_output.rs"]
mod output_pipe;

/// What the desktop title bar can say about the stdio agent.
///
/// Headless `run_stdio` does not change this. A desktop worker is `Waiting`
/// until `initialize` succeeds, then `Attached` until that worker returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum DesktopMcpPresence {
    Off = 0,
    Waiting = 1,
    Attached = 2,
}

static DESKTOP_MCP_PRESENCE: AtomicU8 = AtomicU8::new(0);

pub fn desktop_mcp_presence() -> DesktopMcpPresence {
    match DESKTOP_MCP_PRESENCE.load(Ordering::Relaxed) {
        1 => DesktopMcpPresence::Waiting,
        2 => DesktopMcpPresence::Attached,
        _ => DesktopMcpPresence::Off,
    }
}

fn set_desktop_mcp_presence(presence: DesktopMcpPresence) {
    DESKTOP_MCP_PRESENCE.store(presence as u8, Ordering::Relaxed);
}

struct ClearDesktopMcpPresence;

impl Drop for ClearDesktopMcpPresence {
    fn drop(&mut self) {
        set_desktop_mcp_presence(DesktopMcpPresence::Off);
    }
}

pub fn run_stdio() -> Result<(), String> {
    #[cfg(all(windows, feature = "native-computer-control"))]
    if let Some(result) = crate::computer_control::run_worker_if_requested() {
        return result;
    }
    run(CadServer::new, None)
}

/// Call before creating desktop threads or platform helpers. Their inherited
/// stdout must not retain the agent's pipe after this transport disconnects.
pub fn prepare_desktop_stdio() -> Result<(), String> {
    #[cfg(unix)]
    output_pipe::prepare_stdout_pipe()
        .map_err(|error| format!("MCP stdout inheritance: {error}"))?;
    Ok(())
}

/// Run on a worker while the native event loop remains on the main thread.
/// Merely opening CAD does not allocate a second kernel or document.
pub fn run_desktop_stdio() -> Result<(), String> {
    set_desktop_mcp_presence(DesktopMcpPresence::Waiting);
    let _clear = ClearDesktopMcpPresence;
    let result = run(
        || {
            let mut server = CadServer::new()?;
            server.desktop_binding = Some(DesktopBinding {
                process_id: std::process::id(),
                initial_selection_pending: true,
            });
            Ok(server)
        },
        Some(DESKTOP_TRANSPORT.get_or_init(DesktopTransport::default)),
    );

    let retired = {
        let _stdout = io::stdout().lock();
        let _stderr = io::stderr().lock();
        output_pipe::retire_stdout_pipe().map_err(|error| format!("MCP stdout retirement: {error}"))
    };
    match (result, retired) {
        (Err(error), Err(retirement)) => Err(format!("{error}; {retirement}")),
        (Err(error), _) | (_, Err(error)) => Err(error),
        _ => Ok(()),
    }
}

static DESKTOP_TRANSPORT: OnceLock<DesktopTransport> = OnceLock::new();

#[derive(Default)]
struct DesktopTransport {
    state: Mutex<TransportState>,
    flushed: Condvar,
}

#[derive(Default)]
struct TransportState {
    closing: bool,
    dispatching: bool,
}

struct DispatchGuard<'a>(&'a DesktopTransport);

impl DesktopTransport {
    fn begin(&self) -> Option<DispatchGuard<'_>> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.closing {
            return None;
        }
        state.dispatching = true;
        Some(DispatchGuard(self))
    }

    fn shutdown(&self, timeout: Duration) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.closing = true;
        let (state, _) = self
            .flushed
            .wait_timeout_while(state, timeout, |state| state.dispatching)
            .unwrap_or_else(|error| error.into_inner());
        !state.dispatching
    }
}

impl Drop for DispatchGuard<'_> {
    fn drop(&mut self) {
        self.0
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .dispatching = false;
        self.0.flushed.notify_all();
    }
}

/// Called outside the native event thread after it has acknowledged an exit.
/// Drain only the current response, including its flush, never the stdin reader.
/// False means the bounded wait expired (for example a host stopped reading).
pub fn shutdown_desktop_stdio(timeout: Duration) -> bool {
    DESKTOP_TRANSPORT
        .get()
        .is_none_or(|state| state.shutdown(timeout))
}

pub(super) fn instructions(desktop: bool) -> String {
    let mode = if desktop {
        "This stdio connection controls the desktop it launched. Its first document operation selects that desktop's published active document; startup may return desktop_not_ready. Selection remains pinned until an explicit attach or acknowledged UI document transition. Detaching requires another explicit selection. Use --headless for an independent document."
    } else {
        "This is one persistent headless CAD document. Attach explicitly to control a running desktop."
    };
    format!(
        "{mode} Finish sketches before creating solid features; reuse returned entity/body/face/edge ids. \
         Out-of-focus tools remain callable; discover them with cad_list_all_tools or soft focus. \
         Prefer cad_help and limo-cad://knowledge resources before web search for design guidance. \
         Build and iterate start to finish through MCP using cad_interface execute or individual tools; inspect solid_scene/cad_document between changes. \
         Use cad_route action batch for small ordered groups of literal typed operations on an explicit live route. Inspect its individual receipts; completed calls remain applied and pending calls must be polled with cad_route action status before any retry. \
         Before qualifying a desktop/MCP pair, inspect and require build_pair.status matched. Different, unknown or modified identities require explicit provenance checks. \
         Save the working design as .limo. Use upsert_named_view/rename_named_view/delete_named_view/recall_named_view/clear_named_view for review configurations. \
         Attached cad_interface inspect returns view_state; camera is null without a mounted modeling viewport. \
         For headless persistence use cad_project_model/cad_load_project_model. On desktop use cad_interface action file, command save, with an absolute .limo path; set overwrite true only to replace that file. \
         For desktop Undo/Redo use cad_interface action history, command undo or redo; inspect state.history for availability. \
         Require status applied receipts for all UI actions. \
         A completed launch, UI action or modeling apply retains its ready/applied receipt if snapshot loading fails: snapshot_error explains the failure and model_commands_blocked prevents stale-document commands. Recover with cad_attach/cad_refresh; do not repeat the completed action. \
         Recipe scripts are for explicitly requested teaching or replay on a blank document."
    )
}

pub(super) fn independent_of_default_document(name: &str, arguments: &Value) -> bool {
    match name {
        "cad_computer_control" => arguments["session_id"].is_string(),
        "cad_attach"
        | "cad_route"
        | "cad_detach"
        | "cad_list_sessions"
        | "cad_get_focus"
        | "cad_set_focus"
        | "cad_list_focus_areas"
        | "cad_get_tool_disclosure_mode"
        | "cad_set_tool_disclosure_mode"
        | "cad_list_all_tools"
        | "cad_help"
        | "material_catalog"
        | "printer_catalog" => true,
        "cad_interface" => {
            let action = arguments["action"].as_str();
            let grouped_global_read = action == Some("execute")
                && arguments["operation"].as_str().is_some_and(|operation| {
                    matches!(
                        operation,
                        "cad_help" | "material_catalog" | "printer_catalog"
                    )
                })
                && arguments["operation"]
                    .as_str()
                    .and_then(crate::interface::group_for)
                    .is_some_and(|group| arguments["group"] == group);
            let explicit_computer_control = action == Some("execute")
                && arguments["operation"] == "cad_computer_control"
                && arguments["group"] == "document/session"
                && arguments["arguments"]["session_id"].is_string();
            arguments["action"].is_null()
                || matches!(action, Some("catalog" | "recipes" | "launch"))
                || grouped_global_read
                || explicit_computer_control
                || (action != Some("execute") && arguments.get("session_id").is_some())
        }
        _ => false,
    }
}

enum StdinEvent {
    Line(String),
    Error(String),
    Eof,
}

fn read_lines(reader: impl BufRead, tx: Sender<StdinEvent>) {
    for line in reader.lines() {
        let event = match line {
            Ok(line) => StdinEvent::Line(line),
            Err(error) => {
                let _ = tx.send(StdinEvent::Error(format!("MCP stdin: {error}")));
                return;
            }
        };
        if tx.send(event).is_err() {
            return;
        }
    }
    let _ = tx.send(StdinEvent::Eof);
}

fn run(
    create: impl FnOnce() -> Result<CadServer, String>,
    desktop: Option<&DesktopTransport>,
) -> Result<(), String> {
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .name("limo-cad-mcp-input".into())
        .spawn(move || read_lines(io::stdin().lock(), tx))
        .map_err(|error| format!("MCP input worker: {error}"))?;
    serve_events(rx, &mut io::stdout().lock(), create, desktop)
}

fn write_messages(stdout: &mut impl Write, messages: &[Value]) -> Result<bool, String> {
    for message in messages {
        let result = (|| -> io::Result<()> {
            serde_json::to_writer(&mut *stdout, message)?;
            writeln!(stdout)?;
            stdout.flush()
        })();
        if let Err(error) = result {
            return if error.kind() == io::ErrorKind::BrokenPipe {
                Ok(false)
            } else {
                Err(format!("MCP stdout: {error}"))
            };
        }
    }
    Ok(true)
}

fn serve_events(
    rx: Receiver<StdinEvent>,
    stdout: &mut impl Write,
    create: impl FnOnce() -> Result<CadServer, String>,
    desktop: Option<&DesktopTransport>,
) -> Result<(), String> {
    let mut create = Some(create);
    let mut server: Option<CadServer> = None;
    loop {
        if let Some(server) = server.as_mut() {
            let due = idle_due_messages(server);
            if !due.is_empty() {
                let _dispatch = match desktop {
                    Some(state) => match state.begin() {
                        Some(guard) => Some(guard),
                        None => return Ok(()),
                    },
                    None => None,
                };
                if !write_messages(stdout, &due)? {
                    return Ok(());
                }
                continue;
            }
        }
        let wake = server
            .as_mut()
            .and_then(|server| server.disclosure.ms_until_wake());
        let event = match wake {
            Some(ms) => match rx.recv_timeout(Duration::from_millis(ms.max(1))) {
                Ok(event) => event,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            },
            None => match rx.recv() {
                Ok(event) => event,
                Err(_) => return Ok(()),
            },
        };
        let line = match event {
            StdinEvent::Eof => return Ok(()),
            StdinEvent::Error(error) => return Err(error),
            StdinEvent::Line(line) if line.trim().is_empty() => continue,
            StdinEvent::Line(line) => line,
        };
        let _dispatch = match desktop {
            Some(state) => match state.begin() {
                Some(guard) => Some(guard),
                None => return Ok(()),
            },
            None => None,
        };
        let outgoing = match serde_json::from_str::<Value>(&line) {
            Ok(message) => {
                let initialized = desktop.is_some()
                    && message.get("method").and_then(Value::as_str) == Some("initialize");
                if server.is_none() {
                    server = Some(create.take().expect("server initialized once")()?);
                }
                let outgoing = handle_message(server.as_mut().unwrap(), message);
                if initialized
                    && outgoing
                        .iter()
                        .all(|message| message.get("error").is_none())
                {
                    set_desktop_mcp_presence(DesktopMcpPresence::Attached);
                }
                outgoing
            }
            Err(error) => vec![error_response(
                Value::Null,
                -32700,
                format!("parse error: {error}"),
            )],
        };
        if !write_messages(stdout, &outgoing)? {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn events(input: &[u8]) -> Receiver<StdinEvent> {
        let (tx, rx) = mpsc::channel();
        read_lines(io::Cursor::new(input), tx);
        rx
    }

    #[test]
    fn eof_blank_input_and_invalid_json_do_not_construct_a_document() {
        for input in ["", "\n \r\n", "invalid json\n"] {
            let mut output = Vec::new();
            serve_events(
                events(input.as_bytes()),
                &mut output,
                || panic!("idle input constructed CAD"),
                None,
            )
            .unwrap();
            if input.starts_with("invalid") {
                let reply: Value = serde_json::from_slice(&output).unwrap();
                assert_eq!(reply["error"]["code"], -32700);
            } else {
                assert!(output.is_empty());
            }
        }
    }

    #[test]
    fn first_protocol_message_returns_startup_failure_without_exiting_process() {
        let error = serve_events(
            events(b"{\"method\":\"initialize\",\"id\":1}\n"),
            &mut Vec::new(),
            || Err("kernel unavailable".into()),
            None,
        )
        .unwrap_err();
        assert_eq!(error, "kernel unavailable");
    }

    #[test]
    fn read_failure_is_returned_and_broken_output_finishes_transport() {
        let (tx, rx) = mpsc::channel();
        tx.send(StdinEvent::Error("read failed".into())).unwrap();
        assert_eq!(
            serve_events(rx, &mut Vec::new(), || panic!("unexpected CAD"), None).unwrap_err(),
            "read failed"
        );
        struct Closed;
        impl Write for Closed {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        serve_events(
            events(b"invalid\n"),
            &mut Closed,
            || panic!("unexpected CAD"),
            None,
        )
        .unwrap();
    }

    #[test]
    fn desktop_shutdown_does_not_wait_for_an_idle_reader_and_rejects_queued_work() {
        let desktop = DesktopTransport::default();
        assert!(desktop.shutdown(Duration::ZERO));
        let mut output = Vec::new();
        serve_events(
            events(b"{\"method\":\"initialize\",\"id\":1}\n"),
            &mut output,
            || panic!("dispatch started after shutdown"),
            Some(&desktop),
        )
        .unwrap();
        assert!(output.is_empty());
    }

    #[test]
    fn desktop_shutdown_waits_through_response_flush_and_never_dispatches_the_next_message() {
        let desktop = DesktopTransport::default();
        let (flushing_tx, flushing_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        struct HeldFlush(Sender<()>, Receiver<()>);
        impl Write for HeldFlush {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                self.0.send(()).unwrap();
                self.1.recv().unwrap();
                Ok(())
            }
        }
        thread::scope(|scope| {
            let worker = scope.spawn(|| {
                serve_events(
                    events(b"invalid\n{\"method\":\"initialize\",\"id\":2}\n"),
                    &mut HeldFlush(flushing_tx, release_rx),
                    || panic!("second message dispatched after shutdown"),
                    Some(&desktop),
                )
            });
            flushing_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            assert!(
                !desktop.shutdown(Duration::from_millis(20)),
                "response has not flushed yet"
            );
            release_tx.send(()).unwrap();
            worker.join().unwrap().unwrap();
        });
        assert!(desktop.shutdown(Duration::ZERO));
    }

    #[test]
    fn desktop_discovery_is_available_before_ready_but_model_calls_require_a_target() {
        for (name, args) in [
            ("cad_list_sessions", json!({})),
            ("cad_attach", json!({"session_id":"explicit"})),
            ("cad_help", json!({"action":"topics"})),
            ("material_catalog", json!({})),
            ("printer_catalog", json!({})),
            ("cad_interface", json!({"action":"catalog"})),
            ("cad_interface", json!({"action":"recipes"})),
            (
                "cad_interface",
                json!({"action":"execute", "group":"document/session", "operation":"cad_help",
                    "arguments":{"action":"topics"}}),
            ),
            (
                "cad_interface",
                json!({"action":"script","session_id":"explicit"}),
            ),
        ] {
            assert!(independent_of_default_document(name, &args));
        }
        for (name, args) in [
            ("sketch_create", json!({})),
            ("cad_project_model", json!({})),
            (
                "cad_interface",
                json!({"action":"execute","session_id":"ignored"}),
            ),
            (
                "cad_interface",
                json!({"action":"execute", "operation":"cad_help",
                    "arguments":{"action":"topics"}}),
            ),
            (
                "cad_interface",
                json!({"action":"execute", "group":"not-a-group", "operation":"cad_help",
                    "arguments":{"action":"topics"}}),
            ),
            (
                "cad_interface",
                json!({"action":"execute", "group":"document/session", "operation":"cad_interface",
                    "arguments":{"action":"catalog"}}),
            ),
            (
                "cad_interface",
                json!({"action":"execute", "group":"document/session", "operation":"sketch_begin",
                    "arguments":{}}),
            ),
            (
                "cad_interface",
                json!({"action":"script","recipe":"fillet-basics"}),
            ),
            ("cad_interface", json!({"action":"window","mode":"close"})),
        ] {
            assert!(!independent_of_default_document(name, &args));
        }
        assert!(instructions(true).contains("desktop_not_ready"));
        assert!(!instructions(true).contains("persistent headless"));
    }

    #[test]
    fn desktop_presence_is_off_until_a_worker_marks_it() {
        let previous = desktop_mcp_presence();
        set_desktop_mcp_presence(DesktopMcpPresence::Waiting);
        assert_eq!(desktop_mcp_presence(), DesktopMcpPresence::Waiting);
        set_desktop_mcp_presence(DesktopMcpPresence::Attached);
        assert_eq!(desktop_mcp_presence(), DesktopMcpPresence::Attached);
        set_desktop_mcp_presence(previous);
    }
}
