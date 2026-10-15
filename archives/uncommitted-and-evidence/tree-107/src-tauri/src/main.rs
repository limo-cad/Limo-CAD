//! noBS CAD desktop entry point.

// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod startup;

fn main() -> std::process::ExitCode {
    nbcad_lib::startup_metrics::begin();
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

    // Browser URL launches may reuse one live window without loading its model
    // or suppressing ordinary independent launches. Validate before GUI init.
    if let Startup::Recipe(recipe) = startup {
        if matches!(nbcad_mcp::open_recipe_in_running_desktop(recipe), Ok(true)) {
            return std::process::ExitCode::SUCCESS;
        }
    }

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
    nbcad_lib::run();
    std::process::ExitCode::SUCCESS
}
