//! Diagnostic only: observe the existing Winit/AppKit boundary without changing
//! IME state, input order, or timing. Never enabled for normal desktop use.
use bevy::log::{
    tracing_subscriber::{filter::filter_fn, fmt, fmt::format::FmtSpan, Layer},
    BoxedFmtLayer, Level, LogPlugin,
};

mod budget;

const BYTE_LIMIT: usize = 1024 * 1024;

pub(super) fn enabled() -> bool {
    enabled_for(std::env::consts::OS, |key| std::env::var(key).ok())
}

fn enabled_for(platform: &str, read: impl Fn(&str) -> Option<String>) -> bool {
    platform == "macos"
        && [
            ("NBCAD_NATIVE_IME_TRACE", "1"),
            ("NBCAD_NATIVE_IME_TEST", "macos-japanese"),
            ("GITHUB_ACTIONS", "true"),
            ("RUNNER_OS", "macOS"),
            ("RUNNER_ENVIRONMENT", "github-hosted"),
            ("GITHUB_REPOSITORY", "jackControls/noBS-CAD"),
        ]
        .into_iter()
        .all(|(key, expected)| read(key).as_deref() == Some(expected))
        && read("GITHUB_RUN_ID")
            .is_some_and(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
}

pub(super) fn plugin() -> LogPlugin {
    LogPlugin {
        filter: "off,winit=trace".into(),
        level: Level::ERROR,
        fmt_layer: |_| Some(layer()),
        ..Default::default()
    }
}

fn layer() -> BoxedFmtLayer {
    let writer = budget::Bounded::new(std::io::stderr(), BYTE_LIMIT);
    // This marker also distinguishes an absent logger from no Winit callbacks.
    let mut marker = writer.clone();
    let _ = std::io::Write::write_all(
        &mut marker,
        b"NBCAD_NATIVE_IME_TRACE enabled; Winit callback scopes + set_ime_allowed; maximum 1048576 bytes\n",
    );
    layer_with(writer)
}

fn layer_with<W: std::io::Write + Send + 'static>(writer: budget::Bounded<W>) -> BoxedFmtLayer {
    Box::new(
        fmt::layer()
            .with_ansi(false)
            .with_span_events(FmtSpan::NEW)
            .with_writer(move || writer.clone())
            .with_filter(filter_fn(|metadata| {
                selected(metadata.target(), metadata.name(), metadata.is_span())
            })),
    )
}

fn selected(target: &str, name: &str, is_span: bool) -> bool {
    if is_span {
        return target == "winit::window" && name == "winit::Window::set_ime_allowed";
    }
    // Winit 0.30.13 TraceGuard uses `target = module_path` as an event FIELD,
    // not `target: module_path` metadata. Its two callsites live in `util` and
    // carry only static callback names and module paths, never inserted text.
    // Retain surrounding AppKit callbacks too so focus/order remain visible.
    target == "winit::platform_impl::macos::util"
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::log::{
        tracing,
        tracing_subscriber::{prelude::*, EnvFilter, Registry},
        BoxedLayer,
    };
    use std::{
        io::Write,
        sync::{Arc, Mutex},
    };

    fn environment(key: &str) -> Option<String> {
        Some(
            match key {
                "NBCAD_NATIVE_IME_TRACE" => "1",
                "NBCAD_NATIVE_IME_TEST" => "macos-japanese",
                "GITHUB_ACTIONS" => "true",
                "RUNNER_OS" => "macOS",
                "RUNNER_ENVIRONMENT" => "github-hosted",
                "GITHUB_REPOSITORY" => "jackControls/noBS-CAD",
                "GITHUB_RUN_ID" => "36366040955",
                _ => return None,
            }
            .into(),
        )
    }

    #[test]
    fn trace_requires_every_explicit_disposable_macos_guard() {
        assert!(enabled_for("macos", environment));
        assert!(!enabled_for("windows", environment));
        for absent in [
            "NBCAD_NATIVE_IME_TRACE",
            "NBCAD_NATIVE_IME_TEST",
            "GITHUB_ACTIONS",
            "RUNNER_OS",
            "RUNNER_ENVIRONMENT",
            "GITHUB_REPOSITORY",
            "GITHUB_RUN_ID",
        ] {
            assert!(!enabled_for("macos", |key| {
                if key == absent {
                    None
                } else {
                    environment(key)
                }
            }));
        }
        for invalid in ["", "local", "12 34"] {
            assert!(!enabled_for("macos", |key| {
                if key == "GITHUB_RUN_ID" {
                    Some(invalid.into())
                } else {
                    environment(key)
                }
            }));
        }
    }

    #[test]
    fn trace_allows_only_callback_events_and_ime_setter_span() {
        assert!(selected(
            "winit::platform_impl::macos::util",
            "event util.rs:23",
            false
        ));
        assert!(selected(
            "winit::window",
            "winit::Window::set_ime_allowed",
            true
        ));
        assert!(!selected(
            "winit::platform_impl::macos::view",
            "event view.rs:1",
            false
        ));
        assert!(!selected("winit::window", "winit::Window::set_title", true));
        assert!(!selected(
            "winit::window",
            "winit::Window::set_ime_allowed",
            false
        ));
        assert!(!selected("nbcad_lib", "document", false));
    }

    #[test]
    fn formatter_records_callback_field_and_setter_boolean_without_other_payloads() {
        #[derive(Clone)]
        struct Output(Arc<Mutex<Vec<u8>>>);
        impl Write for Output {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let subscriber = Registry::default()
            .with(None::<BoxedLayer>)
            // Even a broader environment filter cannot bypass the layer filter.
            .with(EnvFilter::new("trace"))
            .with(layer_with(budget::Bounded::new(
                Output(bytes.clone()),
                BYTE_LIMIT,
            )));
        tracing::subscriber::with_default(subscriber, || {
            let _span = tracing::debug_span!(target: "winit::window", "winit::Window::set_ime_allowed", allowed = true).entered();
            tracing::trace!(target: "winit::platform_impl::macos::util", target = "winit::platform_impl::macos::view", "Triggered `{}`", "keyDown:");
            tracing::trace!(target: "winit::platform_impl::macos::view", "forbidden inserted text");
            tracing::info!(target: "nbcad_lib", "forbidden document payload");
        });
        let output = String::from_utf8(bytes.lock().unwrap().clone()).unwrap();
        assert!(
            output.contains("winit::Window::set_ime_allowed{allowed=true}: winit::window: new"),
            "{output}"
        );
        assert!(output.contains("Triggered `keyDown:`"), "{output}");
        assert!(
            output.contains("winit::platform_impl::macos::view"),
            "{output}"
        );
        assert!(!output.contains("forbidden"), "{output}");
    }
}
