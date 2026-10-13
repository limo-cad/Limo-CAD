//! Fetch geometry facts only, excluding process presets and G-code.
use anyhow::{bail, Context, Result};
use limo_cad_core::{
    PrintBedDto, PrintNozzleMode, PrintProfileSource, PrinterCatalogDto, PrinterExtruderDto,
    PrinterProfileDto,
};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut fetch = false;
    let mut check = false;
    for arg in args {
        match arg.as_str() {
            "--fetch" => fetch = true,
            "--check" => check = true,
            "--help" => {
                println!("cargo xtask printer-profiles [--fetch] [--check]\nPinned sources: crates/core/data/printer-sources.json.\nWithout --fetch, use sources cached in target/printer-profiles.\n--check verifies the catalog without writing it.\nNormal app builds embed the checked-in catalog and need no network.");
                return Ok(());
            }
            _ => bail!("Unknown printer-profiles option {arg}"),
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let manifest: Value = serde_json::from_slice(&fs::read(
        root.join("crates/core/data/printer-sources.json"),
    )?)?;
    let mut profiles = vec![];
    for source in manifest["sources"]
        .as_array()
        .context("sources must be an array")?
    {
        let repository = source["repository"].as_str().context("repository")?;
        let revision = source["revision"].as_str().context("revision")?;
        if !matches!(repository, "bambulab/BambuStudio" | "OrcaSlicer/OrcaSlicer")
            || revision.len() != 40
            || !revision.bytes().all(|c| c.is_ascii_hexdigit())
        {
            bail!("Source must use an approved repository and full commit SHA");
        }
        let profile = source["profile"].as_str().context("profile")?;
        let cache = root
            .join("target/printer-profiles")
            .join(repository)
            .join(revision);
        let mut files = BTreeMap::new();
        let resolved = resolve(profile, &mut BTreeSet::new(), &mut files, &mut |path| {
            read_source(&cache, repository, revision, path, fetch)
        })?;
        let provenance = PrintProfileSource {
            repository: repository.into(),
            revision: revision.into(),
            profile: profile.into(),
            files,
        };
        profiles.push(normalize(
            source["id"].as_str().context("id")?,
            &resolved,
            provenance,
        )?);
    }
    let catalog = PrinterCatalogDto {
        schema_version: 1,
        profiles,
    };
    if catalog.profiles.is_empty() {
        bail!("Empty printer catalog");
    }
    let mut generated = serde_json::to_string_pretty(&catalog)?;
    generated.push('\n');
    let destination = root.join("crates/core/data/printers.json");
    if check {
        if serde_json::from_slice::<Value>(&fs::read(&destination)?)?
            != serde_json::from_str::<Value>(&generated)?
        {
            bail!("Embedded printer catalog differs from pinned sources; run cargo xtask printer-profiles --fetch and review the diff");
        }
        println!(
            "Embedded printer catalog matches {} pinned sources",
            catalog.profiles.len()
        );
    } else {
        fs::write(destination, generated)?;
        println!(
            "Generated {} embedded printer profiles",
            catalog.profiles.len()
        );
    }
    Ok(())
}
fn read_source(
    cache: &Path,
    repository: &str,
    revision: &str,
    path: &str,
    fetch: bool,
) -> Result<Vec<u8>> {
    validate_profile_path(path)?;
    let destination = cache.join(path);
    if fetch {
        fs::create_dir_all(destination.parent().unwrap())?;
        let temporary = destination.with_extension("download");
        let url = format!(
            "https://raw.githubusercontent.com/{repository}/{revision}/{}",
            path.replace(' ', "%20")
        );
        let status = Command::new("curl")
            .args([
                "--fail",
                "--silent",
                "--show-error",
                "--location",
                "--proto",
                "=https",
                "--tlsv1.2",
                "--max-time",
                "30",
                "--retry",
                "2",
                "--output",
            ])
            .arg(&temporary)
            .arg(&url)
            .status()
            .context("curl is required to fetch profiles")?;
        if !status.success() {
            bail!("Cannot fetch {url}; catalog left unchanged");
        }
        let bytes = fs::read(&temporary)?;
        if bytes.len() > 2_000_000 {
            bail!("Profile exceeds size limit");
        }
        serde_json::from_slice::<Value>(&bytes).context("Invalid upstream JSON")?;
        fs::write(&destination, bytes)?;
        fs::remove_file(temporary)?;
    }
    fs::read(&destination).with_context(|| format!("Missing cached profile {path}; use --fetch"))
}
fn safe_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment.trim() == segment
        && !segment.ends_with('.')
        && !segment.contains("..")
        && !segment
            .chars()
            .any(|c| c.is_control() || "\\/:<>\"|?*%#".contains(c))
        && !matches!(
            segment
                .split('.')
                .next()
                .unwrap()
                .trim_end()
                .to_ascii_uppercase()
                .as_str(),
            "CON"
                | "PRN"
                | "AUX"
                | "NUL"
                | "COM1"
                | "COM2"
                | "COM3"
                | "COM4"
                | "COM5"
                | "COM6"
                | "COM7"
                | "COM8"
                | "COM9"
                | "LPT1"
                | "LPT2"
                | "LPT3"
                | "LPT4"
                | "LPT5"
                | "LPT6"
                | "LPT7"
                | "LPT8"
                | "LPT9"
        )
}
fn validate_profile_path(path: &str) -> Result<()> {
    if !path.starts_with("resources/profiles/")
        || !path.ends_with(".json")
        || !path.split('/').all(safe_segment)
    {
        bail!("Unsafe profile path {path}");
    }
    Ok(())
}
fn resolve(
    path: &str,
    active: &mut BTreeSet<String>,
    files: &mut BTreeMap<String, String>,
    read: &mut impl FnMut(&str) -> Result<Vec<u8>>,
) -> Result<Map<String, Value>> {
    validate_profile_path(path)?;
    if active.len() > 32 || !active.insert(path.into()) {
        bail!("Profile inheritance cycle or excessive depth at {path}");
    }
    let bytes = read(path)?;
    files.insert(path.into(), crate::hash::hex(&Sha256::digest(&bytes)));
    let value: Value = serde_json::from_slice(&bytes)?;
    let object = value.as_object().context("Profile must be an object")?;
    let mut merged = if let Some(parent) = object.get("inherits") {
        let parent = parent.as_str().context("inherits must be a string")?;
        if !safe_segment(parent) {
            bail!("Unsafe inherited profile name");
        }
        let parent_path = PathBuf::from(path)
            .parent()
            .unwrap()
            .join(format!("{parent}.json"))
            .to_string_lossy()
            .replace('\\', "/");
        resolve(&parent_path, active, files, read)?
    } else {
        Map::new()
    };
    merged.extend(object.clone());
    active.remove(path);
    Ok(merged)
}
fn number(v: &Value) -> Result<f64> {
    let n = if let Some(s) = v.as_str() {
        s.parse()?
    } else {
        v.as_f64().context("Expected numeric profile field")?
    };
    if !n.is_finite() || n <= 0. || n > 1e6 {
        bail!("Invalid profile dimension");
    }
    Ok(n)
}
fn polygon(v: &Value) -> Result<Vec<[f64; 2]>> {
    let tokens: Vec<&str> = if let Some(s) = v.as_str() {
        s.split(',').collect()
    } else {
        v.as_array()
            .context("Expected polygon")?
            .iter()
            .map(|v| v.as_str().context("Expected XY string"))
            .collect::<Result<_>>()?
    };
    tokens
        .into_iter()
        .map(|s| {
            let (x, y) = s
                .trim()
                .split_once('x')
                .context("Expected XxY coordinate")?;
            Ok([x.parse()?, y.parse()?])
        })
        .collect()
}
fn normalize(
    id: &str,
    v: &Map<String, Value>,
    source: PrintProfileSource,
) -> Result<PrinterProfileDto> {
    let get = |k: &str| {
        v.get(k)
            .with_context(|| format!("Missing inherited geometry field {k}"))
    };
    let area = polygon(get("printable_area")?)?;
    let height = number(get("printable_height")?)?;
    let regions = get("extruder_printable_area")?
        .as_array()
        .context("extruder areas")?;
    let heights = get("extruder_printable_height")?
        .as_array()
        .context("extruder heights")?;
    if regions.len() != heights.len() || regions.len() < 2 {
        bail!("Expected at least two matched extruder regions/heights");
    }
    let extruders: Vec<_> = regions
        .iter()
        .zip(heights)
        .map(|(a, h)| {
            Ok(PrinterExtruderDto {
                printable_region: polygon(a)?,
                height_mm: number(h)?,
            })
        })
        .collect::<Result<_>>()?;
    let excluded = match v.get("bed_exclude_area") {
        Some(Value::Array(a)) if a.is_empty() => vec![],
        Some(a) => {
            let flat = polygon(a)?;
            if flat.len() % 4 != 0 {
                bail!("Excluded bed areas must be four-point polygons");
            }
            flat.as_chunks::<4>().0.iter().map(|p| p.to_vec()).collect()
        }
        None => vec![],
    };
    let master = v
        .get("master_extruder_id")
        .and_then(Value::as_str)
        .context("Missing master extruder identity")?;
    let map = get("physical_extruder_map")?
        .as_array()
        .context("physical extruder map")?;
    let main_index = map
        .iter()
        .position(|id| id.as_str() == Some(master))
        .context("Master extruder not found")?;
    let main_extruder = extruders
        .get(main_index)
        .context("Master extruder geometry missing")?;
    let make = |mode, regions: Vec<Vec<[f64; 2]>>, height: f64| -> Result<PrintBedDto> {
        let mut min = [f64::NEG_INFINITY; 2];
        let mut max = [f64::INFINITY; 2];
        for p in &regions {
            for axis in 0..2 {
                min[axis] = min[axis].max(p.iter().map(|v| v[axis]).fold(f64::INFINITY, f64::min));
                max[axis] =
                    max[axis].min(p.iter().map(|v| v[axis]).fold(f64::NEG_INFINITY, f64::max));
            }
        }
        let bed = PrintBedDto {
            name: get("printer_model")?
                .as_str()
                .context("printer_model must be a string")?
                .replace("Bambu Lab ", "Bambu "),
            size_mm: [max[0] - min[0], max[1] - min[1], height],
            margin_mm: 0.,
            nozzle_mode: mode,
            origin_mm: min,
            printable_regions: regions,
            excluded_regions: excluded.clone(),
            source: Some(source.clone()),
        };
        bed.validate().map_err(anyhow::Error::msg)?;
        Ok(bed)
    };
    let main = make(
        PrintNozzleMode::Main,
        vec![area.clone(), main_extruder.printable_region.clone()],
        height.min(main_extruder.height_mm),
    )?;
    let mut dual_regions = vec![area];
    dual_regions.extend(extruders.iter().map(|e| e.printable_region.clone()));
    let dual = make(
        PrintNozzleMode::Dual,
        dual_regions,
        extruders.iter().fold(height, |h, e| h.min(e.height_mm)),
    )?;
    Ok(PrinterProfileDto {
        id: id.into(),
        main,
        dual,
        extruders,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inheritance_override_and_cycle_rejection() {
        let mut read = |path: &str| {
            Ok(if path.ends_with("child.json") {
                br#"{"inherits":"parent","printable_height":"261"}"#.to_vec()
            } else {
                br#"{"printable_height":"250","printable_area":["0x0","256x0","0x256"]}"#.to_vec()
            })
        };
        let resolved = resolve(
            "resources/profiles/BBL/machine/child.json",
            &mut BTreeSet::new(),
            &mut BTreeMap::new(),
            &mut read,
        )
        .unwrap();
        assert_eq!(resolved["printable_height"], "261");
        assert!(resolved.contains_key("printable_area"));
        let mut cycle = |_: &str| Ok(br#"{"inherits":"child"}"#.to_vec());
        assert!(resolve(
            "resources/profiles/BBL/machine/child.json",
            &mut BTreeSet::new(),
            &mut BTreeMap::new(),
            &mut cycle
        )
        .is_err());
    }
    fn machine() -> Map<String, Value> {
        serde_json::from_value(serde_json::json!({
            "printer_model":"Bambu Lab X2D",
            "printable_area":["0x0","256x0","256x256","0x256"],
            "printable_height":"261",
            "extruder_printable_area":[
                ["20.5x0","256x0","256x256","20.5x256"],
                ["0x0","256x0","256x256","0x256"]
            ],
            "extruder_printable_height":["256","261"],
            "physical_extruder_map":["left","right"],
            "master_extruder_id":"right",
            "bed_exclude_area":["0x0","10x0","10x10","0x10"]
        }))
        .unwrap()
    }
    fn provenance() -> PrintProfileSource {
        PrintProfileSource {
            repository: "bambulab/BambuStudio".into(),
            revision: "a".repeat(40),
            profile: "resources/profiles/BBL/machine/X2D.json".into(),
            files: BTreeMap::from([(
                "resources/profiles/BBL/machine/X2D.json".into(),
                "b".repeat(64),
            )]),
        }
    }
    #[test]
    fn normalization_keeps_main_mapping_dual_origin_and_source_snapshot() {
        let source = provenance();
        let profile = normalize("x2d", &machine(), source.clone()).unwrap();
        assert_eq!(profile.main.name, "Bambu X2D");
        assert_eq!(profile.main.size_mm, [256., 256., 261.]);
        assert_eq!(profile.main.origin_mm, [0., 0.]);
        assert_eq!(profile.dual.size_mm, [235.5, 256., 256.]);
        assert_eq!(profile.dual.origin_mm, [20.5, 0.]);
        assert_eq!(profile.main.source, Some(source.clone()));
        assert_eq!(profile.dual.source, Some(source));
        assert_eq!(profile.main.excluded_regions.len(), 1);
        assert!(!profile.main.contains_xy_bounds([0., 0.], [5., 5.]));
        assert!(!profile.dual.contains_xy_bounds([0., 0.], [5., 5.]));
        assert!(profile.dual.contains_xy_bounds([240., 0.], [256., 10.]));
    }
    #[test]
    fn normalization_rejects_unknown_master_and_unsupported_geometry() {
        let mut input = machine();
        input.insert("master_extruder_id".into(), Value::String("missing".into()));
        assert!(normalize("x2d", &input, provenance()).is_err());
        let mut input = machine();
        input.insert(
            "printable_area".into(),
            serde_json::json!(["0x0", "256x0", "128x128", "256x256", "0x256"]),
        );
        assert!(normalize("x2d", &input, provenance()).is_err());
        let mut input = machine();
        input.insert(
            "extruder_printable_height".into(),
            serde_json::json!(["256"]),
        );
        assert!(normalize("x2d", &input, provenance()).is_err());
        let mut input = machine();
        input.insert(
            "bed_exclude_area".into(),
            serde_json::json!(["0x0", "10x0", "10x10"]),
        );
        assert!(normalize("x2d", &input, provenance()).is_err());
    }
    #[test]
    fn unsafe_profile_paths_and_inherited_names_fail_before_reading_them() {
        for path in [
            "/resources/profiles/a.json",
            "C:/resources/profiles/a.json",
            "resources/profiles/../a.json",
            "resources/profiles/./a.json",
            "resources/profiles//a.json",
            "resources/profiles/C:a.json",
            "resources/profiles/NUL.json",
            "resources/profiles/CON .json",
            "resources/profiles/a.json:stream",
            "resources/profiles/a%2f.json",
            "resources/profiles/a#.json",
        ] {
            let mut reads = 0;
            let result = resolve(
                path,
                &mut BTreeSet::new(),
                &mut BTreeMap::new(),
                &mut |_| {
                    reads += 1;
                    Ok(b"{}".to_vec())
                },
            );
            assert!(result.is_err(), "Accepted {path}");
            assert_eq!(reads, 0, "Unsafe path reached the cache/network reader");
        }
        for parent in [
            "../outside",
            "/outside",
            "C:outside",
            "\\outside",
            "",
            ".",
            "NUL",
            "parent ",
            "a%2fb",
            "a#b",
        ] {
            let mut reads = 0;
            let result = resolve(
                "resources/profiles/BBL/machine/child.json",
                &mut BTreeSet::new(),
                &mut BTreeMap::new(),
                &mut |_| {
                    reads += 1;
                    Ok(serde_json::to_vec(&serde_json::json!({"inherits":parent})).unwrap())
                },
            );
            assert!(result.is_err(), "Accepted inherited name {parent:?}");
            assert_eq!(
                reads, 1,
                "Unsafe inherited name reached the cache/network reader"
            );
        }
    }
}
