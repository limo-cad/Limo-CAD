//! noBS CAD desktop entry point.

// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> std::process::ExitCode {
    nbcad_lib::startup_metrics::begin();
    use nbcad_lib::startup;
    use startup::Startup;
    let startup = match startup::parse(std::env::args_os().skip(1)) {
        Ok(startup) => startup,
        Err(error) => {
            eprintln!("{error}");
            return std::process::ExitCode::from(2);
        }
    };
    if startup == Startup::Help {
        println!("{}", startup::USAGE);
        return std::process::ExitCode::SUCCESS;
    }

    // Only this artifact/profile can receive a relaunch. Never attach older
    // installed desktops through the general MCP session registry.
    let launch_requests = if let Startup::Desktop(ref launch) = startup {
        match nbcad_lib::launch_requests::prepare(
            launch.new_window,
            launch.reuse_window,
            &launch.targets,
        ) {
            Ok(nbcad_lib::launch_requests::Relaunch::Forwarded) => {
                return std::process::ExitCode::SUCCESS
            }
            Ok(nbcad_lib::launch_requests::Relaunch::Primary(state)) => Some(state),
            Err(error) => {
                eprintln!("Could not reuse an existing window: {error}");
                Some(nbcad_lib::launch_requests::LaunchRequestsState::standalone(
                    &launch.targets,
                ))
            }
        }
    } else {
        None
    };

    if std::env::var_os("NBCAD_DESKTOP_BIN").is_none() {
        if let Ok(executable) = std::env::current_exe() {
            std::env::set_var("NBCAD_DESKTOP_BIN", executable);
        }
    }
    // Suppress the entire GUI/platform initialization, not the MCP interface.
    // Retain agent-supplied pipes, including Windows GUI-subsystem builds.
    if startup == Startup::Headless {
        return match nbcad_mcp::run_stdio() {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("noBS CAD MCP failed: {error}");
                std::process::ExitCode::FAILURE
            }
        };
    }

    #[cfg(target_os = "linux")]
    {
        // GTK 3 exposes child widget windows as real X11 windows, which lets
        // wgpu own a Vulkan surface beneath WebKitGTK. Under native Wayland it
        // exposes the top-level wl_surface instead; GTK and Vulkan would then
        // attach competing buffers to one compositor-owned surface. Ubuntu's
        // Wayland desktop supplies XWayland for this compatibility path.
        std::env::set_var("GDK_BACKEND", "x11");
    }
    // Prepare before either worker or GUI startup: WebKitGTK can spawn helpers
    // immediately, and none may inherit the agent's output pipe.
    if let Err(error) = nbcad_mcp::prepare_desktop_stdio() {
        eprintln!("Could not prepare local stdio MCP: {error}");
        return std::process::ExitCode::FAILURE;
    }
    // Tauri owns the main thread and application lifetime. Agent disconnects
    // retire only this worker; never join its potentially blocked stdin reader.
    if let Err(error) = std::thread::Builder::new()
        .name("cad-stdio".into())
        .spawn(|| {
            if let Err(error) = nbcad_mcp::run_desktop_stdio() {
                eprintln!("noBS CAD stdio MCP disconnected: {error}");
            }
        })
    {
        eprintln!("Could not start local stdio MCP: {error}");
    }
    nbcad_lib::run_with_launch_requests(launch_requests.unwrap_or_default());
    std::process::ExitCode::SUCCESS
}
