use super::*;
use limo_cad_core::{PrintModifierDto, PrintModifierPrimitiveDto};

const INTERSECTION_EPSILON: f64 = 1e-7;
const PRODUCTION_NS: &str = "http://schemas.microsoft.com/3dmanufacturing/production/2015/06";

fn translation(offset: [f64; 3]) -> Matrix {
    let mut matrix = Matrix::IDENTITY;
    for (axis, value) in offset.into_iter().enumerate() {
        matrix.0[axis * 4 + 3] = value;
    }
    matrix
}

fn volume_uuid(modifier_id: &str, parent_uuid: &str) -> String {
    let bytes = Sha256::digest(
        format!(
            "limo-cad-print-modifier/v1/{}/{parent_uuid}",
            modifier_id.to_ascii_lowercase()
        )
        .as_bytes(),
    );
    let mut bytes: [u8; 16] = bytes[..16].try_into().expect("SHA256 prefix");
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

pub(super) fn read_mesh(
    template: &Template,
    object: u32,
    part: u32,
    body: BodyId,
) -> Result<TriangleMesh, ExportError> {
    let target = &template.targets[&(object, part)];
    let document = xml(text(&template.entries, &target.path)?)?;
    let node = document
        .descendants()
        .find(|node| node.range() == target.mesh_range)
        .ok_or_else(|| err("Print modifier readback mesh is missing"))?;
    let mut mesh = TriangleMesh {
        body_id: body,
        name: String::new(),
        positions: Vec::new(),
        indices: Vec::new(),
    };
    for vertex in node
        .descendants()
        .filter(|node| node.has_tag_name((CORE_NS, "vertex")))
    {
        for axis in ["x", "y", "z"] {
            let value = vertex
                .attribute(axis)
                .ok_or_else(|| err("Modifier mesh coordinate missing"))?
                .parse::<f64>()
                .map_err(err)?;
            if !value.is_finite() || value.abs() > 10_000_000. {
                return fail("Modifier mesh coordinate is outside supported bounds");
            }
            mesh.positions.push(value);
        }
    }
    for triangle in node
        .descendants()
        .filter(|node| node.has_tag_name((CORE_NS, "triangle")))
    {
        for key in ["v1", "v2", "v3"] {
            mesh.indices.push(
                triangle
                    .attribute(key)
                    .ok_or_else(|| err("Modifier triangle index missing"))?
                    .parse()
                    .map_err(err)?,
            );
        }
    }
    validate_3mf_model_mesh(&mesh)?;
    Ok(mesh)
}

fn same_triangle_points(left: [[f64; 3]; 3], right: [[f64; 3]; 3]) -> bool {
    const PERMUTATIONS: [[usize; 3]; 6] = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    PERMUTATIONS.iter().any(|permutation| {
        (0..3).all(|point| {
            (0..3).all(|axis| (left[point][axis] - right[permutation[point]][axis]).abs() <= 0.001)
        })
    })
}

fn same_geometry(
    left: &TriangleMesh,
    left_pose: Matrix,
    right: &TriangleMesh,
    right_pose: Matrix,
) -> bool {
    same_world_geometry(
        &left.positions,
        &left.indices,
        left_pose,
        &right.positions,
        &right.indices,
        right_pose,
    )
    .unwrap_or(false)
}

pub(super) fn same_world_geometry(
    left_positions: &[f64],
    left_indices: &[u32],
    left_pose: Matrix,
    right_positions: &[f64],
    right_indices: &[u32],
    right_pose: Matrix,
) -> Result<bool, ExportError> {
    if left_indices.len() != right_indices.len() {
        return Ok(false);
    }
    if left_indices.len() / 3 > 1_000_000 || right_indices.len() / 3 > 1_000_000 {
        return fail(
            "Native geometry comparison exceeds one million triangles per volume; reduce tessellation for bounded verification",
        );
    }
    for (positions, indices) in [
        (left_positions, left_indices),
        (right_positions, right_indices),
    ] {
        if positions.is_empty()
            || indices.is_empty()
            || !positions.len().is_multiple_of(3)
            || !indices.len().is_multiple_of(3)
            || positions.iter().any(|point| !point.is_finite())
            || indices
                .iter()
                .any(|index| *index as usize >= positions.len() / 3)
        {
            return fail("Native geometry comparison received malformed mesh buffers");
        }
    }
    let points = |positions: &[f64], indices: &[u32; 3]| {
        indices.map(|index| std::array::from_fn(|axis| positions[index as usize * 3 + axis]))
    };
    for (positions, pose) in [(left_positions, left_pose), (right_positions, right_pose)] {
        if positions.as_chunks::<3>().0.iter().any(|point| {
            transform(pose, *point)
                .iter()
                .any(|coordinate| !coordinate.is_finite() || coordinate.abs() > 1e12)
        }) {
            return fail(
                "Native geometry comparison transform exceeds finite bounded world coordinates",
            );
        }
    }
    let mut comparisons = 0usize;
    let centroid_cell = |points: [[f64; 3]; 3]| -> [i64; 3] {
        std::array::from_fn(|axis| {
            ((points[0][axis] + points[1][axis] + points[2][axis]) / 3. / 0.001).floor() as i64
        })
    };
    let mut right_triangles: BTreeMap<[i64; 3], Vec<[[f64; 3]; 3]>> = BTreeMap::new();
    for points in
        right_indices.as_chunks::<3>().0.iter().map(|indices| {
            points(right_positions, indices).map(|point| transform(right_pose, point))
        })
    {
        right_triangles
            .entry(centroid_cell(points))
            .or_default()
            .push(points);
    }
    for indices in left_indices.as_chunks::<3>().0 {
        let points = points(left_positions, indices).map(|point| transform(left_pose, point));
        let cell = centroid_cell(points);
        let mut found = false;
        'cells: for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let key = [cell[0] + dx, cell[1] + dy, cell[2] + dz];
                    if let Some(candidates) = right_triangles.get_mut(&key) {
                        let mut matching_index = None;
                        for (index, candidate) in candidates.iter().enumerate() {
                            comparisons += 1;
                            if comparisons > 25_000_000 {
                                return fail(
                                    "Native geometry comparison exceeded its bounded candidate budget; simplify tessellation or review overlapping triangles",
                                );
                            }
                            if same_triangle_points(points, *candidate) {
                                matching_index = Some(index);
                                break;
                            }
                        }
                        if let Some(index) = matching_index {
                            candidates.swap_remove(index);
                            found = true;
                            break 'cells;
                        }
                    }
                }
            }
        }
        if comparisons > 25_000_000 {
            return fail(
                "Native geometry comparison exceeded its bounded candidate budget; simplify tessellation or review overlapping triangles",
            );
        }
        if !found {
            return Ok(false);
        }
    }
    Ok(right_triangles.values().all(Vec::is_empty))
}

/// Remove only generated volumes whose persistent identity, geometry and settings still match.
/// Normal resources, plates and foreign print intent are never inferred from display names.
pub(super) fn strip_managed(
    template: &mut Template,
    reference: Option<&BambuRefreshReference>,
) -> Result<(), ExportError> {
    let foreign: Vec<_> = template
        .summary
        .objects
        .iter()
        .flat_map(|object| {
            object
                .parts
                .iter()
                .filter(|part| part.subtype != "normal_part")
                .map(move |part| (object, part))
        })
        .collect();
    let owned = reference
        .map(|reference| reference.modifiers.as_slice())
        .unwrap_or_default();
    if foreign.is_empty() && owned.is_empty() {
        return Ok(());
    }
    if foreign.len() != owned.len() {
        return fail(
            "Template contains missing, added or foreign print-only volumes; review their identities rather than silently replacing native intent",
        );
    }
    let config_source = text(&template.entries, CONFIG)?.to_owned();
    let config = xml(&config_source)?;
    let mut edits: BTreeMap<String, Vec<(Range<usize>, String)>> = BTreeMap::new();
    let mut remove_resources: BTreeMap<String, BTreeSet<u32>> = BTreeMap::new();
    let mut matched = BTreeSet::new();
    for (object, part) in foreign {
        if part.subtype != "modifier_part" {
            return fail(
                "Foreign support/negative volumes need an explicitly reviewed adapter; they cannot be treated as CAD modifiers",
            );
        }
        let snapshot = owned.iter().find(|snapshot| Some(&snapshot.target_uuid) == part.uuid.as_ref())
            .ok_or_else(|| err("Native modifier UUID does not match the saved CAD handoff; inspect and explicitly recover the edited template"))?;
        if !matched.insert(&snapshot.target_uuid) {
            return fail("Native modifier UUID is ambiguous across target objects");
        }
        let parent = object
            .parts
            .iter()
            .find(|parent| {
                parent.subtype == "normal_part"
                    && Some(&snapshot.parent_volume_uuid) == parent.uuid.as_ref()
            })
            .ok_or_else(|| err("Managed print modifier lost its bound normal-volume parent"))?;
        if !settings_equal(
            &select_settings(&part.settings),
            &settings_map(&snapshot.modifier.settings),
        ) {
            return Err(err(format!(
                "Native settings on modifier '{}' changed; review/recover the zone before CAD refresh",
                snapshot.modifier.name
            )));
        }
        let safe_metadata = [
            "name",
            "matrix",
            "source_file",
            "source_object_id",
            "source_volume_id",
            "source_offset_x",
            "source_offset_y",
            "source_offset_z",
            "source_in_inches",
            "source_in_meters",
            "extruder",
        ];
        if part.settings.iter().any(|(key, value)| {
            (!SETTING_KEYS.contains(&key.as_str()) && !safe_metadata.contains(&key.as_str()))
                || (key == "extruder" && value != "0")
        }) {
            return fail(
                "Managed modifier contains a native material or unsupported process edit; review it before replacing the zone",
            );
        }
        let parent_mesh = read_mesh(
            template,
            object.object_id,
            parent.part_id,
            snapshot.modifier.body_id,
        )?;
        let actual_mesh = read_mesh(
            template,
            object.object_id,
            part.part_id,
            snapshot.modifier.body_id,
        )?;
        let parent_transform =
            template.targets[&(object.object_id, parent.part_id)].component_transform;
        let modifier_transform =
            template.targets[&(object.object_id, part.part_id)].component_transform;
        let actual_pose = translation(mesh_center(&parent_mesh).map(|value| -value))
            .compose(parent_transform.inverse()?)
            .compose(modifier_transform);
        let expected_mesh = primitive_mesh(&snapshot.modifier)?;
        let expected_pose = translation(snapshot.source_mesh_center_mm.map(|value| -value))
            .compose(modifier_pose(&snapshot.modifier)?);
        if !same_geometry(&actual_mesh, actual_pose, &expected_mesh, expected_pose) {
            return Err(err(format!(
                "Native geometry or local placement of modifier '{}' changed; review/recover it before CAD refresh",
                snapshot.modifier.name
            )));
        }
        let target = &template.targets[&(object.object_id, part.part_id)];
        edits
            .entry(ROOT.into())
            .or_default()
            .push((target.component_range.clone(), String::new()));
        let config_part = config
            .root_element()
            .children()
            .find(|node| {
                node.has_tag_name("object")
                    && node.attribute("id") == Some(object.object_id.to_string().as_str())
            })
            .unwrap()
            .children()
            .find(|node| {
                node.has_tag_name("part")
                    && node.attribute("id") == Some(part.part_id.to_string().as_str())
            })
            .unwrap();
        edits
            .entry(CONFIG.into())
            .or_default()
            .push((config_part.range(), String::new()));
        remove_resources
            .entry(target.path.clone())
            .or_default()
            .insert(part.part_id);
    }
    let mut removed_paths = BTreeSet::new();
    for (path, ids) in remove_resources {
        let source = text(&template.entries, &path)?.to_owned();
        let document = xml(&source)?;
        let resources: Vec<_> = document
            .descendants()
            .filter(|node| node.has_tag_name((CORE_NS, "object")))
            .collect();
        if path != ROOT
            && resources
                .iter()
                .all(|node| node_id(*node, "id").is_ok_and(|id| ids.contains(&id)))
        {
            removed_paths.insert(path);
        } else {
            for node in resources {
                if ids.contains(&node_id(node, "id")?) {
                    edits
                        .entry(path.clone())
                        .or_default()
                        .push((node.range(), String::new()));
                }
            }
        }
    }
    if !removed_paths.is_empty() {
        let path = "3D/_rels/3dmodel.model.rels";
        let source = text(&template.entries, path)?.to_owned();
        let document = xml(&source)?;
        for node in document
            .root_element()
            .children()
            .filter(|node| node.is_element())
        {
            if let Some(target) = node.attribute("Target") {
                if removed_paths.contains(&resolve_relationship(path, target)?) {
                    edits
                        .entry(path.into())
                        .or_default()
                        .push((node.range(), String::new()));
                }
            }
        }
    }
    apply_file_edits(&mut template.entries, edits)?;
    for path in removed_paths {
        template.entries.remove(&path);
    }
    let original_summary = template.summary.clone();
    let entries = std::mem::take(&mut template.entries);
    let mut cleaned = parse_template(&write_archive(&entries)?)?;
    cleaned.summary.template_sha256 = original_summary.template_sha256;
    cleaned.summary.has_identity_manifest = original_summary.has_identity_manifest;
    *template = cleaned;
    Ok(())
}

pub(super) fn append(
    result: &mut BambuProjectExport,
    meshes: &[TriangleMesh],
    structure: &limo_cad_assembly::ComponentStructureDto,
    intent: &PrintIntentDocumentDto,
) -> Result<(), ExportError> {
    let mut template = parse_template(&result.bytes)?;
    let source: BTreeMap<_, _> = meshes.iter().map(|mesh| (mesh.body_id, mesh)).collect();
    let retained: BTreeSet<_> = structure
        .definitions
        .iter()
        .flat_map(|definition| definition.body_ids.iter().copied())
        .collect();
    let root_source = text(&template.entries, ROOT)?.to_owned();
    let root = xml(&root_source)?;
    let config_source = text(&template.entries, CONFIG)?.to_owned();
    let config = xml(&config_source)?;
    let relations_path = "3D/_rels/3dmodel.model.rels";
    let relations_source = text(&template.entries, relations_path)?.to_owned();
    let relations = xml(&relations_source)?;
    let mut root_additions: BTreeMap<u32, String> = BTreeMap::new();
    let mut config_additions: BTreeMap<u32, String> = BTreeMap::new();
    let mut relationship_additions = String::new();
    let mut next_ids: BTreeMap<_, _> = template
        .summary
        .objects
        .iter()
        .map(|object| {
            (
                object.object_id,
                object
                    .parts
                    .iter()
                    .map(|part| part.part_id)
                    .max()
                    .unwrap_or(0),
            )
        })
        .collect();
    let mut reports = Vec::new();
    let mut references = Vec::new();
    let mut expected = Vec::new();
    for modifier in &intent.modifiers {
        let mut report = BambuModifierReport {
            id: modifier.id.clone(),
            name: modifier.name.clone(),
            body_id: modifier.body_id,
            disposition: BambuModifierDisposition::Written,
            instances: Vec::new(),
        };
        if !modifier.enabled {
            report.disposition = BambuModifierDisposition::Disabled;
            reports.push(report);
            continue;
        }
        if settings_map(&modifier.settings).is_empty() {
            report.disposition = BambuModifierDisposition::NoEffect;
            reports.push(report);
            continue;
        }
        let parents: Vec<_> = result
            .report
            .parts
            .iter()
            .filter(|part| part.binding.body_id == modifier.body_id)
            .collect();
        if parents.is_empty() {
            if !source.contains_key(&modifier.body_id) && !retained.contains(&modifier.body_id) {
                return Err(err(format!(
                    "Enabled modifier '{}' has an orphan source body {}; recover, disable or remove it before native export",
                    modifier.name, modifier.body_id.0
                )));
            }
            report.disposition = BambuModifierDisposition::Excluded;
            reports.push(report);
            continue;
        }
        let source_mesh = weld_triangle_mesh(source[&modifier.body_id], DEFAULT_WELD_EPSILON)?;
        validate_3mf_model_mesh(&source_mesh)?;
        let primitive = primitive_mesh(modifier)?;
        if !intersects_parent(&source_mesh, modifier, &primitive)? {
            return Err(err(format!(
                "Modifier '{}' does not intersect solid material in source body {}; move or resize the zone, disable it, or reset its settings",
                modifier.name, modifier.body_id.0
            )));
        }
        let mut unique = BTreeMap::new();
        for parent in &parents {
            unique
                .entry((parent.binding.object_id, parent.binding.part_id))
                .or_insert(*parent);
        }
        for ((object_id, parent_id), parent) in unique {
            if references.len() >= 1024 {
                return fail("Native export supports at most 1024 bound modifier volumes");
            }
            let target = &template.targets[&(object_id, parent_id)];
            let center = mesh_center(&source_mesh);
            let offset = if result.report.placement == BambuPlacementMode::Template {
                center.map(|v| -v)
            } else {
                [0.; 3]
            };
            let local = target
                .component_transform
                .compose(translation(offset))
                .compose(modifier_pose(modifier)?);
            let zone_bounds = bounds(
                primitive
                    .positions
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .map(|point| transform(local, *point)),
            );
            let object = template
                .summary
                .objects
                .iter()
                .find(|object| object.object_id == object_id)
                .unwrap();
            for sibling in object
                .parts
                .iter()
                .filter(|part| part.subtype == "normal_part" && part.part_id != parent_id)
            {
                let sibling_mesh =
                    read_mesh(&template, object_id, sibling.part_id, modifier.body_id)?;
                let sibling_pose =
                    template.targets[&(object_id, sibling.part_id)].component_transform;
                let sibling_bounds = bounds(
                    sibling_mesh
                        .positions
                        .as_chunks::<3>()
                        .0
                        .iter()
                        .map(|point| transform(sibling_pose, *point)),
                );
                if bounds_overlap(zone_bounds, sibling_bounds) {
                    return Err(err(format!(
                        "Modifier '{}' conservatively intersects sibling volume '{}': Bambu applies modifiers across an object's volumes; restrict this body-local zone or change the shared CAD grouping",
                        modifier.name, sibling.name
                    )));
                }
            }
            let parent_uuid = parent
                .target_uuid
                .as_ref()
                .ok_or_else(|| err("Modifier parent needs its persistent volume UUID"))?;
            let uuid = volume_uuid(&modifier.id, parent_uuid);
            let path = format!("3D/Objects/limo_modifier_{uuid}.model");
            if template.entries.contains_key(&path) {
                return fail(
                    "Generated modifier package path collides with an existing entry; inspect/rebind the template",
                );
            }
            let next = next_ids.get_mut(&object_id).unwrap();
            *next = next
                .checked_add(1)
                .ok_or_else(|| err("Native modifier resource ID overflow"))?;
            let id = *next;
            let mut effective = parent.effective_settings.clone();
            let settings = settings_map(&modifier.settings);
            effective.extend(settings.clone());
            validate_effective(&effective)?;
            template.entries.insert(path.clone(), format!("<model xmlns=\"{CORE_NS}\" unit=\"millimeter\"><resources><object id=\"{id}\" type=\"model\">{}</object></resources></model>", mesh_xml(&primitive, [0.; 3])).into_bytes());
            root_additions.entry(object_id).or_default().push_str(&format!("<component xmlns:p=\"{PRODUCTION_NS}\" p:path=\"/{path}\" objectid=\"{id}\" transform=\"{}\"/>", local.standard()));
            let settings_xml: String = settings
                .iter()
                .map(|(key, value)| {
                    format!(
                        "<metadata key=\"{}\" value=\"{}\"/>",
                        escape(key),
                        escape(value)
                    )
                })
                .collect();
            config_additions.entry(object_id).or_default().push_str(&format!("<part id=\"{id}\" subtype=\"modifier_part\" uuid=\"{uuid}\"><metadata key=\"name\" value=\"{}\"/>{settings_xml}<mesh_stat face_count=\"{}\"/></part>", escape(&modifier.name), primitive.triangle_count()));
            relationship_additions.push_str(&format!("<Relationship Id=\"limo-modifier-{uuid}\" Target=\"/{path}\" Type=\"http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel\"/>"));
            references.push(limo_cad_core::BambuRefreshModifier {
                modifier: modifier.clone(),
                parent_volume_uuid: parent_uuid.clone(),
                target_uuid: uuid.clone(),
                source_mesh_center_mm: center,
            });
            expected.push((object_id, id, primitive.clone(), local));
            for parent in parents.iter().filter(|parent| {
                parent.binding.object_id == object_id && parent.binding.part_id == parent_id
            }) {
                let build_range = &template.build_ranges[&(object_id, parent.binding.instance_id)];
                let item = root
                    .descendants()
                    .find(|node| node.range() == *build_range)
                    .unwrap();
                let world = Matrix::parse(item.attribute("transform"))?.compose(local);
                let (min_mm, max_mm) = bounds(
                    primitive
                        .positions
                        .as_chunks::<3>()
                        .0
                        .iter()
                        .map(|point| transform(world, *point)),
                );
                let mut sources = parent.effective_sources.clone();
                sources.extend(
                    settings
                        .keys()
                        .map(|key| (key.clone(), BambuSettingOrigin::CadModifier)),
                );
                report.instances.push(BambuModifierInstanceReport {
                    parent_binding: parent.binding.clone(),
                    target_uuid: uuid.clone(),
                    plate_index: parent.plate_index,
                    world_transform: world.standard_values(),
                    world_bounds: limo_cad_core::PrintModifierBoundsDto { min_mm, max_mm },
                    effective_settings: effective.clone(),
                    effective_sources: sources,
                });
            }
        }
        reports.push(report);
    }
    let mut root_edits = Vec::new();
    let mut config_edits = Vec::new();
    for (object_id, addition) in root_additions {
        let node = root
            .descendants()
            .find(|node| {
                node.has_tag_name((CORE_NS, "object"))
                    && node.attribute("id") == Some(object_id.to_string().as_str())
            })
            .unwrap();
        let components = node
            .children()
            .find(|node| node.has_tag_name((CORE_NS, "components")))
            .unwrap();
        let at = close_position(&root_source, components)?;
        root_edits.push((at..at, addition));
    }
    for (object_id, addition) in config_additions {
        let node = config
            .root_element()
            .children()
            .find(|node| {
                node.has_tag_name("object")
                    && node.attribute("id") == Some(object_id.to_string().as_str())
            })
            .unwrap();
        let at = close_position(&config_source, node)?;
        config_edits.push((at..at, addition));
    }
    template.entries.insert(
        ROOT.into(),
        apply_edits(&root_source, root_edits)?.into_bytes(),
    );
    template.entries.insert(
        CONFIG.into(),
        apply_edits(&config_source, config_edits)?.into_bytes(),
    );
    if !relationship_additions.is_empty() {
        let at = close_position(&relations_source, relations.root_element())?;
        template.entries.insert(
            relations_path.into(),
            apply_edits(&relations_source, vec![(at..at, relationship_additions)])?.into_bytes(),
        );
    }
    result.report.refresh_reference.modifiers = references;
    result
        .report
        .refresh_reference
        .validate()
        .map_err(ExportError)?;
    template.entries.insert(
        MANIFEST.into(),
        serde_json::to_vec_pretty(&result.report.refresh_reference).map_err(err)?,
    );
    let bytes = write_archive(&template.entries)?;
    let parsed = parse_template(&bytes)?;
    for (object, part, mesh, local) in expected {
        let readback = read_mesh(&parsed, object, part, mesh.body_id)?;
        if readback.positions != mesh.positions
            || readback.indices != mesh.indices
            || !parsed.targets[&(object, part)]
                .component_transform
                .near(local)
        {
            return fail("Modifier geometry/placement failed independent output readback");
        }
        let actual = parsed
            .summary
            .objects
            .iter()
            .find(|entry| entry.object_id == object)
            .unwrap()
            .parts
            .iter()
            .find(|entry| entry.part_id == part)
            .unwrap();
        if actual.subtype != "modifier_part" {
            return fail("Print modifier became a physical normal volume");
        }
        let reference = result
            .report
            .refresh_reference
            .modifiers
            .iter()
            .find(|reference| Some(&reference.target_uuid) == actual.uuid.as_ref())
            .ok_or_else(|| err("Modifier readback UUID changed"))?;
        if !settings_equal(
            &select_settings(&actual.settings),
            &settings_map(&reference.modifier.settings),
        ) {
            return fail("Modifier settings failed independent output readback");
        }
    }
    result.report.modifiers = reports;
    for report in result
        .report
        .modifiers
        .iter()
        .filter(|report| report.disposition != BambuModifierDisposition::Written)
    {
        result.report.warnings.push(format!(
            "Print-only modifier '{}' is {:?}; no modifier geometry was written for source body {}",
            report.name, report.disposition, report.body_id.0
        ));
    }
    result.report.output_sha256 = hash(&bytes);
    result.bytes = bytes;
    Ok(())
}

fn modifier_pose(modifier: &PrintModifierDto) -> Result<Matrix, ExportError> {
    modifier.validate().map_err(ExportError)?;
    Matrix::pose(&MeshInstance {
        body_id: modifier.body_id,
        occurrence_id: 1,
        translation: modifier.local_pose.translation_mm,
        rotation: modifier.local_pose.rotation,
        visible: true,
    })
}

fn transform(matrix: Matrix, point: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|axis| {
        (0..3)
            .map(|i| matrix.0[axis * 4 + i] * point[i])
            .sum::<f64>()
            + matrix.0[axis * 4 + 3]
    })
}

fn primitive_mesh(modifier: &PrintModifierDto) -> Result<TriangleMesh, ExportError> {
    modifier.validate().map_err(ExportError)?;
    let (positions, indices) = match modifier.primitive {
        PrintModifierPrimitiveDto::Box { size_mm } => {
            let corners = [
                [-1., -1., -1.],
                [1., -1., -1.],
                [1., 1., -1.],
                [-1., 1., -1.],
                [-1., -1., 1.],
                [1., -1., 1.],
                [1., 1., 1.],
                [-1., 1., 1.],
            ];
            (
                corners
                    .into_iter()
                    .flat_map(|point| {
                        std::array::from_fn::<_, 3, _>(|axis| point[axis] * size_mm[axis] * 0.5)
                    })
                    .collect(),
                vec![
                    0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 3, 7, 6, 3, 6, 2, 0, 4,
                    7, 0, 7, 3, 1, 2, 6, 1, 6, 5,
                ],
            )
        }
        PrintModifierPrimitiveDto::Cylinder {
            radius_mm,
            height_mm,
        } => {
            let deviation_mm = 0.025;
            let angle = (1. - deviation_mm / radius_mm).clamp(-1., 1.).acos();
            let sides = (std::f64::consts::PI / angle).ceil().max(32.) as usize;
            if sides > 2048 {
                return fail(
                    "Cylinder modifier needs more than 2048 sides to meet 0.025 mm chord deviation; reduce its radius",
                );
            }
            let mut positions = Vec::with_capacity((2 * sides + 2) * 3);
            for z in [-height_mm * 0.5, height_mm * 0.5] {
                for index in 0..sides {
                    let angle = index as f64 * std::f64::consts::TAU / sides as f64;
                    positions.extend([radius_mm * angle.cos(), radius_mm * angle.sin(), z]);
                }
            }
            positions.extend([0., 0., -height_mm * 0.5, 0., 0., height_mm * 0.5]);
            let mut indices = Vec::with_capacity(sides * 12);
            for index in 0..sides as u32 {
                let next = (index + 1) % sides as u32;
                let top = sides as u32;
                indices.extend([
                    index,
                    next,
                    next + top,
                    index,
                    next + top,
                    index + top,
                    2 * top,
                    next,
                    index,
                    2 * top + 1,
                    index + top,
                    next + top,
                ]);
            }
            (positions, indices)
        }
    };
    let mesh = TriangleMesh {
        body_id: modifier.body_id,
        name: modifier.name.clone(),
        positions,
        indices,
    };
    let welded = weld_triangle_mesh(&mesh, DEFAULT_WELD_EPSILON)?;
    validate_3mf_model_mesh(&welded)?;
    Ok(welded)
}

fn subtract(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|axis| left[axis] - right[axis])
}
fn dot(left: [f64; 3], right: [f64; 3]) -> f64 {
    left.into_iter().zip(right).map(|(a, b)| a * b).sum()
}
fn cross(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [
        left[1] * right[2] - left[2] * right[1],
        left[2] * right[0] - left[0] * right[2],
        left[0] * right[1] - left[1] * right[0],
    ]
}
fn triangle(mesh: &TriangleMesh, indices: &[u32; 3]) -> [[f64; 3]; 3] {
    indices.map(|index| {
        let start = index as usize * 3;
        std::array::from_fn(|axis| mesh.positions[start + axis])
    })
}
fn ray_triangle(origin: [f64; 3], direction: [f64; 3], triangle: [[f64; 3]; 3]) -> Option<f64> {
    let edge1 = subtract(triangle[1], triangle[0]);
    let edge2 = subtract(triangle[2], triangle[0]);
    let p = cross(direction, edge2);
    let determinant = dot(edge1, p);
    if determinant.abs() < INTERSECTION_EPSILON {
        return None;
    }
    let inverse = 1. / determinant;
    let offset = subtract(origin, triangle[0]);
    let u = dot(offset, p) * inverse;
    if !(-INTERSECTION_EPSILON..=1. + INTERSECTION_EPSILON).contains(&u) {
        return None;
    }
    let q = cross(offset, edge1);
    let v = dot(direction, q) * inverse;
    if v < -INTERSECTION_EPSILON || u + v > 1. + INTERSECTION_EPSILON {
        return None;
    }
    Some(dot(edge2, q) * inverse)
}
fn on_triangle(point: [f64; 3], triangle: [[f64; 3]; 3]) -> bool {
    let edge1 = subtract(triangle[1], triangle[0]);
    let edge2 = subtract(triangle[2], triangle[0]);
    let offset = subtract(point, triangle[0]);
    let normal = cross(edge1, edge2);
    let length = dot(normal, normal).sqrt();
    if length == 0. || dot(offset, normal).abs() > INTERSECTION_EPSILON * length {
        return false;
    }
    let a = dot(edge1, edge1);
    let b = dot(edge1, edge2);
    let c = dot(edge2, edge2);
    let d = dot(offset, edge1);
    let e = dot(offset, edge2);
    let denominator = a * c - b * b;
    if denominator.abs() < f64::EPSILON {
        return false;
    }
    let u = (c * d - b * e) / denominator;
    let v = (a * e - b * d) / denominator;
    u >= -INTERSECTION_EPSILON && v >= -INTERSECTION_EPSILON && u + v <= 1. + INTERSECTION_EPSILON
}
fn point_in_mesh(point: [f64; 3], mesh: &TriangleMesh) -> bool {
    if mesh
        .indices
        .as_chunks::<3>()
        .0
        .iter()
        .any(|indices| on_triangle(point, triangle(mesh, indices)))
    {
        return false;
    }
    let directions = [[1., 0.137, 0.073], [0.093, 1., 0.217], [0.171, 0.057, 1.]];
    directions
        .into_iter()
        .filter(|direction| {
            let mut intersections: Vec<_> = mesh
                .indices
                .as_chunks::<3>()
                .0
                .iter()
                .filter_map(|indices| ray_triangle(point, *direction, triangle(mesh, indices)))
                .filter(|distance| *distance > INTERSECTION_EPSILON)
                .collect();
            intersections.sort_by(f64::total_cmp);
            intersections.dedup_by(|a, b| (*a - *b).abs() < INTERSECTION_EPSILON);
            intersections.len() % 2 == 1
        })
        .count()
        >= 2
}
fn inside_primitive(point: [f64; 3], primitive: &PrintModifierPrimitiveDto) -> bool {
    match primitive {
        PrintModifierPrimitiveDto::Box { size_mm } => point
            .into_iter()
            .zip(*size_mm)
            .all(|(coordinate, size)| coordinate.abs() < size * 0.5 - INTERSECTION_EPSILON),
        PrintModifierPrimitiveDto::Cylinder {
            radius_mm,
            height_mm,
        } => {
            point[0].hypot(point[1]) < *radius_mm - INTERSECTION_EPSILON
                && point[2].abs() < *height_mm * 0.5 - INTERSECTION_EPSILON
        }
    }
}
fn bounds(points: impl IntoIterator<Item = [f64; 3]>) -> ([f64; 3], [f64; 3]) {
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for point in points {
        for axis in 0..3 {
            low[axis] = low[axis].min(point[axis]);
            high[axis] = high[axis].max(point[axis]);
        }
    }
    (low, high)
}
fn bounds_overlap(left: ([f64; 3], [f64; 3]), right: ([f64; 3], [f64; 3])) -> bool {
    (0..3).all(|axis| {
        left.0[axis] < right.1[axis] - INTERSECTION_EPSILON
            && right.0[axis] < left.1[axis] - INTERSECTION_EPSILON
    })
}
fn segment_crossing_samples(
    start: [f64; 3],
    end: [f64; 3],
    triangle: [[f64; 3]; 3],
) -> Option<[[f64; 3]; 2]> {
    let direction = subtract(end, start);
    let fraction = ray_triangle(start, direction, triangle)?;
    if fraction <= INTERSECTION_EPSILON || fraction >= 1. - INTERSECTION_EPSILON {
        return None;
    }
    let delta = (0.00001 / dot(direction, direction).sqrt())
        .min(fraction * 0.5)
        .min((1. - fraction) * 0.5);
    Some(
        [fraction - delta, fraction + delta]
            .map(|sample| std::array::from_fn(|axis| start[axis] + direction[axis] * sample)),
    )
}
fn meshes_cross(left: &TriangleMesh, right: &TriangleMesh, right_pose: Matrix) -> bool {
    let inverse = right_pose.inverse().expect("validated modifier rigid pose");
    let right_triangles: Vec<_> = right
        .indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|indices| triangle(right, indices).map(|point| transform(right_pose, point)))
        .map(|triangle| (triangle, bounds(triangle)))
        .collect();
    for indices in left.indices.as_chunks::<3>().0 {
        let a = triangle(left, indices);
        let a_bounds = bounds(a);
        for (b, b_bounds) in &right_triangles {
            // Triangle bounds have zero extent along their plane, so the solid AABB
            // strict-overlap predicate would discard real surface crossings.
            if !(0..3).all(|axis| {
                a_bounds.0[axis] <= b_bounds.1[axis] + INTERSECTION_EPSILON
                    && b_bounds.0[axis] <= a_bounds.1[axis] + INTERSECTION_EPSILON
            }) {
                continue;
            }
            for edge in 0..3 {
                if segment_crossing_samples(a[edge], a[(edge + 1) % 3], *b).is_some_and(|samples| {
                    samples
                        .into_iter()
                        .any(|sample| point_in_mesh(transform(inverse, sample), right))
                }) || segment_crossing_samples(b[edge], b[(edge + 1) % 3], a).is_some_and(
                    |samples| {
                        samples
                            .into_iter()
                            .any(|sample| point_in_mesh(sample, left))
                    },
                ) {
                    return true;
                }
            }
        }
    }
    false
}

fn intersects_parent(
    parent: &TriangleMesh,
    modifier: &PrintModifierDto,
    primitive: &TriangleMesh,
) -> Result<bool, ExportError> {
    let pose = modifier_pose(modifier)?;
    let inverse = pose.inverse()?;
    let parent_bounds = bounds(parent.positions.as_chunks::<3>().0.iter().copied());
    let modifier_bounds = bounds(
        primitive
            .positions
            .as_chunks::<3>()
            .0
            .iter()
            .map(|point| transform(pose, *point)),
    );
    if !bounds_overlap(parent_bounds, modifier_bounds) {
        return Ok(false);
    }
    if parent
        .positions
        .as_chunks::<3>()
        .0
        .iter()
        .any(|point| inside_primitive(transform(inverse, *point), &modifier.primitive))
    {
        return Ok(true);
    }
    if point_in_mesh(modifier.local_pose.translation_mm, parent) {
        return Ok(true);
    }
    if parent
        .triangle_count()
        .saturating_mul(primitive.triangle_count())
        > 25_000_000
    {
        return fail(
            "Modifier intersection exceeds the bounded mesh inspection budget; use a simpler zone or a coarser target tessellation and review again",
        );
    }
    if primitive
        .positions
        .as_chunks::<3>()
        .0
        .iter()
        .any(|point| point_in_mesh(transform(pose, *point), parent))
    {
        return Ok(true);
    }
    Ok(meshes_cross(parent, primitive, pose))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn zone() -> PrintModifierDto {
        PrintModifierDto {
            id: "01234567-89ab-4cde-8123-456789abcdef".into(),
            name: "Local reinforcement".into(),
            body_id: BodyId(1),
            enabled: true,
            local_pose: limo_cad_core::PrintLocalPoseDto {
                translation_mm: [5., 5., 5.],
                ..Default::default()
            },
            primitive: PrintModifierPrimitiveDto::Box {
                size_mm: [2., 2., 2.],
            },
            settings: PrintSettingsDto {
                wall_count: Some(6),
                ..Default::default()
            },
        }
    }
    #[test]
    fn box_and_cylinder_are_closed_print_only_primitives_and_touching_is_not_intersection() {
        let parent = super::super::tests::cube(1);
        let mut zone = zone();
        let primitive = primitive_mesh(&zone).unwrap();
        assert_eq!(primitive.triangle_count(), 12);
        assert!(intersects_parent(&parent, &zone, &primitive).unwrap());
        zone.local_pose.translation_mm = [11., 5., 5.];
        assert!(!intersects_parent(&parent, &zone, &primitive).unwrap());
        zone.local_pose.translation_mm = [10.5, 5., 5.];
        assert!(intersects_parent(&parent, &zone, &primitive).unwrap());
        zone.primitive = PrintModifierPrimitiveDto::Cylinder {
            radius_mm: 2.,
            height_mm: 6.,
        };
        zone.local_pose.translation_mm = [5., 5., 5.];
        let cylinder = primitive_mesh(&zone).unwrap();
        assert!(cylinder.triangle_count() >= 128);
        assert!(intersects_parent(&parent, &zone, &cylinder).unwrap());
    }
    #[test]
    fn a_modifier_inside_a_closed_cavity_is_rejected_despite_overlapping_bounds() {
        let mut hollow = super::super::tests::cube(1);
        let mut inner = super::super::tests::cube(1);
        for point in inner.positions.as_chunks_mut::<3>().0 {
            for coordinate in point {
                *coordinate = *coordinate * 0.4 + 3.;
            }
        }
        let offset = hollow.positions.len() as u32 / 3;
        hollow.positions.extend(inner.positions);
        for triangle in inner.indices.as_chunks::<3>().0 {
            hollow.indices.extend([
                triangle[0] + offset,
                triangle[2] + offset,
                triangle[1] + offset,
            ]);
        }
        validate_3mf_model_mesh(&hollow).unwrap();
        let modifier = zone();
        assert!(
            !intersects_parent(&hollow, &modifier, &primitive_mesh(&modifier).unwrap()).unwrap()
        );
    }

    #[test]
    fn print_modifier_writer_preserves_repeats_and_compounds_local_rotated_pose_in_both_placements()
    {
        for placement in [
            BambuPlacementMode::Template,
            BambuPlacementMode::ResolvedScene,
        ] {
            let (bytes, meshes, appearances, instances, structure, mut intent, mut request) =
                super::super::tests::fixture();
            let mut modifier = zone();
            modifier.primitive = PrintModifierPrimitiveDto::Box {
                size_mm: [2., 4., 6.],
            };
            modifier.local_pose.translation_mm = [4., 5., 6.];
            modifier.local_pose.rotation = [
                0.,
                0.,
                std::f64::consts::FRAC_1_SQRT_2,
                std::f64::consts::FRAC_1_SQRT_2,
            ];
            intent.modifiers.push(modifier.clone());
            request.placement = placement;
            let result = write_bambu_project(
                &bytes,
                &meshes,
                &appearances,
                &instances,
                &structure,
                &intent,
                &request,
            )
            .unwrap();
            assert_eq!(result.report.parts.len(), 4);
            assert_eq!(result.report.modifiers.len(), 1);
            let report = &result.report.modifiers[0];
            assert_eq!(report.disposition, BambuModifierDisposition::Written);
            assert_eq!(report.instances.len(), 2);
            assert_eq!(
                report
                    .instances
                    .iter()
                    .map(|instance| instance.parent_binding.occurrence_id)
                    .collect::<BTreeSet<_>>(),
                BTreeSet::from([11, 21])
            );
            assert_eq!(
                report.instances[0].target_uuid,
                report.instances[1].target_uuid
            );
            assert_eq!(result.report.refresh_reference.modifiers.len(), 1);
            let parsed = parse_template(&result.bytes).unwrap();
            assert_eq!(parsed.summary.objects.len(), 1);
            assert_eq!(parsed.summary.objects[0].instance_count, 2);
            assert_eq!(
                parsed.summary.objects[0]
                    .parts
                    .iter()
                    .filter(|part| part.subtype == "normal_part")
                    .count(),
                2
            );
            assert_eq!(
                parsed.summary.objects[0]
                    .parts
                    .iter()
                    .filter(|part| part.subtype == "modifier_part")
                    .count(),
                1
            );
            let local = modifier_pose(&modifier).unwrap();
            let center = if placement == BambuPlacementMode::Template {
                mesh_center(&meshes[0]).map(|value| -value)
            } else {
                [0.; 3]
            };
            for instance in &report.instances {
                let parent = result
                    .report
                    .parts
                    .iter()
                    .find(|parent| parent.binding == instance.parent_binding)
                    .unwrap();
                let parent_world = Matrix::parse(Some(
                    &parent
                        .world_transform
                        .into_iter()
                        .map(|value| value.to_string())
                        .collect::<Vec<_>>()
                        .join(" "),
                ))
                .unwrap();
                let world = parent_world.compose(translation(center)).compose(local);
                let primitive = primitive_mesh(&modifier).unwrap();
                let (low, high) = bounds(
                    primitive
                        .positions
                        .as_chunks::<3>()
                        .0
                        .iter()
                        .map(|point| transform(world, *point)),
                );
                for (actual, expected) in instance
                    .world_bounds
                    .min_mm
                    .into_iter()
                    .chain(instance.world_bounds.max_mm)
                    .zip(low.into_iter().chain(high))
                {
                    assert!((actual - expected).abs() < 1e-7);
                }
                assert_eq!(
                    instance.effective_sources["wall_loops"],
                    BambuSettingOrigin::CadModifier
                );
                assert_eq!(instance.effective_settings["wall_loops"], "6");
            }
            request.bindings.clear();
            request.refresh_reference = Some(result.report.refresh_reference.clone());
            let refreshed = write_bambu_project(
                &result.bytes,
                &meshes,
                &appearances,
                &instances,
                &structure,
                &intent,
                &request,
            )
            .unwrap();
            assert_eq!(refreshed.report.modifiers, result.report.modifiers);
            let parsed = parse_template(&refreshed.bytes).unwrap();
            assert_eq!(parsed.summary.objects[0].parts.len(), 3);
            intent.modifiers.clear();
            request.refresh_reference = Some(refreshed.report.refresh_reference);
            let removed = write_bambu_project(
                &refreshed.bytes,
                &meshes,
                &appearances,
                &instances,
                &structure,
                &intent,
                &request,
            )
            .unwrap();
            assert!(removed.report.modifiers.is_empty());
            assert!(removed.report.refresh_reference.modifiers.is_empty());
            assert_eq!(
                parse_template(&removed.bytes).unwrap().summary.objects[0]
                    .parts
                    .len(),
                2
            );
        }
    }

    #[test]
    fn named_layout_offsets_compose_through_nested_groups_before_local_modifiers() {
        use limo_cad_assembly::{
            resolve_view_layout, AssemblySolutionDto, ComponentId, ComponentOccurrenceDto,
            InstanceBodyPoseDto, OccurrenceId, OccurrencePoseDto, ViewOccurrenceOffsetDto,
        };
        let (bytes, meshes, appearances, original, mut structure, mut intent, request) =
            super::super::tests::fixture();
        let half = std::f64::consts::FRAC_1_SQRT_2;
        for (root, intermediate) in [(100, 101), (200, 201)] {
            for occurrence in &mut structure.occurrences {
                if occurrence.parent_occurrence_id == Some(OccurrenceId(root)) {
                    occurrence.parent_occurrence_id = Some(OccurrenceId(intermediate));
                }
            }
            structure.occurrences.push(ComponentOccurrenceDto {
                id: OccurrenceId(intermediate),
                name: format!("Nested {root}"),
                component_id: ComponentId(1),
                parent_occurrence_id: Some(OccurrenceId(root)),
                local_pose: Default::default(),
                visible: true,
                grounded: false,
            });
        }
        structure.next_occurrence_id = 202;
        let mut solution = AssemblySolutionDto {
            solved: true,
            ..Default::default()
        };
        for occurrence in &structure.occurrences {
            let parent = original
                .iter()
                .find(|instance| instance.occurrence_id == occurrence.id.0)
                .unwrap_or_else(|| {
                    if occurrence.id.0 < 200 {
                        &original[0]
                    } else {
                        &original[2]
                    }
                });
            solution.occurrence_poses.push(OccurrencePoseDto {
                occurrence_id: occurrence.id,
                component_id: occurrence.component_id,
                translation: parent.translation,
                rotation: parent.rotation,
            });
        }
        solution.instance_body_poses = original
            .iter()
            .map(|instance| InstanceBodyPoseDto {
                occurrence_id: OccurrenceId(instance.occurrence_id),
                component_id: if instance.body_id == BodyId(1) {
                    ComponentId(2)
                } else {
                    ComponentId(3)
                },
                body_id: instance.body_id,
                translation: instance.translation,
                rotation: instance.rotation,
                visible: instance.visible,
            })
            .collect();
        let layout = resolve_view_layout(
            &structure,
            &solution,
            &[
                ViewOccurrenceOffsetDto {
                    occurrence_id: OccurrenceId(100),
                    translation: [7., 9., 0.],
                    rotation: [0., 0., half, half],
                },
                ViewOccurrenceOffsetDto {
                    occurrence_id: OccurrenceId(101),
                    translation: [3., 4., 0.],
                    rotation: [0., 0., 0., 1.],
                },
            ],
        )
        .unwrap();
        let instances: Vec<_> = layout
            .instance_body_poses
            .iter()
            .map(|pose| MeshInstance {
                body_id: pose.body_id,
                occurrence_id: pose.occurrence_id.0,
                translation: pose.translation,
                rotation: pose.rotation,
                visible: pose.visible,
            })
            .collect();
        intent.modifiers.push(zone());
        let output = write_bambu_project(
            &bytes,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request,
        )
        .unwrap();
        let target = read_bambu_volume_geometry(&output.bytes).unwrap();
        let first = target
            .iter()
            .find(|volume| volume.subtype == "modifier_part" && volume.instance_id == 0)
            .unwrap();
        let repeated = target
            .iter()
            .find(|volume| volume.subtype == "modifier_part" && volume.instance_id == 1)
            .unwrap();
        // The first body's origin moved [30,40] -> [40,53] and rotated 180 degrees;
        // local zone center [5,5,5] therefore becomes [35,48,5]. Second repeat is unchanged.
        for (actual, expected) in [(first, [35., 48., 5.]), (repeated, [85., 75., 5.])] {
            for (axis, coordinate) in expected.into_iter().enumerate() {
                assert!(
                    ((actual.world_bounds.min_mm[axis] + actual.world_bounds.max_mm[axis]) / 2.
                        - coordinate)
                        .abs()
                        < 1e-6
                );
            }
        }
        assert_eq!(solution.instance_body_poses[0].translation, [30., 40., 0.]);
        assert_eq!(output.report.modifiers[0].instances.len(), 2);
    }
    #[test]
    fn print_modifier_writer_rejects_empty_cross_sibling_and_foreign_edits_and_reports_no_effect() {
        let (bytes, meshes, appearances, instances, structure, mut intent, mut request) =
            super::super::tests::fixture();
        let mut modifier = zone();
        modifier.local_pose.translation_mm = [40., 5., 5.];
        intent.modifiers.push(modifier.clone());
        assert!(write_bambu_project(
            &bytes,
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
        .contains("does not intersect"));
        intent.modifiers[0].local_pose.translation_mm = [8., 5., 5.];
        intent.modifiers[0].primitive = PrintModifierPrimitiveDto::Box {
            size_mm: [50., 4., 4.],
        };
        assert!(write_bambu_project(
            &bytes,
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
        .contains("sibling"));
        intent.modifiers[0] = zone();
        let result = write_bambu_project(
            &bytes,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request,
        )
        .unwrap();
        let mut entries = archive(&result.bytes).unwrap();
        let config = text(&entries, CONFIG).unwrap().replace(
            "key=\"wall_loops\" value=\"6\"",
            "key=\"wall_loops\" value=\"8\"",
        );
        entries.insert(CONFIG.into(), config.into_bytes());
        request.bindings.clear();
        request.refresh_reference = Some(result.report.refresh_reference);
        let changed = write_archive(&entries).unwrap();
        assert!(write_bambu_project(
            &changed,
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
        .contains("Native settings on modifier"));
        intent.modifiers[0].settings = Default::default();
        request.refresh_reference = None;
        request.bindings = super::super::tests::fixture().6.bindings;
        let reset = write_bambu_project(
            &bytes,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request,
        )
        .unwrap();
        assert_eq!(
            reset.report.modifiers[0].disposition,
            BambuModifierDisposition::NoEffect
        );
        assert!(reset.report.refresh_reference.modifiers.is_empty());
        intent.modifiers[0].enabled = false;
        let disabled = write_bambu_project(
            &bytes,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request,
        )
        .unwrap();
        assert_eq!(
            disabled.report.modifiers[0].disposition,
            BambuModifierDisposition::Disabled
        );
    }

    #[test]
    #[ignore = "requires LIMO_BAMBU_TEMPLATE complete profile and a fresh owned LIMO_BAMBU_QUALIFICATION_DIR; native slice must be run separately"]
    fn write_native_print_modifier_qualification_fixtures() {
        let input =
            std::env::var_os("LIMO_BAMBU_TEMPLATE").expect("operator complete Bambu template");
        let directory = std::path::PathBuf::from(
            std::env::var_os("LIMO_BAMBU_QUALIFICATION_DIR").expect("fresh owned output"),
        );
        assert!(directory.is_absolute() && directory.is_dir());
        let original = std::fs::read(&input).unwrap();
        let original_hash = hash(&original);
        let complete = parse_template(&original).unwrap();
        let (fixture, mut meshes, mut appearances, mut instances, structure, mut intent, request) =
            super::super::tests::fixture();
        let mut entries = archive(&fixture).unwrap();
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
        for point in meshes[0].positions.as_chunks_mut::<3>().0 {
            point[0] *= 3.;
            point[1] *= 3.;
            point[2] *= 2.;
        }
        instances[1].translation = [30., 80., 0.];
        instances[3].translation = [120., 70., 0.];
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
        let mut box_zone = zone();
        box_zone.name = "Rotated box local 80 percent".into();
        box_zone.local_pose.translation_mm = [7., 15., 10.];
        let angle = 15_f64.to_radians() * 0.5;
        box_zone.local_pose.rotation = [0., 0., angle.sin(), angle.cos()];
        box_zone.primitive = PrintModifierPrimitiveDto::Box {
            size_mm: [10., 24., 8.],
        };
        box_zone.settings = PrintSettingsDto {
            wall_count: Some(6),
            infill_density_percent: Some(80.),
            ..Default::default()
        };
        let mut cylinder = box_zone.clone();
        cylinder.id = "abcdef01-89ab-4cde-8123-456789abcdef".into();
        cylinder.name = "Tilted cylinder local solid".into();
        cylinder.local_pose.translation_mm = [23., 15., 10.];
        let angle = 20_f64.to_radians() * 0.5;
        cylinder.local_pose.rotation = [0., angle.sin(), 0., angle.cos()];
        cylinder.primitive = PrintModifierPrimitiveDto::Cylinder {
            radius_mm: 5.,
            height_mm: 8.,
        };
        cylinder.settings.infill_density_percent = Some(100.);
        cylinder.settings.infill_pattern = Some(InfillPatternDto::Rectilinear);
        intent.modifiers = vec![box_zone, cylinder];
        let configured = write_bambu_project(
            &template,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request,
        )
        .unwrap();
        for (name, output) in [("baseline", baseline), ("configured", configured)] {
            let mut file = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(directory.join(format!("modifier-{name}.3mf")))
                .unwrap();
            file.write_all(&output.bytes).unwrap();
            let evidence = serde_json::json!({"original_template_sha256":original_hash,"synthetic_geometry":true,"report":output.report});
            let mut file = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(directory.join(format!("modifier-{name}.json")))
                .unwrap();
            file.write_all(&serde_json::to_vec_pretty(&evidence).unwrap())
                .unwrap();
        }
        // A separate explicitly single-instance scene qualifies automatic native refresh;
        // repeated same-object instances retain the documented native identify-id limitation.
        let mut unique_entries = entries.clone();
        let unique_source = text(&unique_entries, ROOT).unwrap().to_owned();
        let unique_root = xml(&unique_source).unwrap();
        let items: Vec<_> = unique_root
            .descendants()
            .filter(|node| node.has_tag_name((CORE_NS, "item")))
            .collect();
        unique_entries.insert(
            ROOT.into(),
            apply_edits(&unique_source, vec![(items[1].range(), String::new())])
                .unwrap()
                .into_bytes(),
        );
        let unique_config = text(&unique_entries, CONFIG).unwrap().to_owned();
        let document = xml(&unique_config).unwrap();
        let removed = document
            .descendants()
            .filter(|node| node.has_tag_name("model_instance"))
            .find(|node| {
                metadata(*node)
                    .unwrap()
                    .get("instance_id")
                    .map(String::as_str)
                    == Some("1")
            })
            .unwrap();
        unique_entries.insert(
            CONFIG.into(),
            apply_edits(&unique_config, vec![(removed.range(), String::new())])
                .unwrap()
                .into_bytes(),
        );
        let unique_template = write_archive(&unique_entries).unwrap();
        let mut unique_request = request.clone();
        unique_request
            .bindings
            .retain(|binding| binding.instance_id == 0);
        let unique_instances: Vec<_> = instances
            .iter()
            .filter(|instance| instance.occurrence_id < 20)
            .map(|instance| MeshInstance {
                body_id: instance.body_id,
                occurrence_id: instance.occurrence_id,
                translation: instance.translation,
                rotation: instance.rotation,
                visible: instance.visible,
            })
            .collect();
        let mut unique_intent = intent.clone();
        for (name, enabled) in [("unique-baseline", false), ("unique-configured", true)] {
            unique_intent
                .modifiers
                .iter_mut()
                .for_each(|modifier| modifier.enabled = enabled);
            let output = write_bambu_project(
                &unique_template,
                &meshes,
                &appearances,
                &unique_instances,
                &structure,
                &unique_intent,
                &unique_request,
            )
            .unwrap();
            let mut file = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(directory.join(format!("modifier-{name}.3mf")))
                .unwrap();
            file.write_all(&output.bytes).unwrap();
            let mut file = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(directory.join(format!("modifier-{name}.json")))
                .unwrap();
            file.write_all(&serde_json::to_vec_pretty(&serde_json::json!({"original_template_sha256":original_hash,"synthetic_geometry":true,"report":output.report})).unwrap()).unwrap();
        }
        if let Some(native_input) = std::env::var_os("LIMO_BAMBU_ROUNDTRIP_TEMPLATE") {
            let native = std::fs::read(native_input).unwrap();
            let configured: Value = serde_json::from_slice(
                &std::fs::read(directory.join("modifier-configured.json")).unwrap(),
            )
            .unwrap();
            let reference: BambuRefreshReference =
                serde_json::from_value(configured["report"]["refresh_reference"].clone()).unwrap();
            verify_bambu_modifier_reference(&native, &reference).unwrap();
        }
        assert_eq!(hash(&std::fs::read(input).unwrap()), original_hash);
    }

    #[test]
    #[ignore = "requires completed native modifier slices in LIMO_BAMBU_QUALIFICATION_DIR; writes a fresh unsliced unique-instance refresh"]
    fn verify_native_print_modifier_geometry_settings_and_automatic_refresh() {
        let directory = std::path::PathBuf::from(
            std::env::var_os("LIMO_BAMBU_QUALIFICATION_DIR")
                .expect("owned completed qualification"),
        );
        let mut evidence = Vec::new();
        for name in ["configured", "unique-configured"] {
            let report: Value = serde_json::from_slice(
                &std::fs::read(directory.join(format!("modifier-{name}.json"))).unwrap(),
            )
            .unwrap();
            let reference: BambuRefreshReference =
                serde_json::from_value(report["report"]["refresh_reference"].clone()).unwrap();
            let written = std::fs::read(directory.join(format!("modifier-{name}.3mf"))).unwrap();
            let native =
                std::fs::read(directory.join(format!("{name}-validated/{name}-sliced.3mf")))
                    .unwrap();
            verify_bambu_modifier_reference(&native, &reference).unwrap();
            let before = parse_template(&written).unwrap();
            let after = parse_template(&native).unwrap();
            assert_eq!(before.summary.objects.len(), after.summary.objects.len());
            assert_eq!(
                before.summary.objects[0].instance_count,
                after.summary.objects[0].instance_count
            );
            assert_eq!(
                after.summary.objects[0]
                    .parts
                    .iter()
                    .filter(|part| part.subtype == "normal_part")
                    .count(),
                2
            );
            assert_eq!(
                after.summary.objects[0]
                    .parts
                    .iter()
                    .filter(|part| part.subtype == "modifier_part")
                    .count(),
                2
            );
            assert_eq!(
                before.summary.filament_settings_ids,
                after.summary.filament_settings_ids
            );
            assert_eq!(
                before.summary.support_interface_filament,
                after.summary.support_interface_filament
            );
            let before_root = xml(text(&before.entries, ROOT).unwrap()).unwrap();
            let after_root = xml(text(&after.entries, ROOT).unwrap()).unwrap();
            for original in &before.summary.objects[0].parts {
                let actual = after.summary.objects[0]
                    .parts
                    .iter()
                    .find(|part| part.uuid == original.uuid)
                    .unwrap();
                let original_mesh = read_mesh(
                    &before,
                    before.summary.objects[0].object_id,
                    original.part_id,
                    BodyId(1),
                )
                .unwrap();
                let actual_mesh = read_mesh(
                    &after,
                    after.summary.objects[0].object_id,
                    actual.part_id,
                    BodyId(1),
                )
                .unwrap();
                for index in 0..before.summary.objects[0].instance_count {
                    let old_item = before_root
                        .descendants()
                        .find(|node| {
                            node.range()
                                == before.build_ranges
                                    [&(before.summary.objects[0].object_id, index)]
                        })
                        .unwrap();
                    let new_item = after_root
                        .descendants()
                        .find(|node| {
                            node.range()
                                == after.build_ranges[&(after.summary.objects[0].object_id, index)]
                        })
                        .unwrap();
                    let old_world = Matrix::parse(old_item.attribute("transform"))
                        .unwrap()
                        .compose(
                            before.targets
                                [&(before.summary.objects[0].object_id, original.part_id)]
                                .component_transform,
                        );
                    let new_world = Matrix::parse(new_item.attribute("transform"))
                        .unwrap()
                        .compose(
                            after.targets[&(after.summary.objects[0].object_id, actual.part_id)]
                                .component_transform,
                        );
                    assert!(
                        same_geometry(&original_mesh, old_world, &actual_mesh, new_world),
                        "Native geometry/placement changed for {} instance{}",
                        original.name,
                        index
                    );
                }
            }
            if name == "unique-configured" {
                let (
                    _,
                    mut meshes,
                    mut appearances,
                    mut instances,
                    structure,
                    mut intent,
                    mut request,
                ) = super::super::tests::fixture();
                for point in meshes[0].positions.as_chunks_mut::<3>().0 {
                    point[0] *= 3.;
                    point[1] *= 3.;
                    point[2] *= 2.;
                }
                instances[1].translation = [30., 80., 0.];
                instances.retain(|instance| instance.occurrence_id < 20);
                intent.parts.clear();
                intent.modifiers = reference
                    .modifiers
                    .iter()
                    .map(|reference| reference.modifier.clone())
                    .collect();
                let color = after.summary.filament_colors[0].trim_start_matches('#');
                for appearance in &mut appearances {
                    appearance.filament_type = after.summary.filament_types[0].clone();
                    appearance.color = limo_cad_core::Rgba8::opaque(
                        u8::from_str_radix(&color[..2], 16).unwrap(),
                        u8::from_str_radix(&color[2..4], 16).unwrap(),
                        u8::from_str_radix(&color[4..6], 16).unwrap(),
                    );
                }
                request.bindings.clear();
                request.refresh_reference = Some(reference);
                let refreshed = write_bambu_project(
                    &native,
                    &meshes,
                    &appearances,
                    &instances,
                    &structure,
                    &intent,
                    &request,
                )
                .unwrap();
                let mut file = std::fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(directory.join("modifier-unique-refreshed.3mf"))
                    .unwrap();
                file.write_all(&refreshed.bytes).unwrap();
                let mut file = std::fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(directory.join("modifier-unique-refreshed.json"))
                    .unwrap();
                file.write_all(&serde_json::to_vec_pretty(&refreshed.report).unwrap())
                    .unwrap();
                assert!(refreshed
                    .report
                    .invalidated_entries
                    .iter()
                    .any(|entry| entry.ends_with(".gcode")));
            }
            evidence.push(serde_json::json!({"case":name,"written_sha256":hash(&written),"native_sha256":hash(&native),"normal_volume_count":2,"modifier_volume_count":2,"instance_count":after.summary.objects[0].instance_count,"all_world_mesh_triangles_preserved":true,"modifier_settings_uuid_and_centered_attachment_verified":true,"automatic_refresh":name=="unique-configured"}));
        }
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(directory.join("native-modifier-readback-evidence.json"))
            .unwrap();
        file.write_all(&serde_json::to_vec_pretty(&evidence).unwrap())
            .unwrap();
    }
    #[test]
    #[ignore = "requires completed automatic-refresh native slice in LIMO_BAMBU_QUALIFICATION_DIR"]
    fn verify_native_modifier_automatic_refresh_reslice() {
        let directory = std::path::PathBuf::from(
            std::env::var_os("LIMO_BAMBU_QUALIFICATION_DIR")
                .expect("owned completed qualification"),
        );
        let report: BambuProjectReport = serde_json::from_slice(
            &std::fs::read(directory.join("modifier-unique-refreshed.json")).unwrap(),
        )
        .unwrap();
        let written = std::fs::read(directory.join("modifier-unique-refreshed.3mf")).unwrap();
        let native =
            std::fs::read(directory.join("unique-refreshed-validated/unique-refreshed-sliced.3mf"))
                .unwrap();
        verify_bambu_modifier_reference(&native, &report.refresh_reference).unwrap();
        let before = read_bambu_volume_geometry(&written).unwrap();
        let after = read_bambu_volume_geometry(&native).unwrap();
        assert_eq!(before.len(), 4);
        assert_eq!(after.len(), before.len());
        for source in &before {
            let target = after
                .iter()
                .find(|part| part.target_uuid == source.target_uuid)
                .unwrap();
            assert_eq!(source.subtype, target.subtype);
            assert_eq!(source.plate_index, target.plate_index);
            let source_mesh = TriangleMesh {
                body_id: BodyId(1),
                name: source.name.clone(),
                positions: source.positions.clone(),
                indices: source.indices.clone(),
            };
            let target_mesh = TriangleMesh {
                body_id: BodyId(1),
                name: target.name.clone(),
                positions: target.positions.clone(),
                indices: target.indices.clone(),
            };
            // standard_values is native 3MF column-major; Matrix::parse is its independent reader.
            let source_world = Matrix::parse(Some(
                &source
                    .world_transform
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(" "),
            ))
            .unwrap();
            let target_world = Matrix::parse(Some(
                &target
                    .world_transform
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(" "),
            ))
            .unwrap();
            assert!(
                same_geometry(&source_mesh, source_world, &target_mesh, target_world),
                "Refreshed native geometry or placement changed for {}",
                source.name
            );
        }
        let evidence = serde_json::json!({"written_sha256":hash(&written),"native_sha256":hash(&native),"all_world_triangles_preserved":true,"normal_volume_count":2,"modifier_volume_count":2,"stable_modifier_uuid_settings_and_attachment":true});
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(directory.join("native-modifier-refresh-reslice-evidence.json"))
            .unwrap();
        file.write_all(&serde_json::to_vec_pretty(&evidence).unwrap())
            .unwrap();
    }
}
