//! Rebuild when the filament catalog changes.
//! Source of truth remains `presets/catalog.json`, embedded by the Rust engine.

use std::env;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let source = manifest_dir.join("presets/catalog.json");
    println!("cargo:rerun-if-changed={}", source.display());
}
