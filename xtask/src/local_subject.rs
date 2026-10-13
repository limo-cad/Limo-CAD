//! Managed local checks select a verified current runtime, rebuilding stale inputs.
use anyhow::{ensure, Context, Result};

pub(crate) fn prepare(command: &str, mut arguments: Vec<String>) -> Result<Vec<String>> {
    let installed = command == "cad-call" && installed_requested(&arguments);
    if matches!(arguments.as_slice(), [argument] if matches!(argument.as_str(), "--help" | "-h"))
        || !matches!(
            command,
            "test-mcp" | "verify-package-mcp" | "run-script" | "cad-call"
        )
        || !installed
            && std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true")
            && std::env::var("RUNNER_ENVIRONMENT").as_deref() == Ok("github-hosted")
        || !installed && !crate::deploy_native::managed_install_exists()?
    {
        return Ok(arguments);
    }

    let worker_arguments = command != "test-mcp";
    let desktop_path = command != "verify-package-mcp";
    let mut server = None;
    let mut desktop = None;
    let mut headless = false;
    let mut installed_seen = false;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--installed" if command == "cad-call" => {
                ensure!(!installed_seen, "Duplicate --installed option");
                installed_seen = true;
                arguments.remove(index);
            }
            "--server" => {
                ensure!(server.is_none(), "Duplicate --server option");
                ensure!(index + 1 < arguments.len(), "Missing --server path");
                server = Some(index + 1);
                index += 2;
            }
            "--desktop" if desktop_path => {
                ensure!(desktop.is_none(), "Duplicate --desktop option");
                ensure!(index + 1 < arguments.len(), "Missing --desktop path");
                desktop = Some(index + 1);
                index += 2;
            }
            "--server-arg" => {
                let value = arguments
                    .get(index + 1)
                    .context("Missing --server-arg value")?;
                if worker_arguments {
                    ensure!(
                        value == "--headless" && !headless,
                        "Managed local workers accept exactly one --headless argument"
                    );
                    headless = true;
                }
                index += 2;
            }
            "--session"
            | "--tool"
            | "--args"
            | "--args-file"
            | "--out"
            | "--init-timeout-seconds"
                if command == "cad-call" =>
            {
                ensure!(
                    index + 1 < arguments.len(),
                    "Missing {} value",
                    arguments[index]
                );
                index += 2;
            }
            _ => index += 1,
        }
    }

    let runtime = if installed {
        crate::deploy_native::verify_installed_for_control()?
    } else {
        let source = crate::deploy_native::managed_source_checkout()?;
        crate::deploy_native::prepare_runtime(
            &source,
            &crate::deploy_native::BuildOptions::default(),
        )
        .context("Prepare the current managed CAD runtime before local checks")?
    };
    let runtime = runtime
        .to_str()
        .context("Managed CAD executable path must be Unicode")?
        .to_owned();
    if let Some(index) = server {
        arguments[index] = runtime.clone();
    } else {
        arguments.extend(["--server".into(), runtime.clone()]);
    }
    if let Some(index) = desktop {
        arguments[index] = runtime.clone();
    }
    if worker_arguments && !headless {
        arguments.extend(["--server-arg".into(), "--headless".into()]);
    }
    eprintln!("Managed local CAD subject: {runtime}");
    Ok(arguments)
}

/// Values are literal, so an argument to --server-arg cannot select this mode.
fn installed_requested(arguments: &[String]) -> bool {
    let mut arguments = arguments.iter();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--installed" => return true,
            "--server"
            | "--server-arg"
            | "--session"
            | "--tool"
            | "--args"
            | "--args-file"
            | "--out"
            | "--init-timeout-seconds" => {
                arguments.next();
            }
            _ => {}
        }
    }
    false
}
