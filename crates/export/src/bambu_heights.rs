//! Native object-level height metadata with bounded, typed refresh baselines.

use super::*;
use limo_cad_core::{BambuHeightRangeSnapshotDto, PrintHeightSpeedsDto, PrintLayerHeightPointDto};

const RANGES: &str = "Metadata/layer_config_ranges.xml";
const VARIABLE: &str = "Metadata/layer_heights_profile.txt";

fn number(value: &str, label: &str) -> Result<f64, ExportError> {
    let value: f64 = value.trim().parse().map_err(err)?;
    if !value.is_finite() {
        return Err(err(format!("Native height '{label}' must be finite")));
    }
    Ok(value)
}

fn range_settings(
    options: &BTreeMap<String, String>,
    entries: &BTreeMap<String, Vec<u8>>,
) -> Result<(PrintSettingsDto, PrintHeightSpeedsDto), ExportError> {
    let count = |key: &str| {
        options
            .get(key)
            .map(|value| value.parse::<u32>().map_err(err))
            .transpose()
    };
    let density = options
        .get("sparse_infill_density")
        .map(|value| {
            number(
                value
                    .strip_suffix('%')
                    .ok_or_else(|| err("Native height density must use percent units"))?,
                "density",
            )
        })
        .transpose()?;
    let pattern = options
        .get("sparse_infill_pattern")
        .map(|value| match value.as_str() {
            "grid" => Ok(InfillPatternDto::Grid),
            "gyroid" => Ok(InfillPatternDto::Gyroid),
            "zig-zag" => Ok(InfillPatternDto::Rectilinear),
            "concentric" => Ok(InfillPatternDto::Concentric),
            "cubic" => Ok(InfillPatternDto::Cubic),
            "honeycomb" => Ok(InfillPatternDto::Honeycomb),
            "lightning" => Ok(InfillPatternDto::Lightning),
            _ => fail("Existing native height pattern is outside the typed adapter capability"),
        })
        .transpose()?;
    let settings = PrintSettingsDto {
        wall_count: count("wall_loops")?,
        infill_density_percent: density,
        infill_pattern: pattern,
        top_shell_layers: count("top_shell_layers")?,
        bottom_shell_layers: count("bottom_shell_layers")?,
    };
    settings.validate().map_err(ExportError)?;
    let speed = |key: &str| {
        options.get(key).map(|value| {
        let values=value.split(',').map(|value|number(value,key)).collect::<Result<Vec<_>,_>>()?;
        let profile:Value=serde_json::from_slice(entries.get(PROFILE).ok_or_else(||err("Native height speeds need their complete profile"))?).map_err(err)?;
        if values.len()!=profile_strings(&profile,key)?.len() {
            return fail("Existing native height speed addresses only some extruder variants; review its scope before refresh");
        }
        if values.is_empty()||values.len()>64||values.iter().any(|value|(*value-values[0]).abs()>1e-6) {
            return fail("Existing native height speed has distinct extruder-variant values; review it rather than collapsing its scope");
        }
        Ok(values[0])
    }).transpose()
    };
    let speeds = PrintHeightSpeedsDto {
        outer_wall_mm_s: speed("outer_wall_speed")?,
        inner_wall_mm_s: speed("inner_wall_speed")?,
        infill_mm_s: speed("sparse_infill_speed")?,
    };
    speeds.validate().map_err(ExportError)?;
    Ok((settings, speeds))
}

fn read_ranges(
    entries: &BTreeMap<String, Vec<u8>>,
    ordinal: u32,
) -> Result<Vec<BambuHeightRangeSnapshotDto>, ExportError> {
    if !entries.contains_key(RANGES) {
        return Ok(Vec::new());
    }
    let document = xml(text(entries, RANGES)?)?;
    if !document.root_element().has_tag_name("objects") {
        return fail("Native height range root must be objects");
    }
    let objects: Vec<_> = document
        .root_element()
        .children()
        .filter(|node| {
            node.is_element()
                && node.has_tag_name("object")
                && node.attribute("id").and_then(|id| id.parse::<u32>().ok()) == Some(ordinal)
        })
        .collect();
    if objects.len() > 1 {
        return fail("Duplicate native height object ordinal is ambiguous");
    }
    let Some(object) = objects.first() else {
        return Ok(Vec::new());
    };
    let mut ranges = Vec::new();
    for range in object.children().filter(|node| node.is_element()) {
        if !range.has_tag_name("range") {
            return fail("Existing native height object contains unsupported metadata; review it before replacement");
        }
        let min_z_mm = number(
            range
                .attribute("min_z")
                .ok_or_else(|| err("Native height range omitted min_z"))?,
            "min_z",
        )?;
        let max_z_mm = number(
            range
                .attribute("max_z")
                .ok_or_else(|| err("Native height range omitted max_z"))?,
            "max_z",
        )?;
        if min_z_mm < 0. || min_z_mm >= max_z_mm || max_z_mm > 1e6 {
            return fail("Existing native height range requires finite nonnegative ordered bounds");
        }
        let mut options = BTreeMap::new();
        for option in range.children().filter(|node| node.is_element()) {
            if !option.has_tag_name("option") {
                return fail("Unsupported native height range node");
            }
            let key = option
                .attribute("opt_key")
                .ok_or_else(|| err("Native height option omitted opt_key"))?;
            if !SETTING_KEYS.contains(&key)
                && ![
                    "layer_height",
                    "outer_wall_speed",
                    "inner_wall_speed",
                    "sparse_infill_speed",
                ]
                .contains(&key)
            {
                return Err(err(format!(
                    "Existing height key '{key}' is unsupported; it will not be silently erased"
                )));
            }
            let value = option.text().unwrap_or_default().trim();
            let max_value_bytes = if [
                "outer_wall_speed",
                "inner_wall_speed",
                "sparse_infill_speed",
            ]
            .contains(&key)
            {
                32768
            } else {
                128
            };
            if value.len() > max_value_bytes
                || options.insert(key.to_string(), value.to_string()).is_some()
            {
                return fail("Native height option is excessive or duplicated");
            }
        }
        let layer_height_mm = number(
            options.get("layer_height").ok_or_else(|| {
                err("Native settings-only height range requires its unchanged layer_height")
            })?,
            "layer_height",
        )?;
        if layer_height_mm <= 0. || layer_height_mm > 1e6 {
            return fail("Existing native range layer height must be positive and bounded");
        }
        let (settings, speeds) = range_settings(&options, entries)?;
        ranges.push(BambuHeightRangeSnapshotDto {
            min_z_mm,
            max_z_mm,
            layer_height_mm,
            settings,
            speeds,
        });
        if ranges.len() > 256 {
            return fail("Native height object has more than 256 intervals");
        }
    }
    ranges.sort_by(|a, b| {
        a.min_z_mm
            .total_cmp(&b.min_z_mm)
            .then(a.max_z_mm.total_cmp(&b.max_z_mm))
    });
    Ok(ranges)
}

fn read_profiles(
    entries: &BTreeMap<String, Vec<u8>>,
) -> Result<BTreeMap<u32, Vec<PrintLayerHeightPointDto>>, ExportError> {
    let mut profiles = BTreeMap::new();
    if !entries.contains_key(VARIABLE) {
        return Ok(profiles);
    }
    for line in text(entries, VARIABLE)?
        .lines()
        .filter(|line| !line.trim().is_empty())
    {
        let (identity, values) = line
            .trim()
            .split_once('|')
            .ok_or_else(|| err("Malformed native variable layer profile"))?;
        let ordinal: u32 = identity
            .strip_prefix("object_id=")
            .ok_or_else(|| err("Native variable profile omitted object_id"))?
            .parse()
            .map_err(err)?;
        if ordinal == 0 || profiles.contains_key(&ordinal) || profiles.len() >= 4096 {
            return fail("Native variable object ordinals must be positive, bounded and unique");
        }
        let values = values.strip_suffix(';').unwrap_or(values);
        if values.split(';').any(str::is_empty) {
            return fail("Native variable profile contains an empty sample");
        }
        let values = values
            .split(';')
            .map(|value| number(value, "variable sample"))
            .collect::<Result<Vec<_>, _>>()?;
        if values.len() % 2 != 0 || values.is_empty() || values.len() > 8192 {
            return fail("Native variable profile requires bounded Z/height pairs");
        }
        let mut points = Vec::new();
        let mut previous = -1.;
        for pair in values.as_chunks::<2>().0 {
            if pair[0] <= previous
                || pair[0] < 0.
                || pair[1] <= 0.
                || pair[0] > 1e6
                || pair[1] > 1e6
            {
                return fail(
                    "Native variable samples require increasing bounded Z and positive heights",
                );
            }
            points.push(PrintLayerHeightPointDto {
                z_mm: pair[0],
                height_mm: pair[1],
            });
            previous = pair[0];
        }
        profiles.insert(ordinal, points);
    }
    Ok(profiles)
}

fn range_xml(
    ordinal: u32,
    ranges: &[BambuHeightRangeSnapshotDto],
    speed_counts: [usize; 3],
) -> String {
    if ranges.is_empty() {
        return String::new();
    }
    let mut output = format!("<object id=\"{ordinal}\">");
    for range in ranges {
        output.push_str(&format!(
            "<range min_z=\"{}\" max_z=\"{}\">",
            range.min_z_mm, range.max_z_mm
        ));
        let mut options = settings_map(&range.settings);
        options.insert("layer_height".into(), range.layer_height_mm.to_string());
        for (index, (key, value)) in [
            ("outer_wall_speed", range.speeds.outer_wall_mm_s),
            ("inner_wall_speed", range.speeds.inner_wall_mm_s),
            ("sparse_infill_speed", range.speeds.infill_mm_s),
        ]
        .into_iter()
        .enumerate()
        {
            if let Some(value) = value {
                options.insert(
                    key.into(),
                    vec![value.to_string(); speed_counts[index]].join(","),
                );
            }
        }
        for (key, value) in options {
            output.push_str(&format!(
                "<option opt_key=\"{}\">{}</option>",
                escape(&key),
                escape(&value)
            ));
        }
        output.push_str("</range>");
    }
    output.push_str("</object>");
    output
}

fn write_ranges(
    entries: &mut BTreeMap<String, Vec<u8>>,
    ordinal: u32,
    ranges: &[BambuHeightRangeSnapshotDto],
) -> Result<(), ExportError> {
    let mut speed_counts = [1; 3];
    for (index, key) in [
        "outer_wall_speed",
        "inner_wall_speed",
        "sparse_infill_speed",
    ]
    .into_iter()
    .enumerate()
    {
        let requested = ranges.iter().any(|range| match index {
            0 => range.speeds.outer_wall_mm_s.is_some(),
            1 => range.speeds.inner_wall_mm_s.is_some(),
            _ => range.speeds.infill_mm_s.is_some(),
        });
        if requested {
            let profile: Value = serde_json::from_slice(
                entries
                    .get(PROFILE)
                    .ok_or_else(|| err("Height speeds need a complete native profile"))?,
            )
            .map_err(err)?;
            // Native coFloats index nozzle/extruder variants, beyond the physical nozzle count.
            // Fill the existing vector so a selected later variant cannot inherit a hidden default.
            speed_counts[index] = profile_strings(&profile, key)?.len();
        }
    }
    let replacement = range_xml(ordinal, ranges, speed_counts);
    let Some(source) = entries
        .get(RANGES)
        .map(|bytes| std::str::from_utf8(bytes).map(str::to_owned))
        .transpose()
        .map_err(err)?
    else {
        if !ranges.is_empty() {
            entries.insert(
                RANGES.into(),
                format!("<objects>{replacement}</objects>").into_bytes(),
            );
        }
        return Ok(());
    };
    let document = xml(&source)?;
    let objects: Vec<_> = document
        .root_element()
        .children()
        .filter(|node| {
            node.has_tag_name("object")
                && node.attribute("id").and_then(|id| id.parse::<u32>().ok()) == Some(ordinal)
        })
        .collect();
    if objects.len() > 1 {
        return fail("Duplicate native height object ordinal");
    }
    let patch = if let Some(object) = objects.first() {
        (object.range(), replacement)
    } else if document.root_element().range().end >= 2
        && source[document.root_element().range()]
            .trim_end()
            .ends_with("/>")
    {
        let root = document.root_element();
        let content = &source[root.range()];
        let end = content
            .rfind("/>")
            .ok_or_else(|| err("Malformed empty native height root"))?;
        (
            root.range(),
            format!(
                "{}>{replacement}</{}>",
                &content[..end],
                root.tag_name().name()
            ),
        )
    } else {
        let offset = close_position(&source, document.root_element())?;
        (offset..offset, replacement)
    };
    entries.insert(
        RANGES.into(),
        apply_edits(&source, vec![patch])?.into_bytes(),
    );
    Ok(())
}

fn write_profiles(
    entries: &mut BTreeMap<String, Vec<u8>>,
    profiles: &BTreeMap<u32, Vec<PrintLayerHeightPointDto>>,
) {
    if profiles.is_empty() {
        entries.remove(VARIABLE);
        return;
    }
    let mut output = String::new();
    for (ordinal, points) in profiles {
        output.push_str(&format!("object_id={ordinal}|"));
        for (index, point) in points.iter().enumerate() {
            if index != 0 {
                output.push(';');
            }
            output.push_str(&format!("{};{}", point.z_mm, point.height_mm));
        }
        output.push('\n');
    }
    entries.insert(VARIABLE.into(), output.into_bytes());
}

fn reference_object(
    template: &Template,
    reference: &BambuRefreshReference,
    record: &limo_cad_core::BambuRefreshHeightObjectDto,
) -> Result<(u32, u32), ExportError> {
    let mut objects = BTreeSet::new();
    for source in &record.source_bindings {
        let part = reference
            .parts
            .iter()
            .find(|part| {
                part.binding.body_id == source.body_id
                    && part.binding.occurrence_id == source.occurrence_id
            })
            .ok_or_else(|| err("Height refresh source is missing its normal-volume identity"))?;
        objects.insert(resolved_reference_binding(template, part)?.object_id);
    }
    if objects.len() != 1 {
        return fail("Native height baseline no longer resolves to one printable object; review the handoff bindings");
    }
    let object_id = *objects.first().unwrap();
    let ordinal = template
        .summary
        .objects
        .iter()
        .find(|object| object.object_id == object_id)
        .ok_or_else(|| err("Height refresh object disappeared"))?
        .object_ordinal;
    Ok((object_id, ordinal))
}

fn same_profile(
    left: Option<&Vec<PrintLayerHeightPointDto>>,
    right: Option<&Vec<PrintLayerHeightPointDto>>,
) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => {
            left.len() == right.len()
                && left.iter().zip(right).all(|(a, b)| {
                    (a.z_mm - b.z_mm).abs() <= 1e-6 && (a.height_mm - b.height_mm).abs() <= 1e-6
                })
        }
        _ => false,
    }
}
fn same_ranges(
    left: &[BambuHeightRangeSnapshotDto],
    right: &[BambuHeightRangeSnapshotDto],
) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(a, b)| {
            let optional = |a: Option<f64>, b: Option<f64>| match (a, b) {
                (None, None) => true,
                (Some(a), Some(b)) => (a - b).abs() <= 1e-6,
                _ => false,
            };
            (a.min_z_mm - b.min_z_mm).abs() <= 1e-6
                && (a.max_z_mm - b.max_z_mm).abs() <= 1e-6
                && (a.layer_height_mm - b.layer_height_mm).abs() <= 1e-6
                && a.settings.wall_count == b.settings.wall_count
                && a.settings.infill_pattern == b.settings.infill_pattern
                && a.settings.top_shell_layers == b.settings.top_shell_layers
                && a.settings.bottom_shell_layers == b.settings.bottom_shell_layers
                && optional(
                    a.settings.infill_density_percent,
                    b.settings.infill_density_percent,
                )
                && optional(a.speeds.outer_wall_mm_s, b.speeds.outer_wall_mm_s)
                && optional(a.speeds.inner_wall_mm_s, b.speeds.inner_wall_mm_s)
                && optional(a.speeds.infill_mm_s, b.speeds.infill_mm_s)
        })
}

pub(super) fn verify_reference(
    template: &Template,
    reference: &BambuRefreshReference,
) -> Result<Vec<BambuHeightObjectReadback>, ExportError> {
    if reference.height_objects.is_empty() {
        return Ok(Vec::new());
    }
    let profiles = read_profiles(&template.entries)?;
    let mut objects = BTreeSet::new();
    let mut readback = Vec::new();
    for record in &reference.height_objects {
        let (object, ordinal) = reference_object(template, reference, record)?;
        if !objects.insert(object) {
            return fail("Height refresh baselines resolve ambiguously after native save");
        }
        let ranges = read_ranges(&template.entries, ordinal)?;
        let profile = profiles.get(&ordinal).cloned();
        if !same_ranges(&ranges, &record.written_ranges)
            || !same_profile(profile.as_ref(), record.written_profile.as_ref())
        {
            return fail("Native height intervals or variable samples changed after the CAD handoff; inspect and explicitly establish a reviewed new baseline before refresh");
        }
        readback.push(BambuHeightObjectReadback {
            object_id: object,
            object_ordinal: ordinal,
            ranges,
            profile,
        });
    }
    Ok(readback)
}

pub(super) fn strip_managed(
    template: &mut Template,
    reference: Option<&BambuRefreshReference>,
) -> Result<(), ExportError> {
    let Some(reference) = reference else {
        return Ok(());
    };
    if reference.height_objects.is_empty() {
        return Ok(());
    }
    verify_reference(template, reference)?;
    let mut profiles = read_profiles(&template.entries)?;
    for record in &reference.height_objects {
        let (_, ordinal) = reference_object(template, reference, record)?;
        write_ranges(&mut template.entries, ordinal, &record.baseline_ranges)?;
        if let Some(profile) = &record.baseline_profile {
            profiles.insert(ordinal, profile.clone());
        } else {
            profiles.remove(&ordinal);
        }
    }
    write_profiles(&mut template.entries, &profiles);
    Ok(())
}

fn native_number(
    profile: &Value,
    object: &BambuTemplateObject,
    key: &str,
) -> Result<f64, ExportError> {
    let value = object
        .settings
        .get(key)
        .cloned()
        .map(Ok)
        .unwrap_or_else(|| profile_string(profile, key))?;
    let value = number(&value, key)?;
    if value <= 0. || value > 1e6 {
        return fail("Selected native layer height is outside supported bounds");
    }
    Ok(value)
}

fn native_flag(
    profile: &Value,
    object: &BambuTemplateObject,
    key: &str,
) -> Result<bool, ExportError> {
    let value = object
        .settings
        .get(key)
        .cloned()
        .map(Ok)
        .unwrap_or_else(|| profile_string(profile, key))?;
    match value.as_str() {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => fail("Selected native boolean must be 0 or 1"),
    }
}

fn validate_variable(
    template: &Template,
    object: &BambuTemplateObject,
    parts: &[&BambuPartReport],
    points: &[PrintLayerHeightPointDto],
) -> Result<(), ExportError> {
    let initial = native_number(&template.profile, object, "initial_layer_print_height")?;
    if (points[0].height_mm - initial).abs() > 1e-6 {
        return fail("Variable profile must retain the selected native initial layer height; otherwise Bambu resets the requested profile");
    }
    if native_flag(&template.profile, object, "enable_support")? {
        let style = object
            .settings
            .get("support_style")
            .cloned()
            .map(Ok)
            .unwrap_or_else(|| profile_string(&template.profile, "support_style"))?;
        if style == "tree_organic" || style == "organic" {
            return fail("Organic support is incompatible with variable layer heights in the selected native adapter");
        }
    }
    let raft_layers: u32 = object
        .settings
        .get("raft_layers")
        .cloned()
        .map(Ok)
        .unwrap_or_else(|| profile_string(&template.profile, "raft_layers"))?
        .parse()
        .map_err(err)?;
    if raft_layers > 0 {
        return fail("Variable layer profiles with rafts require separate native qualification");
    }
    let minima = profile_strings(&template.profile, "min_layer_height")?;
    let maxima = profile_strings(&template.profile, "max_layer_height")?;
    let nozzle_count = template.summary.nozzle_diameter_mm.len();
    if minima.len() != nozzle_count || maxima.len() != nozzle_count {
        return fail("Variable layer limits must map exactly to the template's physical nozzles; extruder-variant or missing limit vectors need explicit profile review");
    }
    let nozzle_map = profile_strings(&template.profile, "filament_nozzle_map")?;
    for part in parts {
        let filament = part
            .filament_index
            .checked_sub(1)
            .ok_or_else(|| err("Height profile needs an explicit filament mapping"))?
            as usize;
        let nozzle: usize = nozzle_map
            .get(filament)
            .ok_or_else(|| err("Height profile filament has no selected nozzle"))?
            .parse()
            .map_err(err)?;
        if nozzle >= nozzle_count {
            return fail("Height profile filament has no physical nozzle in the template");
        }
    }
    // Native support/interface selection may use another physical extruder. Until
    // every support path is qualified, require the schedule to fit all nozzles.
    for (minimum, maximum) in minima.iter().zip(&maxima) {
        let minimum = number(minimum, "minimum layer height")?;
        let maximum = number(maximum, "maximum layer height")?;
        if minimum <= 0.
            || maximum < minimum
            || points
                .iter()
                .any(|point| point.height_mm < minimum || point.height_mm > maximum)
        {
            return fail("Variable layer samples exceed a selected nozzle's minimum/maximum heights; the current adapter conservatively requires compatibility with every physical nozzle");
        }
    }
    Ok(())
}

fn validate_binding(
    binding: &limo_cad_core::PrintHeightBindingDto,
    body: BodyId,
    parts: &[&BambuPartReport],
    meshes: &BTreeMap<BodyId, &TriangleMesh>,
    geometry: &[BambuVolumeGeometry],
    placement: BambuPlacementMode,
) -> Result<(), ExportError> {
    let occurrences: Vec<_> = parts
        .iter()
        .filter(|part| part.binding.body_id == body)
        .collect();
    if occurrences.len() != binding.occurrences.len() {
        return fail("Height binding quantity or visibility changed; explicitly review and rebind the complete layout");
    }
    for occurrence in &binding.occurrences {
        let part = occurrences
            .iter()
            .find(|part| part.binding.occurrence_id == occurrence.occurrence_id)
            .ok_or_else(|| err("Height occurrence is absent from the selected export layout"))?;
        let source = meshes
            .get(&body)
            .ok_or_else(|| err("Enabled height intent is orphaned from source geometry"))?;
        let mut actual = Matrix::parse(Some(
            &part
                .world_transform
                .iter()
                .map(f64::to_string)
                .collect::<Vec<_>>()
                .join(" "),
        ))?;
        if placement == BambuPlacementMode::Template {
            let mut offset = Matrix::IDENTITY;
            for (axis, value) in mesh_center(source).into_iter().enumerate() {
                offset.0[axis * 4 + 3] = -value;
            }
            actual = actual.compose(offset);
        }
        let expected = Matrix::pose(&MeshInstance {
            body_id: body,
            occurrence_id: occurrence.occurrence_id,
            translation: occurrence.pose.translation_mm,
            rotation: occurrence.pose.rotation,
            visible: true,
        })?;
        if !actual.near(expected) {
            return fail("Actual exported orientation or placement differs from the height binding; review the selected layout/template placement and explicitly rebind");
        }
        let group = binding
            .groups
            .iter()
            .find(|group| group.root_occurrence_id == occurrence.root_occurrence_id)
            .ok_or_else(|| err("Height binding omitted its multipart group"))?;
        let current: BTreeSet<_> = parts
            .iter()
            .filter(|candidate| {
                candidate.binding.object_id == part.binding.object_id
                    && candidate.binding.instance_id == part.binding.instance_id
            })
            .map(|candidate| limo_cad_core::PrintSourceOccurrenceDto {
                body_id: candidate.binding.body_id,
                occurrence_id: candidate.binding.occurrence_id,
            })
            .collect();
        if current != group.members.iter().copied().collect() {
            return fail("Height binding multipart membership changed; rebind rather than applying object-level ranges to unreviewed siblings");
        }
        let world: Vec<_> = geometry
            .iter()
            .filter(|volume| {
                volume.subtype == "normal_part"
                    && volume.object_id == part.binding.object_id
                    && volume.instance_id == part.binding.instance_id
            })
            .collect();
        let min = world
            .iter()
            .map(|volume| volume.world_bounds.min_mm[2])
            .fold(f64::INFINITY, f64::min);
        let max = world
            .iter()
            .map(|volume| volume.world_bounds.max_mm[2])
            .fold(f64::NEG_INFINITY, f64::max);
        if (min - group.min_z_mm).abs() > 0.001 || (max - group.max_z_mm).abs() > 0.001 {
            return fail("Actual exported multipart bounds differ from the height binding; correct/rebind the requested interval before export");
        }
    }
    Ok(())
}

pub(super) fn append(
    result: &mut BambuProjectExport,
    meshes: &[TriangleMesh],
    intent: &PrintIntentDocumentDto,
) -> Result<(), ExportError> {
    let mut template = parse_template(&result.bytes)?;
    if !intent.height_ranges.iter().any(|range| range.enabled)
        && !intent
            .layer_height_profiles
            .iter()
            .any(|profile| profile.enabled)
    {
        if template.entries.contains_key(RANGES) || template.entries.contains_key(VARIABLE) {
            result.report.warnings.push("Unmanaged native height metadata is retained unchanged; review its compatibility with replacement geometry when reslicing.".into());
        }
        return Ok(());
    }
    let source: BTreeMap<_, _> = meshes.iter().map(|mesh| (mesh.body_id, mesh)).collect();
    let geometry = read_bambu_volume_geometry(&result.bytes)?;
    let all_parts: Vec<_> = result.report.parts.iter().collect();
    for range in intent.height_ranges.iter().filter(|range| range.enabled) {
        validate_binding(
            &range.binding,
            range.body_id,
            &all_parts,
            &source,
            &geometry,
            result.report.placement,
        )?;
    }
    for profile in intent
        .layer_height_profiles
        .iter()
        .filter(|profile| profile.enabled)
    {
        validate_binding(
            &profile.binding,
            profile.body_id,
            &all_parts,
            &source,
            &geometry,
            result.report.placement,
        )?;
    }
    let mut profiles = read_profiles(&template.entries)?;
    let mut references = Vec::new();
    for object in &template.summary.objects {
        let parts: Vec<_> = all_parts
            .iter()
            .copied()
            .filter(|part| part.binding.object_id == object.object_id)
            .collect();
        if !parts.iter().any(|part| {
            intent
                .height_ranges
                .iter()
                .any(|range| range.enabled && range.body_id == part.binding.body_id)
                || intent
                    .layer_height_profiles
                    .iter()
                    .any(|profile| profile.enabled && profile.body_id == part.binding.body_id)
        }) {
            continue;
        }
        let baseline_ranges = read_ranges(&template.entries, object.object_ordinal)?;
        let baseline_profile = profiles.get(&object.object_ordinal).cloned();
        for instance in 0..object.instance_count {
            let volumes: Vec<_> = geometry
                .iter()
                .filter(|volume| {
                    volume.subtype == "normal_part"
                        && volume.object_id == object.object_id
                        && volume.instance_id == instance
                })
                .collect();
            let min = volumes
                .iter()
                .map(|volume| volume.world_bounds.min_mm[2])
                .fold(f64::INFINITY, f64::min);
            let max = volumes
                .iter()
                .map(|volume| volume.world_bounds.max_mm[2])
                .fold(f64::NEG_INFINITY, f64::max);
            let height = max - min;
            if baseline_ranges
                .iter()
                .any(|range| range.max_z_mm > height + 0.001)
            {
                return fail("Existing native height baseline exceeds the replacement object's actual height; reconcile the native edits before refresh");
            }
            if let Some(points) = &baseline_profile {
                if points.len() < 3
                    || points[0].z_mm != 0.
                    || (points.last().unwrap().z_mm - height).abs() > 0.001
                {
                    return fail("Existing native variable profile does not match the replacement object's actual height; review the baseline rather than allowing a silent native reset");
                }
            }
        }
        if let Some(points) = &baseline_profile {
            validate_variable(&template, object, &parts, points)?;
        }
        let mut requested_ranges: Option<Vec<BambuHeightRangeSnapshotDto>> = None;
        let mut requested_profile: Option<Option<Vec<PrintLayerHeightPointDto>>> = None;
        for part in &parts {
            let mut ranges = Vec::new();
            for range in intent
                .height_ranges
                .iter()
                .filter(|range| range.enabled && range.body_id == part.binding.body_id)
            {
                let occurrence = range
                    .binding
                    .occurrences
                    .iter()
                    .find(|occurrence| occurrence.occurrence_id == part.binding.occurrence_id)
                    .ok_or_else(|| err("Height range omitted an intentional occurrence"))?;
                let [min_z_mm, max_z_mm] = match range.coordinate {
                    limo_cad_core::PrintHeightCoordinateDto::ObjectBottom => {
                        [range.min_z_mm, range.max_z_mm]
                    }
                    limo_cad_core::PrintHeightCoordinateDto::BuildPlate => [
                        range.min_z_mm - occurrence.min_z_mm,
                        range.max_z_mm - occurrence.min_z_mm,
                    ],
                };
                let mut effective = part.effective_settings.clone();
                effective.extend(settings_map(&range.settings));
                validate_effective(&effective)?;
                ranges.push(BambuHeightRangeSnapshotDto {
                    min_z_mm,
                    max_z_mm,
                    layer_height_mm: native_number(&template.profile, object, "layer_height")?,
                    settings: range.settings.clone(),
                    speeds: range.speeds.clone(),
                });
            }
            ranges.sort_by(|a, b| {
                a.min_z_mm
                    .total_cmp(&b.min_z_mm)
                    .then(a.max_z_mm.total_cmp(&b.max_z_mm))
            });
            if requested_ranges
                .as_ref()
                .is_some_and(|previous| previous != &ranges)
            {
                return fail("Bambu height ranges apply to the whole multipart object; every sibling volume and intentional instance must carry equivalent reviewed intervals/settings");
            }
            requested_ranges = Some(ranges);
            let profile = intent
                .layer_height_profiles
                .iter()
                .find(|profile| profile.enabled && profile.body_id == part.binding.body_id)
                .map(|profile| profile.points.clone());
            if requested_profile
                .as_ref()
                .is_some_and(|previous| previous != &profile)
            {
                return fail("Bambu variable layering applies to the entire multipart object; all sibling volumes/instances must request the same reviewed profile");
            }
            requested_profile = Some(profile);
        }
        let requested_ranges = requested_ranges.unwrap_or_default();
        let requested_profile = requested_profile.flatten();
        if requested_ranges.is_empty() && requested_profile.is_none() {
            continue;
        }
        for requested in &requested_ranges {
            if baseline_ranges.iter().any(|native| {
                requested.min_z_mm < native.max_z_mm && requested.max_z_mm > native.min_z_mm
            }) {
                return fail("Requested height intervals overlap existing native baseline edits; inspect/reconcile them explicitly rather than relying on implicit priority");
            }
        }
        let mut written_ranges = baseline_ranges.clone();
        written_ranges.extend(requested_ranges);
        written_ranges.sort_by(|a, b| {
            a.min_z_mm
                .total_cmp(&b.min_z_mm)
                .then(a.max_z_mm.total_cmp(&b.max_z_mm))
        });
        let written_profile = requested_profile.or(baseline_profile.clone());
        if let Some(points) = &written_profile {
            validate_variable(&template, object, &parts, points)?;
        }
        write_ranges(
            &mut template.entries,
            object.object_ordinal,
            &written_ranges,
        )?;
        if let Some(points) = &written_profile {
            profiles.insert(object.object_ordinal, points.clone());
        }
        references.push(limo_cad_core::BambuRefreshHeightObjectDto {
            source_bindings: parts
                .iter()
                .map(|part| limo_cad_core::PrintSourceOccurrenceDto {
                    body_id: part.binding.body_id,
                    occurrence_id: part.binding.occurrence_id,
                })
                .collect(),
            baseline_ranges,
            written_ranges,
            baseline_profile,
            written_profile,
        });
    }
    if !profiles.is_empty() && profile_string(&template.profile, "enable_prime_tower")? == "1" {
        let mut schedules = BTreeMap::<u32, Option<Vec<PrintLayerHeightPointDto>>>::new();
        for object in &template.summary.objects {
            for instance in 0..object.instance_count {
                let plate = template.plate_indices[&(object.object_id, instance)];
                let schedule = profiles.get(&object.object_ordinal).cloned();
                if schedules
                    .get(&plate)
                    .is_some_and(|previous| previous != &schedule)
                {
                    return fail("Prime-tower objects on one plate require matching variable schedules; review the complete template without silently changing tower/support settings");
                }
                schedules.insert(plate, schedule);
            }
        }
    }
    write_profiles(&mut template.entries, &profiles);
    result.report.refresh_reference.height_objects = references;
    result
        .report
        .refresh_reference
        .validate()
        .map_err(ExportError)?;
    template.entries.insert(
        MANIFEST.into(),
        serde_json::to_vec_pretty(&result.report.refresh_reference).map_err(err)?,
    );
    result.bytes = write_archive(&template.entries)?;
    result.report.output_sha256 = hash(&result.bytes);
    let mut checked = parse_template(&result.bytes)?;
    strip_managed(&mut checked, Some(&result.report.refresh_reference))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use limo_cad_core::{
        PrintHeightBindingDto, PrintHeightCoordinateDto, PrintHeightGroupDto, PrintHeightLayoutDto,
        PrintHeightOccurrenceDto, PrintHeightRangeDto, PrintLayerHeightProfileDto,
        PrintLocalPoseDto, PrintSourceOccurrenceDto,
    };
    use serde_json::json;

    fn binding(
        body: BodyId,
        meshes: &[TriangleMesh],
        instances: &[MeshInstance],
    ) -> PrintHeightBindingDto {
        let mut groups = BTreeMap::<u64, PrintHeightGroupDto>::new();
        for instance in instances.iter().filter(|instance| instance.visible) {
            let root = if instance.occurrence_id < 20 {
                100
            } else {
                200
            };
            let group = groups.entry(root).or_insert(PrintHeightGroupDto {
                root_occurrence_id: root,
                members: Vec::new(),
                min_z_mm: f64::INFINITY,
                max_z_mm: f64::NEG_INFINITY,
            });
            group.members.push(PrintSourceOccurrenceDto {
                body_id: instance.body_id,
                occurrence_id: instance.occurrence_id,
            });
            let matrix = Matrix::pose(instance).unwrap();
            let mesh = meshes
                .iter()
                .find(|mesh| mesh.body_id == instance.body_id)
                .unwrap();
            for point in mesh.positions.as_chunks::<3>().0 {
                let z = (0..3)
                    .map(|axis| matrix.0[8 + axis] * point[axis])
                    .sum::<f64>()
                    + matrix.0[11];
                group.min_z_mm = group.min_z_mm.min(z);
                group.max_z_mm = group.max_z_mm.max(z);
            }
        }
        PrintHeightBindingDto {
            layout: PrintHeightLayoutDto::Assembly,
            occurrences: instances
                .iter()
                .filter(|instance| instance.visible && instance.body_id == body)
                .map(|instance| {
                    let root = if instance.occurrence_id < 20 {
                        100
                    } else {
                        200
                    };
                    let group = &groups[&root];
                    PrintHeightOccurrenceDto {
                        body_id: body,
                        occurrence_id: instance.occurrence_id,
                        root_occurrence_id: root,
                        pose: PrintLocalPoseDto {
                            translation_mm: instance.translation,
                            rotation: instance.rotation,
                        },
                        min_z_mm: group.min_z_mm,
                        max_z_mm: group.max_z_mm,
                    }
                })
                .collect(),
            groups: groups.into_values().collect(),
        }
    }
    fn ranges(
        intent: &mut PrintIntentDocumentDto,
        meshes: &[TriangleMesh],
        instances: &[MeshInstance],
    ) {
        intent.height_ranges = (1..=2)
            .map(|id| PrintHeightRangeDto {
                id: format!("01234567-89ab-4cde-8123-{id:012}"),
                name: "Reviewed entire multipart band".into(),
                body_id: BodyId(id),
                enabled: true,
                coordinate: PrintHeightCoordinateDto::ObjectBottom,
                min_z_mm: 2.,
                max_z_mm: 7.,
                binding: binding(BodyId(id), meshes, instances),
                settings: PrintSettingsDto {
                    wall_count: Some(6),
                    infill_density_percent: Some(80.),
                    ..Default::default()
                },
                speeds: Default::default(),
            })
            .collect();
    }
    fn full_layer_profile(template: &[u8]) -> Vec<u8> {
        let mut entries = archive(template).unwrap();
        let mut profile: Value = serde_json::from_slice(&entries[PROFILE]).unwrap();
        for (key, value) in [
            ("min_layer_height", json!(["0.08", "0.08"])),
            ("max_layer_height", json!(["0.28", "0.28"])),
            ("enable_support", json!("0")),
            ("enable_prime_tower", json!("0")),
            ("raft_layers", json!("0")),
        ] {
            profile[key] = value;
        }
        entries.insert(PROFILE.into(), serde_json::to_vec(&profile).unwrap());
        write_archive(&entries).unwrap()
    }
    #[test]
    fn height_writer_repeated_nested_groups_readback_refresh_and_partial_sibling_rejection() {
        let (template, meshes, appearances, instances, structure, mut intent, request) =
            super::super::tests::fixture();
        ranges(&mut intent, &meshes, &instances);
        let output = write_bambu_project(
            &template,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request,
        )
        .unwrap();
        let parsed = parse_template(&output.bytes).unwrap();
        let verified =
            inspect_bambu_height_reference(&output.bytes, &output.report.refresh_reference)
                .unwrap();
        assert_eq!(verified.len(), 1);
        assert_eq!(verified[0].object_ordinal, 1);
        verify_bambu_height_reference(&output.bytes, &output.report.refresh_reference).unwrap();
        let written = read_ranges(&parsed.entries, 1).unwrap();
        assert_eq!(written.len(), 1);
        assert_eq!(written[0].min_z_mm, 2.);
        assert_eq!(written[0].max_z_mm, 7.);
        assert_eq!(written[0].layer_height_mm, 0.2);
        assert_eq!(written[0].settings.wall_count, Some(6));
        assert_eq!(
            output.report.refresh_reference.height_objects[0]
                .source_bindings
                .len(),
            4
        );
        assert!(
            read_ranges(&parsed.entries, 20).unwrap().is_empty(),
            "native ordinal is not resource id"
        );
        let refresh = BambuProjectRequest {
            bindings: Vec::new(),
            refresh_reference: Some(output.report.refresh_reference.clone()),
            ..request.clone()
        };
        let renewed = write_bambu_project(
            &output.bytes,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &refresh,
        )
        .unwrap();
        assert_eq!(
            renewed.report.refresh_reference.height_objects,
            output.report.refresh_reference.height_objects
        );
        let mut removed = intent.clone();
        removed.height_ranges.clear();
        let reset = write_bambu_project(
            &output.bytes,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &removed,
            &refresh,
        )
        .unwrap();
        assert!(
            read_ranges(&parse_template(&reset.bytes).unwrap().entries, 1)
                .unwrap()
                .is_empty()
        );
        let mut partial = intent.clone();
        partial.height_ranges.pop();
        assert!(write_bambu_project(
            &template,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &partial,
            &request
        )
        .err()
        .unwrap()
        .0
        .contains("whole multipart"));
        let mut stale = intent.clone();
        stale.height_ranges[0].binding.occurrences[0]
            .pose
            .translation_mm[0] += 1.;
        assert!(write_bambu_project(
            &template,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &stale,
            &request
        )
        .is_err());
        let mut modified = parsed.entries;
        modified.insert(
            RANGES.into(),
            range_xml(
                1,
                &[BambuHeightRangeSnapshotDto {
                    min_z_mm: 1.,
                    ..written[0].clone()
                }],
                [1; 3],
            )
            .replace("<object", "<objects><object")
            .replace("</object>", "</object></objects>")
            .into_bytes(),
        );
        assert!(verify_bambu_height_reference(
            &write_archive(&modified).unwrap(),
            &output.report.refresh_reference
        )
        .is_err());
        assert!(write_bambu_project(
            &write_archive(&modified).unwrap(),
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &refresh
        )
        .err()
        .unwrap()
        .0
        .contains("changed after"));
    }
    #[test]
    fn variable_profile_uses_actual_nozzle_and_initial_layer_without_altering_geometry() {
        let (template, meshes, appearances, instances, structure, mut intent, request) =
            super::super::tests::fixture();
        let template = full_layer_profile(&template);
        let baseline = write_bambu_project(
            &template,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request,
        )
        .unwrap();
        intent.layer_height_profiles = (1..=2)
            .map(|id| PrintLayerHeightProfileDto {
                id: format!("01234567-89ab-4cde-8123-{id:012}"),
                name: "Fine upper layers".into(),
                body_id: BodyId(id),
                enabled: true,
                binding: binding(BodyId(id), &meshes, &instances),
                points: vec![
                    PrintLayerHeightPointDto {
                        z_mm: 0.,
                        height_mm: 0.2,
                    },
                    PrintLayerHeightPointDto {
                        z_mm: 5.,
                        height_mm: 0.16,
                    },
                    PrintLayerHeightPointDto {
                        z_mm: 10.,
                        height_mm: 0.12,
                    },
                ],
            })
            .collect();
        let output = write_bambu_project(
            &template,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request,
        )
        .unwrap();
        assert_eq!(
            read_profiles(&parse_template(&output.bytes).unwrap().entries).unwrap()[&1],
            intent.layer_height_profiles[0].points
        );
        assert_eq!(
            output.report.parts, baseline.report.parts,
            "only native layer metadata changes"
        );
        let mut limits = archive(&template).unwrap();
        let mut profile: Value = serde_json::from_slice(&limits[PROFILE]).unwrap();
        // Filament maps to nozzle zero, but support can use the other physical
        // nozzle. Never accept samples by looking only at the first limit.
        profile["min_layer_height"] = json!(["0.08", "0.15"]);
        limits.insert(PROFILE.into(), serde_json::to_vec(&profile).unwrap());
        assert!(write_bambu_project(
            &write_archive(&limits).unwrap(),
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request
        )
        .err()
        .unwrap()
        .0
        .contains("every physical nozzle"));
        profile["min_layer_height"] = json!(["0.08", "0.08", "0.08", "0.15", "0.15", "0.15"]);
        limits.insert(PROFILE.into(), serde_json::to_vec(&profile).unwrap());
        assert!(write_bambu_project(
            &write_archive(&limits).unwrap(),
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request
        )
        .err()
        .unwrap()
        .0
        .contains("physical nozzles"));
        for profile in &mut intent.layer_height_profiles {
            profile.points[1].height_mm = 0.02;
        }
        assert!(write_bambu_project(
            &template,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request
        )
        .err()
        .unwrap()
        .0
        .contains("selected nozzle"));
        for profile in &mut intent.layer_height_profiles {
            profile.points[1].height_mm = 0.16;
            profile.points[0].height_mm = 0.12;
        }
        assert!(write_bambu_project(
            &template,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request
        )
        .err()
        .unwrap()
        .0
        .contains("initial layer"));
    }
    fn variable_profiles_for_meshes(
        meshes: &[TriangleMesh],
        instances: &[MeshInstance],
    ) -> Vec<PrintLayerHeightProfileDto> {
        meshes
            .iter()
            .map(|mesh| {
                let mut captured = binding(mesh.body_id, meshes, instances);
                let roots: BTreeSet<_> = captured
                    .occurrences
                    .iter()
                    .map(|occurrence| occurrence.root_occurrence_id)
                    .collect();
                captured
                    .groups
                    .retain(|group| roots.contains(&group.root_occurrence_id));
                PrintLayerHeightProfileDto {
                    id: format!("01234567-89ab-4cde-8123-{:012}", mesh.body_id.0),
                    name: "Reviewed plate schedule".into(),
                    body_id: mesh.body_id,
                    enabled: true,
                    binding: captured,
                    points: vec![
                        PrintLayerHeightPointDto {
                            z_mm: 0.,
                            height_mm: 0.2,
                        },
                        PrintLayerHeightPointDto {
                            z_mm: 5.,
                            height_mm: 0.16,
                        },
                        PrintLayerHeightPointDto {
                            z_mm: 10.,
                            height_mm: 0.12,
                        },
                    ],
                }
            })
            .collect()
    }

    #[test]
    fn variable_profiles_validate_shared_plate_prime_tower_schedules() {
        let (
            template,
            mut meshes,
            mut appearances,
            mut instances,
            structure,
            mut intent,
            mut request,
        ) = super::super::tests::fixture();
        let mut entries = archive(&full_layer_profile(&template)).unwrap();
        let root = text(&entries, ROOT).unwrap().to_owned();
        let document = xml(&root).unwrap();
        let object = document
            .descendants()
            .find(|node| node.has_tag_name((CORE_NS, "object")))
            .unwrap();
        let second = root[object.range()]
            .replace("id=\"20\"", "id=\"30\"")
            .replace("/a.model", "/c.model")
            .replace("/b.model", "/d.model");
        let second_item = document
            .descendants()
            .filter(|node| node.has_tag_name((CORE_NS, "item")))
            .nth(1)
            .unwrap();
        let item = root[second_item.range()].replace("objectid=\"20\"", "objectid=\"30\"");
        let root = apply_edits(&root, vec![(second_item.range(), item)])
            .unwrap()
            .replace("</resources>", &format!("{second}</resources>"));
        entries.insert(ROOT.into(), root.into_bytes());
        for (old, new) in [("a", "c"), ("b", "d")] {
            let bytes = entries[&format!("3D/Objects/{old}.model")].clone();
            entries.insert(format!("3D/Objects/{new}.model"), bytes);
        }
        let rels = text(&entries, "3D/_rels/3dmodel.model.rels").unwrap()
            .replace("</Relationships>", r#"<Relationship Id="part-c" Target="/3D/Objects/c.model" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/><Relationship Id="part-d" Target="/3D/Objects/d.model" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/></Relationships>"#);
        entries.insert("3D/_rels/3dmodel.model.rels".into(), rels.into_bytes());
        let source = text(&entries, CONFIG).unwrap().to_owned();
        let document = xml(&source).unwrap();
        let object = document
            .descendants()
            .find(|node| node.has_tag_name("object"))
            .unwrap();
        let second = source[object.range()]
            .replace("id=\"20\"", "id=\"30\"")
            .replace("first-volume", "third-volume")
            .replace("second-volume", "fourth-volume");
        let instance = document
            .descendants()
            .filter(|node| node.has_tag_name("model_instance"))
            .nth(1)
            .unwrap();
        let replacement = source[instance.range()]
            .replace(
                "key=\"object_id\" value=\"20\"",
                "key=\"object_id\" value=\"30\"",
            )
            .replace(
                "key=\"instance_id\" value=\"1\"",
                "key=\"instance_id\" value=\"0\"",
            );
        let source = apply_edits(
            &source,
            vec![
                (instance.range(), replacement),
                (object.range().end..object.range().end, second),
            ],
        )
        .unwrap();
        entries.insert(CONFIG.into(), source.into_bytes());
        let mut profile: Value = serde_json::from_slice(&entries[PROFILE]).unwrap();
        profile["enable_prime_tower"] = json!("1");
        entries.insert(PROFILE.into(), serde_json::to_vec(&profile).unwrap());
        let template = write_archive(&entries).unwrap();
        let summary = inspect_bambu_template(&template).unwrap();
        assert_eq!(summary.plate_count, 1);
        assert_eq!(summary.objects.len(), 2);
        assert!(summary
            .objects
            .iter()
            .all(|object| object.instance_count == 1));

        for (index, body) in [3, 4].into_iter().enumerate() {
            let mut mesh = meshes[index].clone();
            mesh.body_id = BodyId(body);
            meshes.push(mesh);
            let mut appearance = appearances[index].clone();
            appearance.body_id = BodyId(body);
            appearances.push(appearance);
            instances[index + 2].body_id = BodyId(body);
            request.bindings[index + 2].body_id = BodyId(body);
            request.bindings[index + 2].object_id = 30;
            request.bindings[index + 2].instance_id = 0;
        }
        let mut structure = serde_json::to_value(structure).unwrap();
        for (index, body) in [3, 4].into_iter().enumerate() {
            let mut definition = structure["definitions"][index + 1].clone();
            definition["id"] = json!(index + 4);
            definition["name"] = json!(format!("Part{body}"));
            definition["body_ids"] = json!([body]);
            structure["definitions"]
                .as_array_mut()
                .unwrap()
                .push(definition);
            structure["occurrences"][index + 4]["component_id"] = json!(index + 4);
        }
        structure["next_component_id"] = json!(6);
        let structure = serde_json::from_value(structure).unwrap();
        intent.layer_height_profiles = variable_profiles_for_meshes(&meshes, &instances);
        let matching = write_bambu_project(
            &template,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request,
        )
        .unwrap();
        let profiles = read_profiles(&parse_template(&matching.bytes).unwrap().entries).unwrap();
        assert_eq!(profiles.len(), 2);
        assert_eq!(profiles[&1], profiles[&2]);
        assert!(matching
            .report
            .z_preflight
            .iter()
            .all(|group| group.issues.is_empty()));

        let mut different = intent.clone();
        for schedule in &mut different.layer_height_profiles[2..] {
            schedule.points[1].height_mm = 0.18;
        }
        let original = different.clone();
        let error = write_bambu_project(
            &template,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &different,
            &request,
        )
        .err()
        .unwrap();
        assert!(
            error
                .0
                .contains("Prime-tower objects on one plate require matching variable schedules"),
            "{error}"
        );
        assert_eq!(
            different, original,
            "Incompatibility never rewrites requested schedules"
        );
        let mut mixed = intent.clone();
        mixed
            .layer_height_profiles
            .retain(|schedule| schedule.body_id.0 <= 2);
        let error = write_bambu_project(
            &template,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &mixed,
            &request,
        )
        .err()
        .unwrap();
        assert!(error.0.contains("matching variable schedules"), "{error}");
        let fixed = PrintIntentDocumentDto {
            layer_height_profiles: vec![],
            ..intent
        };
        assert!(write_bambu_project(
            &template,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &fixed,
            &request
        )
        .is_ok());
    }

    #[test]
    fn variable_profiles_reject_organic_support_and_rafts_without_changing_intent() {
        let (template, meshes, appearances, instances, structure, mut intent, request) =
            super::super::tests::fixture();
        let template = full_layer_profile(&template);
        intent.layer_height_profiles = variable_profiles_for_meshes(&meshes, &instances);
        for (enable_support, style, raft_layers, expected) in [
            ("1", "tree_organic", "0", "Organic support is incompatible"),
            ("1", "organic", "0", "Organic support is incompatible"),
            (
                "0",
                "default",
                "1",
                "rafts require separate native qualification",
            ),
        ] {
            let mut entries = archive(&template).unwrap();
            let mut profile: Value = serde_json::from_slice(&entries[PROFILE]).unwrap();
            profile["enable_support"] = json!(enable_support);
            profile["support_style"] = json!(style);
            profile["raft_layers"] = json!(raft_layers);
            entries.insert(PROFILE.into(), serde_json::to_vec(&profile).unwrap());
            let original = intent.clone();
            let error = write_bambu_project(
                &write_archive(&entries).unwrap(),
                &meshes,
                &appearances,
                &instances,
                &structure,
                &intent,
                &request,
            )
            .err()
            .unwrap();
            assert!(error.0.contains(expected), "{error}");
            assert_eq!(
                intent, original,
                "Incompatible support settings never change print intent"
            );
        }
    }

    #[test]
    fn object_bottom_ranges_do_not_depend_on_repeat_world_z_roundoff() {
        let (template, meshes, appearances, mut instances, structure, mut intent, mut request) =
            super::super::tests::fixture();
        for instance in &mut instances {
            instance.translation[2] = if instance.occurrence_id < 20 {
                10.1
            } else {
                100.1
            };
        }
        ranges(&mut intent, &meshes, &instances);
        request.placement = BambuPlacementMode::ResolvedScene;
        let output = write_bambu_project(
            &template,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request,
        )
        .unwrap();
        let readback =
            inspect_bambu_height_reference(&output.bytes, &output.report.refresh_reference)
                .unwrap();
        assert_eq!(readback.len(), 1);
        assert_eq!(readback[0].ranges[0].min_z_mm, 2.);
        assert_eq!(readback[0].ranges[0].max_z_mm, 7.);
        assert_eq!(output.report.parts.len(), 4);
    }

    #[test]
    fn unmanaged_native_height_options_survive_ordinary_export_and_require_review_when_targeted() {
        let (template, meshes, appearances, instances, structure, mut intent, request) =
            super::super::tests::fixture();
        let mut entries = archive(&template).unwrap();
        let native = b"<objects><object id=\"1\"><range min_z=\"2\" max_z=\"7\"><option opt_key=\"layer_height\">0.2</option><option opt_key=\"unknown_native\">4</option></range></object></objects>".to_vec();
        entries.insert(RANGES.into(), native.clone());
        let template = write_archive(&entries).unwrap();
        let output = write_bambu_project(
            &template,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request,
        )
        .unwrap();
        assert_eq!(archive(&output.bytes).unwrap()[RANGES], native);
        assert!(output
            .report
            .warnings
            .iter()
            .any(|warning| warning.contains("Unmanaged native height")));
        let refresh = BambuProjectRequest {
            bindings: Vec::new(),
            refresh_reference: Some(output.report.refresh_reference.clone()),
            ..request.clone()
        };
        let renewed = write_bambu_project(
            &output.bytes,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &refresh,
        )
        .unwrap();
        assert_eq!(archive(&renewed.bytes).unwrap()[RANGES], native);
        ranges(&mut intent, &meshes, &instances);
        assert!(write_bambu_project(
            &template,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request
        )
        .err()
        .unwrap()
        .0
        .contains("not be silently erased"));
    }

    #[test]
    fn native_height_parser_preserves_empty_roots_and_rejects_unknown_options_and_holes() {
        let mut entries = BTreeMap::from([(RANGES.into(), b"<objects/>".to_vec())]);
        let range = BambuHeightRangeSnapshotDto {
            min_z_mm: 2.,
            max_z_mm: 7.,
            layer_height_mm: 0.2,
            settings: PrintSettingsDto {
                wall_count: Some(6),
                ..Default::default()
            },
            speeds: Default::default(),
        };
        write_ranges(&mut entries, 1, std::slice::from_ref(&range)).unwrap();
        assert_eq!(read_ranges(&entries, 1).unwrap(), vec![range]);
        entries.insert(RANGES.into(),b"<objects><object id=\"1\"><range min_z=\"2\" max_z=\"7\"><option opt_key=\"layer_height\">0.2</option><option opt_key=\"unknown_native\">4</option></range></object></objects>".to_vec());
        assert!(read_ranges(&entries, 1)
            .err()
            .unwrap()
            .0
            .contains("not be silently erased"));
        entries.insert(
            VARIABLE.into(),
            b"object_id=1|0;0.2;;5;0.16;10;0.12".to_vec(),
        );
        assert!(read_profiles(&entries)
            .err()
            .unwrap()
            .0
            .contains("empty sample"));
        entries.remove(VARIABLE);
        entries.insert(RANGES.into(), b"<objects/>".to_vec());
        entries.insert(
            PROFILE.into(),
            serde_json::to_vec(&json!({
                "outer_wall_speed":["60","60","60","50","50","50"],
                "inner_wall_speed":["150","150","150","100","100","100"],
                "sparse_infill_speed":["200","200","200","100","100","100"]
            }))
            .unwrap(),
        );
        let speed_range = BambuHeightRangeSnapshotDto {
            min_z_mm: 2.,
            max_z_mm: 7.,
            layer_height_mm: 0.2,
            settings: Default::default(),
            speeds: PrintHeightSpeedsDto {
                outer_wall_mm_s: Some(12.),
                inner_wall_mm_s: Some(14.),
                infill_mm_s: Some(16.),
            },
        };
        write_ranges(&mut entries, 1, std::slice::from_ref(&speed_range)).unwrap();
        let document = xml(text(&entries, RANGES).unwrap()).unwrap();
        assert_eq!(
            document
                .descendants()
                .find(|node| node.attribute("opt_key") == Some("outer_wall_speed"))
                .unwrap()
                .text(),
            Some("12,12,12,12,12,12")
        );
        assert_eq!(read_ranges(&entries, 1).unwrap(), vec![speed_range]);
        let source = text(&entries, RANGES)
            .unwrap()
            .replace("12,12,12,12,12,12", "12,12,12,50,50,50");
        entries.insert(RANGES.into(), source.into_bytes());
        assert!(read_ranges(&entries, 1)
            .err()
            .unwrap()
            .0
            .contains("distinct extruder-variant"));
    }

    #[test]
    #[ignore = "requires LIMO_BAMBU_TEMPLATE complete profile and fresh LIMO_BAMBU_QUALIFICATION_DIR; native slicing runs separately"]
    fn write_native_print_height_qualification_fixtures() {
        let input =
            std::env::var_os("LIMO_BAMBU_TEMPLATE").expect("operator complete saved template");
        let directory = std::path::PathBuf::from(
            std::env::var_os("LIMO_BAMBU_QUALIFICATION_DIR").expect("fresh owned output directory"),
        );
        assert!(directory.is_absolute() && directory.is_dir());
        let original = std::fs::read(input).unwrap();
        let complete = parse_template(&original).unwrap();
        let (synthetic, mut meshes, mut appearances, mut instances, structure, mut intent, request) =
            super::super::tests::fixture();
        let mut entries = archive(&synthetic).unwrap();
        entries.insert(
            PROFILE.into(),
            serde_json::to_vec(&complete.profile).unwrap(),
        );
        let config = text(&entries, CONFIG)
            .unwrap()
            .replace("first-volume", "7f397df8-a10a-4d87-a0b2-90b73dc18b5d")
            .replace("second-volume", "c153b5f8-e2e7-49c8-a904-29ce82ef632b");
        entries.insert(CONFIG.into(), config.into_bytes());
        let template = write_archive(&entries).unwrap();
        for mesh in &mut meshes {
            for point in mesh.positions.as_chunks_mut::<3>().0 {
                point[0] *= 3.;
                point[1] *= 3.;
                point[2] *= 4.;
            }
        }
        for (index, instance) in instances.iter_mut().enumerate() {
            instance.translation = [
                if index < 2 {
                    30. + 40. * index as f64
                } else {
                    130. + 40. * (index - 2) as f64
                },
                100.,
                0.,
            ];
            instance.rotation = [0., 0., 0., 1.];
        }
        let color = complete.summary.filament_colors[0].trim_start_matches('#');
        for appearance in &mut appearances {
            appearance.filament_type = complete.summary.filament_types[0].clone();
            appearance.color = limo_cad_core::Rgba8::opaque(
                u8::from_str_radix(&color[..2], 16).unwrap(),
                u8::from_str_radix(&color[2..4], 16).unwrap(),
                u8::from_str_radix(&color[4..6], 16).unwrap(),
            );
        }
        intent.parts.clear();
        let mut evidence = Vec::new();
        for orientation in ["upright", "horizontal"] {
            if orientation == "horizontal" {
                let half = std::f64::consts::FRAC_1_SQRT_2;
                for instance in &mut instances {
                    instance.rotation = [half, 0., 0., half];
                }
            }
            let height = if orientation == "upright" { 40. } else { 30. };
            for mode in ["baseline", "ranges", "speeds", "variable", "precedence"] {
                let mut requested = intent.clone();
                if ["ranges", "speeds", "precedence"].contains(&mode) {
                    ranges(&mut requested, &meshes, &instances);
                    for range in &mut requested.height_ranges {
                        range.min_z_mm = 5.;
                        range.max_z_mm = 15.;
                    }
                }
                if mode == "speeds" {
                    for range in &mut requested.height_ranges {
                        range.speeds = PrintHeightSpeedsDto {
                            outer_wall_mm_s: Some(12.),
                            inner_wall_mm_s: Some(14.),
                            infill_mm_s: Some(16.),
                        };
                    }
                }
                if mode == "variable" {
                    requested.layer_height_profiles = (1..=2)
                        .map(|id| PrintLayerHeightProfileDto {
                            id: format!("abcdef01-89ab-4cde-8123-{id:012}"),
                            name: "Actual nozzle fine upper layers".into(),
                            body_id: BodyId(id),
                            enabled: true,
                            binding: binding(BodyId(id), &meshes, &instances),
                            points: vec![
                                PrintLayerHeightPointDto {
                                    z_mm: 0.,
                                    height_mm: 0.2,
                                },
                                PrintLayerHeightPointDto {
                                    z_mm: height / 2.,
                                    height_mm: 0.2,
                                },
                                PrintLayerHeightPointDto {
                                    z_mm: height,
                                    height_mm: 0.12,
                                },
                            ],
                        })
                        .collect();
                }
                if mode == "precedence" {
                    requested.modifiers = vec![limo_cad_core::PrintModifierDto {
                        id: "aaaaaaaa-89ab-4cde-8123-456789abcdef".into(),
                        name: "Local zone above height range".into(),
                        body_id: BodyId(1),
                        enabled: true,
                        local_pose: PrintLocalPoseDto {
                            translation_mm: [15., 15., 10.],
                            ..Default::default()
                        },
                        primitive: limo_cad_core::PrintModifierPrimitiveDto::Box {
                            size_mm: [12., 12., 8.],
                        },
                        settings: PrintSettingsDto {
                            wall_count: Some(8),
                            infill_density_percent: Some(100.),
                            infill_pattern: Some(InfillPatternDto::Rectilinear),
                            ..Default::default()
                        },
                    }];
                }
                let output = write_bambu_project(
                    &template,
                    &meshes,
                    &appearances,
                    &instances,
                    &structure,
                    &requested,
                    &request,
                )
                .unwrap();
                let name = format!("height-{orientation}-{mode}");
                let mut file = std::fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(directory.join(format!("{name}.3mf")))
                    .unwrap();
                file.write_all(&output.bytes).unwrap();
                let report = json!({"original_template_sha256":hash(&original),"synthetic_geometry":true,"orientation":orientation,"mode":mode,"expected_object_height_mm":height,"expected_interval_mm":[5,15],"report":output.report});
                let mut file = std::fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(directory.join(format!("{name}.json")))
                    .unwrap();
                file.write_all(&serde_json::to_vec_pretty(&report).unwrap())
                    .unwrap();
                evidence.push(json!({"name":name,"sha256":hash(&output.bytes)}));
            }
        }
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(directory.join("height-fixtures.json"))
            .unwrap();
        file.write_all(&serde_json::to_vec_pretty(&evidence).unwrap())
            .unwrap();
    }
}
