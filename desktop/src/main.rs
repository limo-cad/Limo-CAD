//! Limo CAD desktop entry point.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod startup;

fn main() -> std::process::ExitCode {
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

    if let Startup::Recipe(recipe) = startup {
        if matches!(
            limo_cad_mcp::open_recipe_in_running_desktop(recipe),
            Ok(true)
        ) {
            return std::process::ExitCode::SUCCESS;
        }
    }

    if let Ok(executable) = std::env::current_exe() {
        std::env::set_var("LIMO_CAD_DESKTOP_BIN", &executable);
        std::env::set_var("LIMO_CAD_LOCAL_RUNTIME", &executable);
    }

    if startup == Startup::Headless {
        return match limo_cad_mcp::run_stdio() {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Limo CAD MCP failed: {error}");
                std::process::ExitCode::FAILURE
            }
        };
    }

    if let Err(error) = limo_cad_mcp::prepare_desktop_stdio() {
        eprintln!("Could not prepare local stdio MCP: {error}");
        return std::process::ExitCode::FAILURE;
    }

    if let Err(error) = std::thread::Builder::new()
        .name("cad-stdio".into())
        .spawn(|| {
            if let Err(error) = limo_cad_mcp::run_desktop_stdio() {
                eprintln!("Limo CAD stdio MCP disconnected: {error}");
            }
        })
    {
        eprintln!("Could not start local stdio MCP: {error}");
    }
    let (recipe, project) = match &startup {
        Startup::Recipe(recipe) => (Some(*recipe), None),
        Startup::Project(path) => (None, Some(path.as_path())),
        _ => (None, None),
    };
    let exit = limo_cad::native_viewport::winit_host::run_with_startup(recipe, project);
    let _ = limo_cad_mcp::shutdown_desktop_stdio(std::time::Duration::from_secs(3));
    exit
}
