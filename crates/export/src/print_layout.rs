//! Conservative print-layout diagnostics and non-destructive arrangement proposals.
use crate::{ExportError, TriangleMesh};
use limo_cad_assembly::{
    AssemblySolutionDto, AssemblyTransformDto, ComponentStructureDto, OccurrenceId,
};
use limo_cad_core::{BodyId, PrintBedDto};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayoutIssue {
    pub code: String,
    pub message: String,
    pub occurrence_ids: Vec<u64>,
}

pub(crate) fn bed_z_issue(
    min_z: f64,
    label: &str,
    occurrence_ids: Vec<u64>,
) -> Option<LayoutIssue> {
    if min_z < -1e-5 {
        Some(issue(
            "below_bed",
            format!("{label} extends below the bed (minimum Z {min_z:.4} mm)."),
            occurrence_ids,
        ))
    } else if min_z > 1e-5 {
        Some(issue("above_bed", format!("{label} starts above the bed (minimum Z {min_z:.4} mm); check whether other parts or slicer supports carry it."), occurrence_ids))
    } else {
        None
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutTranslation {
    pub occurrence_id: u64,
    /// Additional world-axis translation; add to the saved view's current offset.
    pub translation: [f64; 3],
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrintLayoutReport {
    pub bed: PrintBedDto,
    pub printable_instances: usize,
    pub printable_groups: usize,
    pub excluded_instances: usize,
    pub issues: Vec<LayoutIssue>,
    pub proposed_translations: Vec<LayoutTranslation>,
    pub proposal_fits: bool,
    pub clearance_mm: f64,
    pub overlap_check: String,
}
#[derive(Clone, Copy)]
struct Bounds {
    min: [f64; 3],
    max: [f64; 3],
}
impl Bounds {
    fn empty() -> Self {
        Self {
            min: [f64::INFINITY; 3],
            max: [f64::NEG_INFINITY; 3],
        }
    }
    fn add(&mut self, point: [f64; 3]) {
        for (axis, value) in point.into_iter().enumerate() {
            self.min[axis] = self.min[axis].min(value);
            self.max[axis] = self.max[axis].max(value);
        }
    }
    fn size(self) -> [f64; 3] {
        std::array::from_fn(|i| self.max[i] - self.min[i])
    }
}

pub fn analyze_print_layout(
    meshes: &[TriangleMesh],
    structure: &ComponentStructureDto,
    solution: &AssemblySolutionDto,
    bed: &PrintBedDto,
) -> Result<PrintLayoutReport, ExportError> {
    bed.validate().map_err(ExportError)?;
    structure.validate().map_err(ExportError)?;
    if !solution.solved {
        return Err(ExportError(
            "Resolve assembly errors before checking the print layout.".into(),
        ));
    }
    let sources: HashMap<BodyId, _> = meshes.iter().map(|m| (m.body_id, m)).collect();
    let parents: HashMap<_, _> = structure
        .occurrences
        .iter()
        .map(|o| (o.id, o.parent_occurrence_id))
        .collect();
    let root = |mut id: OccurrenceId| {
        while let Some(parent) = parents.get(&id).copied().flatten() {
            id = parent;
        }
        id.0
    };
    let mut report = PrintLayoutReport {
        bed: bed.clone(), printable_instances: 0, printable_groups: 0, excluded_instances: 0,
        issues: vec![], proposed_translations: vec![], proposal_fits: true, clearance_mm: 2.,
        overlap_check: "Conservative transformed mesh bounds; warnings are possible intersections, not exact solid collisions.".into(),
    };
    let mut groups = BTreeMap::<u64, Bounds>::new();
    let mut parts = Vec::new();
    let mut excluded_occurrences = BTreeSet::new();
    for pose in &solution.instance_body_poses {
        let Some(mesh) = sources.get(&pose.body_id) else {
            report.excluded_instances += 1;
            excluded_occurrences.insert(pose.occurrence_id.0);
            continue;
        };
        if !pose.visible {
            report.excluded_instances += 1;
            excluded_occurrences.insert(pose.occurrence_id.0);
            continue;
        }
        crate::mesh_weld::validate_mesh_buffers(mesh)?;
        if mesh.positions.is_empty() {
            return Err(ExportError(
                "Print layout contains an empty body mesh".into(),
            ));
        }
        if pose
            .translation
            .iter()
            .chain(pose.rotation.iter())
            .any(|v| !v.is_finite())
            || pose.rotation.iter().map(|v| v * v).sum::<f64>() < 1e-12
        {
            return Err(ExportError(
                "Invalid occurrence transform in print layout".into(),
            ));
        }
        let transform = AssemblyTransformDto {
            translation: pose.translation,
            rotation: pose.rotation,
        };
        let mut bounds = Bounds::empty();
        for v in mesh.positions.as_chunks::<3>().0.iter() {
            bounds.add(transform.transform_point(*v));
        }
        let group = groups
            .entry(root(pose.occurrence_id))
            .or_insert_with(Bounds::empty);
        group.add(bounds.min);
        group.add(bounds.max);
        parts.push((pose.occurrence_id.0, pose.body_id.0, bounds));
        report.printable_instances += 1;
        if let Some(issue) = bed_z_issue(
            bounds.min[2],
            &format!("{} (occurrence {})", mesh.name, pose.occurrence_id.0),
            vec![pose.occurrence_id.0],
        ) {
            report.issues.push(issue);
        }
        if !bed.contains_xy_bounds(
            [bounds.min[0], bounds.min[1]],
            [bounds.max[0], bounds.max[1]],
        ) || bounds.max[2] > bed.size_mm[2] + 1e-5
        {
            report.issues.push(issue(
                "outside_bed",
                format!(
                    "{} (occurrence {}) exceeds the configured usable envelope.",
                    mesh.name, pose.occurrence_id.0
                ),
                vec![pose.occurrence_id.0],
            ));
        }
    }
    report.printable_groups = groups.len();
    if report.printable_instances == 0 {
        report.issues.push(issue(
            "empty_layout",
            "No visible occurrences are included in this export.".into(),
            vec![],
        ));
        report.proposal_fits = false;
    }
    if report.excluded_instances > 0 {
        report.issues.push(issue("excluded_instances", format!("{} body occurrences are excluded by visibility or body selection. Repeated included occurrences are all preserved.", report.excluded_instances), excluded_occurrences.into_iter().collect()));
    }
    for (i, (id_a, body_a, a)) in parts.iter().enumerate() {
        for (id_b, body_b, b) in &parts[i + 1..] {
            if (0..3).all(|axis| a.max[axis].min(b.max[axis]) - a.min[axis].max(b.min[axis]) > 1e-5)
            {
                report.issues.push(issue("possible_overlap", format!("Bounds overlap for body {body_a} / occurrence {id_a} and body {body_b} / occurrence {id_b}; this may be intentional multipart geometry."), vec![*id_a, *id_b]));
            }
        }
    }
    let mut ordered: Vec<_> = groups.into_iter().collect();
    ordered
        .sort_by(|(a_id, a), (b_id, b)| b.size()[1].total_cmp(&a.size()[1]).then(a_id.cmp(b_id)));
    let margin = bed.margin_mm;
    let start = [bed.origin_mm[0] + margin, bed.origin_mm[1] + margin];
    let end = [
        bed.origin_mm[0] + bed.size_mm[0] - margin,
        bed.origin_mm[1] + bed.size_mm[1] - margin,
    ];
    let (mut x, mut y, mut row_depth) = (start[0], start[1], 0f64);
    let mut placed: Vec<Bounds> = vec![];
    for (id, bounds) in ordered {
        let size = bounds.size();
        if x + size[0] > end[0] + 1e-5 {
            x = start[0];
            y += row_depth + report.clearance_mm;
            row_depth = 0.;
        }
        if size[0] > bed.size_mm[0] - 2. * margin + 1e-5
            || size[1] > bed.size_mm[1] - 2. * margin + 1e-5
            || size[2] > bed.size_mm[2] + 1e-5
        {
            report.proposal_fits = false;
            report.issues.push(issue("arrangement_does_not_fit", "All groups do not fit this bed in their current orientations. Change orientation or split the layout into additional named views.".into(), vec![id]));
            continue;
        }
        let fits = |x: f64, y: f64| {
            bed.contains_xy_bounds([x, y], [x + size[0], y + size[1]])
                && placed.iter().all(|p| {
                    x + size[0] + report.clearance_mm <= p.min[0] + 1e-5
                        || x >= p.max[0] + report.clearance_mm - 1e-5
                        || y + size[1] + report.clearance_mm <= p.min[1] + 1e-5
                        || y >= p.max[1] + report.clearance_mm - 1e-5
                })
        };
        if !fits(x, y) {
            let mut xs = vec![start[0]];
            let mut ys = vec![start[1]];
            for p in &bed.excluded_regions {
                xs.push(p.iter().map(|v| v[0]).fold(f64::NEG_INFINITY, f64::max) + margin);
                ys.push(p.iter().map(|v| v[1]).fold(f64::NEG_INFINITY, f64::max) + margin);
            }
            for p in &placed {
                xs.push(p.max[0] + report.clearance_mm);
                ys.push(p.max[1] + report.clearance_mm);
            }
            let step = (bed.size_mm[0].max(bed.size_mm[1]) / 256.).max(1.);
            for i in 0..=256 {
                xs.push(start[0] + i as f64 * step);
                ys.push(start[1] + i as f64 * step);
            }
            xs.sort_by(f64::total_cmp);
            ys.sort_by(f64::total_cmp);
            let candidate = ys
                .into_iter()
                .filter(|y| *y + size[1] <= end[1] + 1e-5)
                .find_map(|y| {
                    xs.iter()
                        .copied()
                        .filter(|x| *x + size[0] <= end[0] + 1e-5)
                        .find(|x| fits(*x, y))
                        .map(|x| (x, y))
                });
            if let Some((nx, ny)) = candidate {
                x = nx;
                y = ny;
            } else {
                report.proposal_fits = false;
                report.issues.push(issue("arrangement_does_not_fit","A conservative arrangement could not fit all groups within the printable regions and exclusions. Change orientation or use additional named views.".into(),vec![id]));
                continue;
            }
        }
        let translation = [x - bounds.min[0], y - bounds.min[1], -bounds.min[2]];
        if translation.iter().any(|v| v.abs() > 1e-5) {
            report.proposed_translations.push(LayoutTranslation {
                occurrence_id: id,
                translation,
            });
        }
        placed.push(Bounds {
            min: [x, y, 0.],
            max: [x + size[0], y + size[1], size[2]],
        });
        x += size[0] + report.clearance_mm;
        row_depth = row_depth.max(size[1]);
    }
    if !report.proposal_fits {
        report.proposed_translations.clear();
    }
    Ok(report)
}
fn issue(code: &str, message: String, occurrence_ids: Vec<u64>) -> LayoutIssue {
    LayoutIssue {
        code: code.into(),
        message,
        occurrence_ids,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture(
        sizes: &[[f64; 3]],
    ) -> (
        Vec<TriangleMesh>,
        ComponentStructureDto,
        AssemblySolutionDto,
    ) {
        let (source, _) = crate::print_in_place_clip();
        let source = &source[0];
        let mut bounds = Bounds::empty();
        for point in source.positions.as_chunks::<3>().0.iter() {
            bounds.add(*point);
        }
        let meshes: Vec<_> = sizes
            .iter()
            .enumerate()
            .map(|(index, size)| {
                let mut mesh = source.clone();
                mesh.body_id = BodyId(index as u64 + 1);
                mesh.positions = source
                    .positions
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .flat_map(|point| {
                        std::array::from_fn::<_, 3, _>(|i| {
                            (point[i] - bounds.min[i]) / bounds.size()[i] * size[i]
                        })
                    })
                    .collect();
                mesh
            })
            .collect();
        let count = sizes.len();
        let structure: ComponentStructureDto = serde_json::from_value(json!({
            "definitions": (1..=count).map(|id| json!({"id":id,"name":format!("Group {id}"),"body_ids":[id]})).collect::<Vec<_>>(),
            "occurrences": (1..=count).map(|id| json!({"id":id,"name":format!("Group {id}"),"component_id":id})).collect::<Vec<_>>(),
            "next_component_id":count+1,"next_occurrence_id":count+1
        })).unwrap();
        let solution: AssemblySolutionDto = serde_json::from_value(json!({
            "solved":true,"body_poses":[],"diagnostics":[],
            "occurrence_poses": (1..=count).map(|id| json!({"occurrence_id":id,"component_id":id,"translation":[0.,0.,0.],"rotation":[0.,0.,0.,1.]})).collect::<Vec<_>>(),
            "instance_body_poses": (1..=count).map(|id| json!({"occurrence_id":id,"component_id":id,"body_id":id,"translation":[0.,0.,0.],"rotation":[0.,0.,0.,1.],"visible":true})).collect::<Vec<_>>()
        })).unwrap();
        (meshes, structure, solution)
    }

    #[test]
    fn exclusions_report_repeated_occurrences_and_body_selection_without_deduplicating_counts() {
        let (meshes, mut structure, mut solution) = fixture(&[[10., 6., 3.]; 3]);
        structure.definitions[0].body_ids = vec![BodyId(1), BodyId(2)];
        structure.definitions[1].body_ids = vec![BodyId(3)];
        structure.definitions[2].body_ids.clear();
        structure.occurrences[2].component_id = structure.occurrences[0].component_id;
        solution.occurrence_poses[2].component_id = solution.occurrence_poses[0].component_id;
        solution.instance_body_poses[1].body_id = BodyId(3);
        solution.instance_body_poses[2].body_id = BodyId(1);
        solution.instance_body_poses[2].component_id = solution.instance_body_poses[0].component_id;
        solution.instance_body_poses[0].visible = false;
        solution.instance_body_poses[2].visible = false;
        for index in [0, 2] {
            let mut second_hidden_body = solution.instance_body_poses[index];
            second_hidden_body.body_id = BodyId(2);
            solution.instance_body_poses.push(second_hidden_body);
        }
        let report =
            analyze_print_layout(&meshes[..1], &structure, &solution, &PrintBedDto::default())
                .unwrap();
        assert_eq!(report.printable_instances, 0);
        assert_eq!(report.excluded_instances, 5);
        let excluded = report
            .issues
            .iter()
            .find(|i| i.code == "excluded_instances")
            .unwrap();
        assert_eq!(excluded.occurrence_ids, vec![1, 2, 3]);
    }
    #[test]
    fn arrangement_backfills_gaps_when_the_next_shelf_is_below_the_bed() {
        let (meshes, structure, solution) = fixture(&[[4., 8., 1.], [8., 4., 1.], [6., 4., 1.]]);
        let bed = PrintBedDto {
            size_mm: [12., 14., 2.],
            margin_mm: 0.,
            origin_mm: [0., 0.],
            printable_regions: vec![],
            excluded_regions: vec![],
            ..Default::default()
        };
        let report = analyze_print_layout(&meshes, &structure, &solution, &bed).unwrap();
        assert!(report.proposal_fits, "{:?}", report.issues);
        let offsets: Vec<_> = report
            .proposed_translations
            .iter()
            .map(|movement| limo_cad_assembly::ViewOccurrenceOffsetDto {
                occurrence_id: OccurrenceId(movement.occurrence_id),
                translation: movement.translation,
                rotation: [0., 0., 0., 1.],
            })
            .collect();
        let arranged =
            limo_cad_assembly::resolve_view_layout(&structure, &solution, &offsets).unwrap();
        let checked = analyze_print_layout(&meshes, &structure, &arranged, &bed).unwrap();
        assert_eq!(checked.printable_instances, 3);
        assert_eq!(checked.printable_groups, 3);
        assert!(checked.issues.is_empty(), "{:?}", checked.issues);
        assert_eq!(arranged.instance_body_poses[2].translation, [6., 0., 0.]);
        assert!(solution
            .instance_body_poses
            .iter()
            .all(|p| p.translation == [0.; 3]));
    }

    #[test]
    fn arrangement_and_bed_diagnostics_agree_at_numeric_boundaries() {
        let bed = PrintBedDto {
            size_mm: [1., 1.5, 1.75],
            margin_mm: 0.,
            origin_mm: [0., 0.],
            printable_regions: vec![vec![[0., 0.], [1., 0.], [1., 1.5], [0., 1.5]]],
            excluded_regions: vec![],
            ..Default::default()
        };
        let (meshes, structure, solution) = fixture(&[[1.000005, 1.500005, 1.750005]]);
        let report = analyze_print_layout(&meshes, &structure, &solution, &bed).unwrap();
        assert!(!report.issues.iter().any(|i| i.code == "outside_bed"));
        assert!(report.proposal_fits, "{:?}", report.issues);
        for size in [[1.0001, 1.5, 1.75], [1., 1.5001, 1.75], [1., 1.5, 1.7501]] {
            let (meshes, structure, solution) = fixture(&[size]);
            let report = analyze_print_layout(&meshes, &structure, &solution, &bed).unwrap();
            assert!(report.issues.iter().any(|i| i.code == "outside_bed"));
            assert!(!report.proposal_fits);
            assert!(report.proposed_translations.is_empty());
        }
    }
}
