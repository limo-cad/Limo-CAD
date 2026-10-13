//! Validate the vectors embedded by the shared Bevy ribbon.
use anyhow::{ensure, Context, Result};
use regex::Regex;
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs};

pub fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    ensure!(args.next().is_none(), "audit-icons takes no arguments");
    let root = crate::release_tooling::root();
    let registry =
        fs::read_to_string(root.join("desktop/src/native_viewport/interface_shell/ribbon.rs"))?;
    let declared: BTreeSet<_> = Regex::new(r#"source!\("([A-Za-z][A-Za-z0-9-]*)"\)"#)?
        .captures_iter(&registry)
        .map(|capture| capture[1].to_owned())
        .collect();
    ensure!(!declared.is_empty(), "Bevy ribbon vector registry is empty");
    let forbidden = Regex::new(
        r"(?i)<(?:image|script|foreignObject)\b|\bon\w+\s*=|\b(?:href|xlink:href)\s*=|data:image/",
    )?;
    let mut available = BTreeSet::new();
    for entry in fs::read_dir(root.join("assets/ribbon-icons"))? {
        let entry = entry?;
        if entry.file_name() == "LICENSE.lucide" {
            continue;
        }
        ensure!(
            entry.file_type()?.is_file() && entry.path().extension().is_some_and(|s| s == "svg"),
            "Invalid ribbon vector {}",
            entry.path().display()
        );
        let svg = fs::read_to_string(entry.path())?;
        ensure!(
            svg.contains("viewBox=\"0 0 24 24\"") && !forbidden.is_match(&svg),
            "Invalid or executable ribbon vector {}",
            entry.path().display()
        );
        available.insert(
            entry
                .path()
                .file_stem()
                .context("Vector filename missing")?
                .to_string_lossy()
                .into_owned(),
        );
    }
    ensure!(
        declared.is_subset(&available),
        "Missing Bevy vectors: {:?}",
        declared.difference(&available).collect::<Vec<_>>()
    );
    ensure!(
        root.join("assets/ribbon-icons/LICENSE.lucide").is_file(),
        "Lucide license missing"
    );
    let brand = fs::read_to_string(root.join("public/app-icon.svg"))?.replace("\r\n", "\n");
    ensure!(
        brand.contains("Limo CAD NB monogram") && !forbidden.is_match(&brand),
        "Canonical mark lacks provenance or embeds executable content"
    );
    let provenance = fs::read_to_string(root.join("docs/ICON_PROVENANCE.md"))?;
    ensure!(
        provenance.contains("assets/ribbon-icons") && provenance.contains("LICENSE.lucide"),
        "Vector provenance is missing"
    );
    println!(
        "Bevy icon audit OK: {} embedded vectors, {} available",
        declared.len(),
        available.len()
    );
    println!(
        "app-icon.svg sha256 {}",
        crate::hash::hex(&Sha256::digest(brand.as_bytes()))
    );
    Ok(())
}
