//! Prepare an explicitly separate shader-cold macOS test app.
#[cfg(target_os = "macos")]
mod macos;

use anyhow::{bail, Context, Result};
use std::path::PathBuf;

#[derive(Default)]
struct Options {
    app: Option<PathBuf>,
    out: Option<PathBuf>,
    verify: bool,
    help: bool,
}

impl Options {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Self> {
        let mut options = Self::default();
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--app" if options.app.is_none() => {
                    options.app = Some(
                        args.next()
                            .context("--app requires a packaged .app")?
                            .into(),
                    );
                }
                "--out" if options.out.is_none() => {
                    options.out = Some(
                        args.next()
                            .context("--out requires a new directory")?
                            .into(),
                    );
                }
                "--verify" if !options.verify => options.verify = true,
                "--help" | "-h" if !options.help => options.help = true,
                _ => bail!("unknown or duplicate cold-start option {argument}"),
            }
        }
        Ok(options)
    }
}

pub(crate) fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let options = Options::parse(args)?;
    if options.help {
        println!(
            "Usage: cargo xtask prepare-cold-start --app RELEASE.app --out NEW_ABSOLUTE_DIRECTORY [--verify]\n\
             macOS only. Copies an existing signed release without changing compiled sections.\n\
             A test-only pre-main helper creates a fresh Metal cache on every launch.\n\
             --verify exercises two normal Mac launches of a small GPU probe, not CAD.\n\
             Existing settings and caches are preserved; the output directory must not exist.\n\
             This isolates shader compilation caches, not OS/filesystem memory caches."
        );
        return Ok(());
    }
    let app = options.app.context("missing --app")?;
    let out = options.out.context("missing --out")?;
    #[cfg(target_os = "macos")]
    return macos::prepare(&app, &out, options.verify);
    #[cfg(not(target_os = "macos"))]
    bail!(
        "prepare-cold-start requires macOS (app: {}, output: {})",
        app.display(),
        out.display()
    );
}
