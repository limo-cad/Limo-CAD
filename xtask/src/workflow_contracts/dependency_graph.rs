//! Check crate identities at native boundaries without compiling the desktop.
use toml_edit::{DocumentMut, Table};

fn dependency_version<'a>(
    lock: &'a DocumentMut,
    owner: &str,
    dependency: &str,
) -> Result<&'a str, String> {
    let packages = lock
        .get("package")
        .and_then(toml_edit::Item::as_array_of_tables)
        .ok_or("lockfile has no packages")?;
    let owners: Vec<_> = packages
        .iter()
        .filter(|package| package["name"].as_str() == Some(owner))
        .collect();
    let [owner_package] = owners.as_slice() else {
        return Err(format!("expected one {owner} package"));
    };
    let references: Vec<_> = owner_package
        .get("dependencies")
        .and_then(toml_edit::Item::as_array)
        .into_iter()
        .flatten()
        .filter_map(toml_edit::Value::as_str)
        .filter(|reference| reference.split_whitespace().next() == Some(dependency))
        .collect();
    let [reference] = references.as_slice() else {
        return Err(format!("expected one {owner} -> {dependency} dependency"));
    };
    let version = reference.split_whitespace().nth(1);
    let targets: Vec<_> = packages
        .iter()
        .filter(|package| {
            package["name"].as_str() == Some(dependency)
                && version.is_none_or(|version| package["version"].as_str() == Some(version))
        })
        .collect();
    let [target] = targets.as_slice() else {
        return Err(format!("ambiguous or missing {owner} -> {reference}"));
    };
    target["version"]
        .as_str()
        .ok_or_else(|| format!("{dependency} has no version"))
}

fn same_dependency(
    lock: &DocumentMut,
    first: &str,
    second: &str,
    dependency: &str,
) -> Result<(), String> {
    let first_version = dependency_version(lock, first, dependency)?;
    let second_version = dependency_version(lock, second, dependency)?;
    if first_version != second_version {
        return Err(format!(
            "{dependency} types cross {first} ({first_version}) and {second} ({second_version}); update their shared dependency together"
        ));
    }
    Ok(())
}

fn native_boundaries(lock: &DocumentMut) -> Result<(), String> {
    for owner in [
        "limo-cad",
        "bevy_ui",
        "bevy_winit",
        "bevy_ui_widgets",
        "accesskit_winit",
    ] {
        same_dependency(lock, "bevy_a11y", owner, "accesskit")?;
    }
    for owner in ["bevy_ui", "bevy_ui_widgets"] {
        // Widgets rely on the same Parley instance that Bevy enables std on.
        same_dependency(lock, "bevy_text", owner, "parley")?;
    }
    same_dependency(lock, "bevy_text", "bevy_ui_widgets", "smol_str")?;
    // Multiple Windows versions elsewhere in the graph are valid. These two
    // crates exchange D3D12 device/heap types, so their selected edges must agree.
    same_dependency(lock, "wgpu-hal", "gpu-allocator", "windows")
}

fn desktop_lock() -> DocumentMut {
    super::read("desktop/Cargo.lock").parse().unwrap()
}

#[test]
fn desktop_shared_native_dependencies_agree() {
    native_boundaries(&desktop_lock()).unwrap();
}

fn package_mut<'a>(lock: &'a mut DocumentMut, name: &str) -> &'a mut Table {
    lock["package"]
        .as_array_of_tables_mut()
        .unwrap()
        .iter_mut()
        .find(|package| package["name"].as_str() == Some(name))
        .unwrap()
}

fn replace_edge(lock: &mut DocumentMut, owner: &str, dependency: &str, version: &str) {
    let dependencies = package_mut(lock, owner)["dependencies"]
        .as_array_mut()
        .unwrap();
    let edge = dependencies
        .iter()
        .position(|value| value.as_str().unwrap().split_whitespace().next() == Some(dependency))
        .unwrap();
    dependencies.replace(edge, format!("{dependency} {version}"));
}

fn split_dependency(lock: &mut DocumentMut, owner: &str, dependency: &str, version: &str) {
    let old_version = dependency_version(lock, owner, dependency)
        .unwrap()
        .to_owned();
    let mut added = package_mut(lock, dependency).clone();
    added["version"] = toml_edit::value(version);
    let packages = lock["package"].as_array_of_tables_mut().unwrap();
    for package in packages.iter_mut() {
        if let Some(edges) = package
            .get_mut("dependencies")
            .and_then(toml_edit::Item::as_array_mut)
        {
            for edge in edges.iter_mut() {
                if edge.as_str() == Some(dependency) {
                    *edge = format!("{dependency} {old_version}").into();
                }
            }
        }
    }
    packages.push(added);
    replace_edge(lock, owner, dependency, version);
}

#[test]
fn rejects_accesskit_types_split_from_bevy() {
    let mut lock = desktop_lock();
    split_dependency(&mut lock, "limo-cad", "accesskit", "0.25.1");
    assert!(native_boundaries(&lock)
        .unwrap_err()
        .contains("accesskit types cross"));
}

#[test]
fn rejects_parley_split_from_bevys_feature_provider() {
    let mut lock = desktop_lock();
    split_dependency(&mut lock, "bevy_ui_widgets", "parley", "0.12.0");
    assert!(native_boundaries(&lock)
        .unwrap_err()
        .contains("parley types cross"));
}

#[test]
fn rejects_widget_text_type_split() {
    let mut lock = desktop_lock();
    split_dependency(&mut lock, "bevy_ui_widgets", "smol_str", "0.3.2");
    assert!(native_boundaries(&lock)
        .unwrap_err()
        .contains("smol_str types cross"));
}

#[test]
fn rejects_d3d12_allocator_abi_split() {
    let mut lock = desktop_lock();
    replace_edge(&mut lock, "gpu-allocator", "windows", "0.61.3");
    assert!(native_boundaries(&lock)
        .unwrap_err()
        .contains("windows types cross"));
}
