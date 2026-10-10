//! Normalize upstream engineering cards and print profiles into the existing catalog.
use anyhow::{bail, ensure, Context, Result};
use limo_cad_core::{
    MaterialDetails, MaterialPrintProfile, MaterialProperty, MaterialSource, MaterialValue,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Deserialize)]
struct Manifest {
    sources: Vec<Input>,
    #[serde(default)]
    parents: BTreeMap<String, String>,
}
#[derive(Deserialize)]
struct Input {
    repository: String,
    revision: String,
    path: String,
    format: String,
    kind: String,
    family: String,
    #[serde(default)]
    merge_ids: Vec<String>,
    #[serde(default)]
    new_id: Option<String>,
    #[serde(default)]
    base_path: String,
}
struct Fetcher {
    root: PathBuf,
    fetch: bool,
    seen: BTreeMap<String, (Value, MaterialSource)>,
}
impl Fetcher {
    fn read(&mut self, input: &Input, path: &str) -> Result<(Value, MaterialSource)> {
        ensure!(
            matches!(
                input.repository.as_str(),
                "FreeCAD/FreeCAD" | "OrcaSlicer/OrcaSlicer" | "bambulab/BambuStudio"
            ),
            "Unapproved material repository"
        );
        ensure!(
            input.revision.len() == 40 && input.revision.bytes().all(|c| c.is_ascii_hexdigit()),
            "Pin full material source revisions"
        );
        ensure!(
            !path.contains("..")
                && !path.contains('\\')
                && !path.starts_with('/')
                && !path.chars().any(char::is_control),
            "Unsafe material path"
        );
        let id = format!("{}@{}:{path}", input.repository, input.revision);
        if let Some(v) = self.seen.get(&id) {
            return Ok(v.clone());
        }
        let cached = self
            .root
            .join("target/material-sources")
            .join(&input.repository)
            .join(&input.revision)
            .join(path);
        let bytes = if self.fetch {
            let url = format!(
                "https://raw.githubusercontent.com/{}/{}/{}",
                input.repository,
                input.revision,
                path.replace(' ', "%20").replace('@', "%40")
            );
            let agent: ureq::Agent = ureq::Agent::config_builder()
                .https_only(true)
                .timeout_global(Some(Duration::from_secs(45)))
                .build()
                .into();
            let mut response = agent
                .get(&url)
                .header("User-Agent", "Limo-CAD-material-catalog")
                .call()
                .with_context(|| format!("Fetch {url}"))?;
            let mut bytes = vec![];
            response
                .body_mut()
                .as_reader()
                .take(2_000_001)
                .read_to_end(&mut bytes)?;
            ensure!(
                bytes.len() <= 2_000_000,
                "Material source exceeds size limit"
            );
            bytes
        } else {
            fs::read(&cached)
                .with_context(|| format!("Missing cached source {path}; use --fetch"))?
        };
        let v: Value = if input.format == "freecad" {
            freecad_yaml(&bytes).with_context(|| format!("Invalid FreeCAD YAML: {path}"))?
        } else {
            serde_json::from_slice(&bytes).context("Invalid slicer JSON")?
        };
        let source = MaterialSource {
            id: id.clone(),
            repository: input.repository.clone(),
            revision: input.revision.clone(),
            path: path.into(),
            sha256: Sha256::digest(&bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
            license: if input.format == "freecad" {
                v["General"]["License"]
                    .as_str()
                    .context("Material card needs an explicit license")?
                    .into()
            } else {
                "AGPL-3.0".into()
            },
            author: if input.format == "freecad" {
                v["General"]["Author"]
                    .as_str()
                    .context("Material card needs attribution")?
                    .into()
            } else {
                format!("{} contributors", input.repository)
            },
            reference: v["General"]["SourceURL"].as_str().unwrap_or("").into(),
        };
        if self.fetch {
            fs::create_dir_all(cached.parent().unwrap())?;
            fs::write(cached, bytes)?;
        }
        self.seen.insert(id, (v.clone(), source.clone()));
        Ok((v, source))
    }
}

pub fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut fetch = false;
    let mut check = false;
    for arg in args {
        match arg.as_str() {
            "--fetch" => fetch = true,
            "--check" => check = true,
            "--help" => {
                println!("cargo xtask materials [--fetch] [--check]\nFetch pinned material sources and merge into crates/export/presets/catalog.json.\n--check verifies without writing. Without --fetch, use target/material-sources.\nLegacy color/product entries remain in the same catalog; no second runtime catalog.");
                return Ok(());
            }
            _ => bail!("Unknown materials option {arg}"),
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let manifest: Manifest =
        serde_json::from_slice(&fs::read(root.join("crates/export/presets/sources.json"))?)?;
    let destination = root.join("crates/export/presets/catalog.json");
    let existing: Vec<Value> = serde_json::from_slice(&fs::read(&destination)?)?;
    let mut catalog: Vec<Value> = existing
        .iter()
        .filter(|v| !v["id"].as_str().unwrap_or("").starts_with("material."))
        .cloned()
        .collect();
    for row in &mut catalog {
        row.as_object_mut()
            .context("Catalog entry must be an object")?
            .remove("material");
    }
    let mut fetcher = Fetcher {
        root: root.into(),
        fetch,
        seen: BTreeMap::new(),
    };
    for input in &manifest.sources {
        ensure!(
            matches!(input.kind.as_str(), "plastic" | "metal"),
            "Unknown material kind"
        );
        let mut sources = BTreeMap::new();
        let mut active = BTreeSet::new();
        let (label, properties, print_profile) = match input.format.as_str() {
            "freecad" => {
                let (label, p) = freecad(
                    input,
                    &input.path,
                    &manifest.parents,
                    &mut fetcher,
                    &mut sources,
                    &mut active,
                )?;
                (label, p, None)
            }
            "slicer" => {
                let resolved = slicer(input, &input.path, &mut fetcher, &mut sources, &mut active)?;
                let source = fetcher.read(input, &input.path)?.1;
                let label = resolved["name"]
                    .as_str()
                    .context("Slicer profile needs name")?
                    .to_string();
                let context = format!("Print profile: {label}");
                let mut p = vec![];
                for (name, (value, id)) in &resolved.properties {
                    let values = values(value);
                    for (index, v) in values.iter().enumerate() {
                        let (name, value, unit) = slicer_quantity(name, v)?;
                        let name = if values.len() > 1 {
                            format!("{name} [{}]", index + 1)
                        } else {
                            name
                        };
                        p.push(MaterialProperty {
                            name,
                            value,
                            unit,
                            context: context.clone(),
                            source_id: id.clone(),
                        });
                    }
                }
                let compatible_printers = resolved["compatible_printers"]
                    .as_array()
                    .map(|v| {
                        v.iter()
                            .filter_map(Value::as_str)
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default();
                let print_profile = MaterialPrintProfile {
                    name: label.clone(),
                    source_id: source.id,
                    compatible_printers,
                };
                (label, p, Some(print_profile))
            }
            _ => bail!("Unknown source format"),
        };
        let mut ids = input.merge_ids.clone();
        if let Some(id) = &input.new_id {
            ensure!(
                id.starts_with("material."),
                "Imported IDs must use material. prefix"
            );
            ensure!(
                !catalog.iter().any(|v| v["id"] == *id),
                "Duplicate imported material ID {id}"
            );
            catalog.push(json!({"id":id,"brand":"Generic","filament_type":input.family,"material_name":label,"color_name":"Natural","r":180,"g":180,"b":180,"filament_id":null,"density_g_cm3":null,"diameter_mm":1.75}));
            ids.push(id.clone());
        }
        ensure!(!ids.is_empty(), "Source needs explicit material mappings");
        for id in ids {
            let row = catalog
                .iter_mut()
                .find(|v| v["id"] == id)
                .with_context(|| format!("Unknown material mapping {id}"))?;
            let mut material: MaterialDetails = row
                .get("material")
                .map(|v| serde_json::from_value(v.clone()))
                .transpose()?
                .unwrap_or(MaterialDetails {
                    kind: input.kind.clone(),
                    catalog_id: id,
                    ..Default::default()
                });
            ensure!(
                material.kind == input.kind,
                "Cannot merge metal and plastic records"
            );
            material.properties.extend(properties.clone());
            for source in sources.values() {
                if !material.sources.iter().any(|s| s.id == source.id) {
                    material.sources.push(source.clone());
                }
            }
            if let Some(profile) = &print_profile {
                material.print_profiles.push(profile.clone());
            }
            row["material"] = serde_json::to_value(material)?;
        }
    }
    for row in &mut catalog {
        let id = row["id"].as_str().context("Catalog needs id")?.to_string();
        let mut material: MaterialDetails = row
            .get("material")
            .map(|v| serde_json::from_value(v.clone()))
            .transpose()?
            .unwrap_or(MaterialDetails {
                kind: "plastic".into(),
                catalog_id: id,
                ..Default::default()
            });
        material.properties.sort_by(|a, b| {
            (&a.name, &a.context, &a.source_id).cmp(&(&b.name, &b.context, &b.source_id))
        });
        material.sources.sort_by(|a, b| a.id.cmp(&b.id));
        material.warnings = assess(&material);
        material.validate().map_err(anyhow::Error::msg)?;
        let density = material
            .properties
            .iter()
            .filter(|p| p.name == "Density" && p.unit == "kg/m^3")
            .find(|p| p.context.starts_with("Print profile:"))
            .or_else(|| {
                material
                    .properties
                    .iter()
                    .find(|p| p.name == "Density" && p.unit == "kg/m^3")
            });
        if let Some(MaterialProperty {
            value: MaterialValue::Number(v),
            ..
        }) = density
        {
            row["density_g_cm3"] = json!(v / 1000.);
        }
        row["material"] = serde_json::to_value(material)?;
    }
    let mut generated = serde_json::to_string_pretty(&catalog)?;
    generated.push('\n');
    if check {
        ensure!(serde_json::from_slice::<Value>(&fs::read(&destination)?)?==serde_json::from_str::<Value>(&generated)?,"Unified material catalog differs from pinned sources; run cargo xtask materials --fetch and review the diff");
    } else {
        fs::write(destination, generated)?;
    }
    println!(
        "{} unified materials verified from {} pinned source files",
        catalog.len(),
        fetcher.seen.len()
    );
    Ok(())
}

struct Resolved {
    fields: BTreeMap<String, Value>,
    properties: BTreeMap<String, (Value, String)>,
}
impl std::ops::Index<&str> for Resolved {
    type Output = Value;
    fn index(&self, k: &str) -> &Value {
        self.fields.get(k).unwrap_or(&Value::Null)
    }
}
fn slicer(
    input: &Input,
    path: &str,
    f: &mut Fetcher,
    sources: &mut BTreeMap<String, MaterialSource>,
    active: &mut BTreeSet<String>,
) -> Result<Resolved> {
    ensure!(
        active.len() < 32 && active.insert(path.into()),
        "Slicer inheritance cycle"
    );
    let (v, source) = f.read(input, path)?;
    sources.insert(source.id.clone(), source.clone());
    let mut resolved = if let Some(parent) = v.get("inherits") {
        let parent = parent.as_str().context("inherits must be a string")?;
        ensure!(
            !parent.contains('/') && !parent.contains('\\') && !parent.contains(".."),
            "Unsafe inherited name"
        );
        slicer(
            input,
            &format!("{}/{}.json", input.base_path, parent),
            f,
            sources,
            active,
        )?
    } else {
        Resolved {
            fields: BTreeMap::new(),
            properties: BTreeMap::new(),
        }
    };
    for (key, value) in v.as_object().context("Slicer profile must be an object")? {
        resolved.fields.insert(key.clone(), value.clone());
        if ![
            "type",
            "name",
            "inherits",
            "from",
            "instantiation",
            "setting_id",
            "filament_id",
            "renamed_from",
            "compatible_printers",
            "compatible_printers_condition",
            "include",
        ]
        .contains(&key.as_str())
            && !key.contains("gcode")
        {
            resolved
                .properties
                .insert(key.clone(), (value.clone(), source.id.clone()));
        }
    }
    ensure!(
        v.get("include").is_none(),
        "Slicer profile includes need an explicit adapter"
    );
    active.remove(path);
    Ok(resolved)
}
fn freecad(
    input: &Input,
    path: &str,
    parents: &BTreeMap<String, String>,
    f: &mut Fetcher,
    sources: &mut BTreeMap<String, MaterialSource>,
    active: &mut BTreeSet<String>,
) -> Result<(String, Vec<MaterialProperty>)> {
    ensure!(
        active.len() < 32 && active.insert(path.into()),
        "FreeCAD inheritance cycle"
    );
    let (v, source) = f.read(input, path)?;
    sources.insert(source.id.clone(), source.clone());
    let name = v["General"]["Name"]
        .as_str()
        .context("Material card needs name")?
        .to_string();
    let mut props = BTreeMap::<String, MaterialProperty>::new();
    if let Some(inherits) = v["Inherits"].as_object() {
        for parent in inherits.values() {
            let uuid = parent["UUID"].as_str().context("Parent needs UUID")?;
            let path = parents
                .get(uuid)
                .with_context(|| format!("Unknown material parent UUID {uuid}"))?;
            let (_, p) = freecad(input, path, parents, f, sources, active)?;
            for property in p {
                props.insert(property.name.clone(), property);
            }
        }
    }
    for section in ["Models", "AppearanceModels"] {
        if let Some(models) = v[section].as_object() {
            for fields in models.values() {
                for (key, value) in fields
                    .as_object()
                    .context("Model fields must be an object")?
                {
                    if key == "UUID" {
                        continue;
                    }
                    let text = scalar(value);
                    let (value, unit) = quantity(&text)?;
                    props.insert(
                        key.clone(),
                        MaterialProperty {
                            name: key.clone(),
                            value,
                            unit,
                            context: format!("Engineering reference: {name}"),
                            source_id: source.id.clone(),
                        },
                    );
                }
            }
        }
    }
    if let Some(description) = v["General"]["Description"].as_str() {
        props.insert(
            "Description".into(),
            MaterialProperty {
                name: "Description".into(),
                value: MaterialValue::Text(description.into()),
                unit: String::new(),
                context: format!("Engineering reference: {name}"),
                source_id: source.id,
            },
        );
    }
    active.remove(path);
    Ok((name, props.into_values().collect()))
}
fn values(v: &Value) -> &[Value] {
    v.as_array()
        .map(Vec::as_slice)
        .unwrap_or_else(|| std::slice::from_ref(v))
}
fn scalar(v: &Value) -> String {
    v.as_str()
        .map(str::to_string)
        .unwrap_or_else(|| v.to_string())
}
fn quantity(text: &str) -> Result<(MaterialValue, String)> {
    let text = text.trim();
    let split = text.find(char::is_whitespace).unwrap_or(text.len());
    let number = text[..split].replace(',', ".");
    if let Ok(n) = number.parse::<f64>() {
        ensure!(n.is_finite(), "Nonfinite material property");
        let unit = text[split..].trim();
        let (factor, unit) = match unit {
            "MPa" => (1e6, "Pa"),
            "GPa" => (1e9, "Pa"),
            "g/cm^3" | "g/cm³" => (1000., "kg/m^3"),
            "µm/m/K" => (1e-6, "1/K"),
            "m/m/K" => (1., "1/K"),
            _ => (1., unit),
        };
        ensure!(
            (n * factor).is_finite(),
            "Material unit conversion overflow"
        );
        Ok((MaterialValue::Number(n * factor), unit.into()))
    } else {
        ensure!(text.len() <= 4096, "Material value too long");
        Ok((MaterialValue::Text(text.into()), String::new()))
    }
}
fn freecad_yaml(bytes: &[u8]) -> Result<Value> {
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    let mut value = None;
    for document in serde_yaml_ng::Deserializer::from_slice(bytes) {
        let parsed = Value::deserialize(document)?;
        if parsed.is_null() {
            continue;
        }
        ensure!(
            value.is_none(),
            "Expected one material card per source file"
        );
        value = Some(parsed);
    }
    value.context("Empty FreeCAD material card")
}
fn slicer_quantity(name: &str, v: &Value) -> Result<(String, MaterialValue, String)> {
    let text = scalar(v);
    let mut value = if let Ok(number) = text.trim().parse::<f64>() {
        ensure!(number.is_finite(), "Nonfinite slicer property");
        MaterialValue::Number(number)
    } else {
        ensure!(text.len() <= 4096, "Slicer value too long");
        MaterialValue::Text(text)
    };
    let mut unit = String::new();
    let name: String = if name == "filament_density" {
        if let MaterialValue::Number(n) = &mut value {
            *n *= 1000.;
            ensure!(*n > 0. && n.is_finite(), "Invalid filament density");
        } else {
            bail!("Filament density must be numeric");
        }
        unit = "kg/m^3".into();
        "Density".into()
    } else {
        name.into()
    };
    if matches!(value, MaterialValue::Number(_))
        && (name.contains("temperature")
            || name.ends_with("plate_temp")
            || name.ends_with("plate_temp_initial_layer")
            || matches!(
                name.as_str(),
                "filament_flush_temp"
                    | "filament_flush_temp_fast"
                    | "filament_tower_interface_print_temp"
            ))
    {
        unit = "degC".into();
    }
    if name == "filament_diameter" && matches!(value, MaterialValue::Number(_)) {
        unit = "mm".into();
    }
    Ok((name, value, unit))
}
fn assess(material: &MaterialDetails) -> Vec<String> {
    let mut warnings = vec![];
    if material.sources.is_empty() {
        warnings.push("Existing curated product metadata; physical properties have not been verified against an upstream source.".into());
    }
    if material.kind == "plastic"
        && material
            .properties
            .iter()
            .any(|p| p.context.starts_with("Engineering reference:"))
    {
        warnings.push("Generic engineering properties are reference values, not measured strength of your printed part. Processing and sample conditions may differ.".into());
    }
    for p in &material.properties {
        if p.name == "YieldStrength" && p.unit == "Pa" {
            if let MaterialValue::Number(y) = p.value {
                if material.properties.iter().any(|u| {
                    u.name == "UltimateTensileStrength"
                        && u.source_id == p.source_id
                        && u.unit == "Pa"
                        && matches!(u.value,MaterialValue::Number(t) if y>t)
                }) {
                    warnings.push(format!("{} lists yield strength above ultimate tensile strength; verify the source before engineering use.",p.context));
                }
            }
        }
    }
    warnings.sort();
    warnings.dedup();
    warnings
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn freecad_inheritance_preserves_parent_provenance_and_child_overrides() {
        let root = tempfile::tempdir().unwrap();
        let input = Input {
            repository: "FreeCAD/FreeCAD".into(),
            revision: "1".repeat(40),
            path: "cards/child.FCMat".into(),
            format: "freecad".into(),
            kind: "metal".into(),
            family: "Aluminum".into(),
            merge_ids: vec![],
            new_id: None,
            base_path: String::new(),
        };
        let cache = root
            .path()
            .join("target/material-sources")
            .join(&input.repository)
            .join(&input.revision)
            .join("cards");
        fs::create_dir_all(&cache).unwrap();
        let parent = "General:\n  Name: Aluminum\n  License: CC-BY-4.0\n  Author: Parent author\nModels:\n  Physical:\n    Density: 2700 kg/m^3\n    YoungsModulus: 70 GPa\n";
        let child = "General:\n  Name: Aluminum 6061\n  License: CC-BY-4.0\n  Author: Child author\nInherits:\n  Parent:\n    UUID: parent-uuid\nModels:\n  Mechanical:\n    YoungsModulus: 68.9 GPa\n";
        fs::write(cache.join("parent.FCMat"), parent).unwrap();
        fs::write(cache.join("child.FCMat"), child).unwrap();
        let mut fetcher = Fetcher {
            root: root.path().into(),
            fetch: false,
            seen: BTreeMap::new(),
        };
        let parents = BTreeMap::from([("parent-uuid".into(), "cards/parent.FCMat".into())]);
        let mut sources = BTreeMap::new();
        let (label, properties) = freecad(
            &input,
            &input.path,
            &parents,
            &mut fetcher,
            &mut sources,
            &mut BTreeSet::new(),
        )
        .unwrap();
        assert_eq!(label, "Aluminum 6061");
        assert_eq!(sources.len(), 2);
        let density = properties.iter().find(|p| p.name == "Density").unwrap();
        assert_eq!(density.value, MaterialValue::Number(2700.));
        assert_eq!(density.context, "Engineering reference: Aluminum");
        assert!(density.source_id.ends_with("parent.FCMat"));
        let modulus = properties
            .iter()
            .find(|p| p.name == "YoungsModulus")
            .unwrap();
        assert_eq!(modulus.value, MaterialValue::Number(68.9e9));
        assert_eq!(modulus.unit, "Pa");
        assert_eq!(modulus.context, "Engineering reference: Aluminum 6061");
        assert!(modulus.source_id.ends_with("child.FCMat"));
        assert!(freecad(
            &input,
            &input.path,
            &BTreeMap::new(),
            &mut fetcher,
            &mut BTreeMap::new(),
            &mut BTreeSet::new()
        )
        .unwrap_err()
        .to_string()
        .contains("Unknown material parent"));
        fs::write(
            cache.join("parent.FCMat"),
            format!("{parent}Inherits:\n  Child:\n    UUID: child-uuid\n"),
        )
        .unwrap();
        fetcher.seen.clear();
        let mut cyclic = parents;
        cyclic.insert("child-uuid".into(), input.path.clone());
        assert!(freecad(
            &input,
            &input.path,
            &cyclic,
            &mut fetcher,
            &mut BTreeMap::new(),
            &mut BTreeSet::new()
        )
        .unwrap_err()
        .to_string()
        .contains("cycle"));
    }
    #[test]
    fn slicer_inheritance_retains_defining_sources_and_rejects_cycles() {
        let root = tempfile::tempdir().unwrap();
        let input = Input {
            repository: "OrcaSlicer/OrcaSlicer".into(),
            revision: "1".repeat(40),
            path: "profiles/child.json".into(),
            format: "slicer".into(),
            kind: "plastic".into(),
            family: "PLA".into(),
            merge_ids: vec![],
            new_id: None,
            base_path: "profiles".into(),
        };
        let cache = root
            .path()
            .join("target/material-sources")
            .join(&input.repository)
            .join(&input.revision)
            .join("profiles");
        fs::create_dir_all(&cache).unwrap();
        fs::write(cache.join("parent.json"),json!({"name":"Parent","filament_density":["1.20"],"nozzle_temperature":["210"],"filament_start_gcode":["DO NOT EMBED"]}).to_string()).unwrap();
        fs::write(
            cache.join("child.json"),
            json!({"name":"Child","inherits":"parent","filament_density":["1.24"]}).to_string(),
        )
        .unwrap();
        let mut fetcher = Fetcher {
            root: root.path().into(),
            fetch: false,
            seen: BTreeMap::new(),
        };
        let mut sources = BTreeMap::new();
        let resolved = slicer(
            &input,
            &input.path,
            &mut fetcher,
            &mut sources,
            &mut BTreeSet::new(),
        )
        .unwrap();
        assert!(resolved.properties["filament_density"]
            .1
            .ends_with("child.json"));
        assert!(resolved.properties["nozzle_temperature"]
            .1
            .ends_with("parent.json"));
        assert_eq!(resolved.properties["filament_density"].0, json!(["1.24"]));
        assert!(!resolved.properties.contains_key("filament_start_gcode"));
        assert_eq!(sources.len(), 2);
        fs::write(
            cache.join("parent.json"),
            json!({"name":"Parent","inherits":"child"}).to_string(),
        )
        .unwrap();
        fetcher.seen.clear();
        assert!(slicer(
            &input,
            &input.path,
            &mut fetcher,
            &mut sources,
            &mut BTreeSet::new()
        )
        .err()
        .unwrap()
        .to_string()
        .contains("cycle"));
    }
    #[test]
    fn preserves_context_and_converts_units_without_averaging() {
        assert_eq!(
            quantity("68.9 GPa").unwrap(),
            (MaterialValue::Number(68.9e9), "Pa".into())
        );
        assert_eq!(
            quantity("2700 kg/m^3").unwrap(),
            (MaterialValue::Number(2700.), "kg/m^3".into())
        );
        assert_eq!(
            slicer_quantity("filament_density", &json!("1.24")).unwrap(),
            (
                "Density".into(),
                MaterialValue::Number(1240.),
                "kg/m^3".into()
            )
        );
        assert!(quantity("NaN MPa").is_err());
        assert_eq!(
            slicer_quantity("volumetric_speed_coefficients", &json!("0 0 0 0 0 0")).unwrap(),
            (
                "volumetric_speed_coefficients".into(),
                MaterialValue::Text("0 0 0 0 0 0".into()),
                String::new()
            )
        );
        assert_eq!(
            quantity("2200,00 MPa").unwrap(),
            (MaterialValue::Number(2.2e9), "Pa".into())
        );
        assert_eq!(
            quantity("68,00 µm/m/K").unwrap(),
            (MaterialValue::Number(0.000068), "1/K".into())
        );
    }
    #[test]
    fn slicer_abbreviated_temperatures_retain_values_and_sentinels() {
        for name in [
            "filament_flush_temp",
            "filament_flush_temp_fast",
            "filament_tower_interface_print_temp",
        ] {
            for value in ["220", "0", "-1"] {
                assert_eq!(
                    slicer_quantity(name, &json!(value)).unwrap(),
                    (
                        name.into(),
                        MaterialValue::Number(value.parse().unwrap()),
                        "degC".into()
                    )
                );
            }
        }
        for name in [
            "filament_flow_ratio",
            "filament_max_volumetric_speed",
            "filament_flush_volumetric_speed",
        ] {
            assert_eq!(
                slicer_quantity(name, &json!("1.0")).unwrap(),
                (name.into(), MaterialValue::Number(1.), String::new())
            );
        }
    }
    #[test]
    fn freecad_accepts_bom_and_empty_documents_but_rejects_multiple_cards() {
        let bytes = b"\xef\xbb\xbf---\nGeneral:\n  Name: PC\n---\n";
        assert_eq!(freecad_yaml(bytes).unwrap()["General"]["Name"], "PC");
        assert!(freecad_yaml(b"---\nGeneral: {}\n---\nModels: {}\n").is_err());
    }
    #[test]
    fn mixed_strength_card_is_flagged() {
        let property = |name: &str, value| MaterialProperty {
            name: name.into(),
            value: MaterialValue::Number(value),
            unit: "Pa".into(),
            context: "Engineering reference: ABS".into(),
            source_id: "card".into(),
        };
        let material = MaterialDetails {
            kind: "plastic".into(),
            properties: vec![
                property("YieldStrength", 44.1e6),
                property("UltimateTensileStrength", 38.8e6),
            ],
            ..Default::default()
        };
        assert!(assess(&material)
            .iter()
            .any(|w| w.contains("above ultimate")));
    }
}
