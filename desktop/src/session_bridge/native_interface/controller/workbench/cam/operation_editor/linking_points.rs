//! Preferred stations remain plain setup-XY points in the shared linking DTO.
//! Picking an existing center copies coordinates; it does not create a new
//! geometry association or grant permission for a predrilled entry.
use super::*;
use limo_cad_cam::{CamSetupDto, CamUnits, Point2Dto};
use operation_geometry::points;
use std::collections::{HashMap, HashSet};

pub(in super::super) mod picking;

const COLLECTIONS: [(&str, &str); 3] = [
    ("predrill_positions", "Predrill position"),
    ("entry_positions", "Preferred entry position"),
    ("exit_positions", "Preferred exit position"),
];

struct Candidate {
    key: String,
    label: String,
    point: Point2Dto,
    world: [f64; 3],
    identity: picking::Key,
}

pub(super) struct Context {
    predrill: Vec<Candidate>,
    stations: Vec<Candidate>,
    copied: HashMap<(&'static str, usize), Point2Dto>,
    stamp: std::sync::Arc<()>,
    source_visits: usize,
    model_valid: bool,
}

impl Context {
    pub(super) fn new(
        setup: &CamSetupDto,
        operation: &CamOperationDto,
        scene: &SolidSceneDto,
    ) -> Self {
        if !matches!(
            operation,
            CamOperationDto::Contour2d { .. } | CamOperationDto::Adaptive3d { .. }
        ) {
            return Self {
                predrill: Vec::new(),
                stations: Vec::new(),
                copied: HashMap::new(),
                stamp: std::sync::Arc::new(()),
                source_visits: 0,
                model_valid: scene.errors.is_empty(),
            };
        }
        let mut predrill = Vec::new();
        let mut source_visits = setup.operations.len().saturating_add(scene.bodies.len());
        for earlier in setup
            .operations
            .iter()
            .take_while(|op| op.id() != operation.id())
        {
            let CamOperationDto::Drill {
                id,
                name,
                enabled: true,
                points,
                holes,
                ..
            } = earlier
            else {
                continue;
            };
            for (index, point) in points
                .iter()
                .chain(holes.iter().map(|hole| &hole.point))
                .enumerate()
            {
                source_visits = source_visits.saturating_add(1);
                if point.x.is_finite() && point.y.is_finite() {
                    predrill.push(Candidate {
                        key: format!("drill:{id}:{index}"),
                        label: format!("{name} · drilled center {}", index + 1),
                        point: *point,
                        world: std::array::from_fn(|i| {
                            [setup.wcs.origin.x, setup.wcs.origin.y, setup.wcs.origin.z][i]
                                + point.x * setup.wcs.x_axis[i]
                                + point.y * setup.wcs.y_axis[i]
                                + setup.stock.max.z * setup.wcs.z_axis[i]
                        }),
                        identity: picking::Key::Drill {
                            operation: *id,
                            index,
                        },
                    });
                }
            }
        }
        let mut stations = Vec::new();
        let mut seen = HashSet::new();
        for body in scene
            .bodies
            .iter()
            .filter(|body| setup.body_ids.contains(&body.id))
        {
            for (index, vertex) in body.mesh.positions.as_chunks::<3>().0.iter().enumerate() {
                if stations.len() >= 2000 {
                    break;
                }
                source_visits = source_visits.saturating_add(1);
                let vertex = [
                    f64::from(vertex[0]),
                    f64::from(vertex[1]),
                    f64::from(vertex[2]),
                ];
                if !vertex.iter().all(|value| value.is_finite())
                    || !seen.insert(format!(
                        "{:.5}:{:.5}:{:.5}",
                        vertex[0], vertex[1], vertex[2]
                    ))
                {
                    continue;
                }
                let relative = [
                    vertex[0] - setup.wcs.origin.x,
                    vertex[1] - setup.wcs.origin.y,
                    vertex[2] - setup.wcs.origin.z,
                ];
                let project =
                    |axis: [f64; 3]| relative.into_iter().zip(axis).map(|(a, b)| a * b).sum();
                let point = Point2Dto::new(project(setup.wcs.x_axis), project(setup.wcs.y_axis));
                if point.x.is_finite() && point.y.is_finite() {
                    stations.push(Candidate {
                        key: format!("vertex:{}:{index}", body.id.0),
                        label: format!("{} · vertex {}", body.name, index + 1),
                        point,
                        world: vertex,
                        identity: picking::Key::Vertex {
                            body: body.id.0,
                            index,
                        },
                    });
                }
            }
        }
        Self {
            predrill,
            stations,
            copied: HashMap::new(),
            stamp: std::sync::Arc::new(()),
            source_visits,
            model_valid: scene.errors.is_empty(),
        }
    }

    fn candidates(&self, key: &str) -> &[Candidate] {
        if key == "predrill_positions" {
            &self.predrill
        } else {
            &self.stations
        }
    }
}

fn prefix(key: &str) -> String {
    format!("/native/linking/{key}")
}

fn supported(draft: &Draft, key: &str) -> bool {
    match draft.record["kind"].as_str() {
        Some("contour2d") => true,
        Some("adaptive3d") => key != "exit_positions",
        _ => false,
    }
}

fn original<'a>(record: &'a Value, key: &str) -> &'a [Value] {
    record[key].as_array().map(Vec::as_slice).unwrap_or(&[])
}

fn collection(path: &str) -> Option<(&'static str, &'static str)> {
    COLLECTIONS.into_iter().find(|(key, _)| {
        let prefix = prefix(key);
        path.starts_with(&format!("{prefix}/")) || path == points::cursor(&prefix)
    })
}

pub(super) fn handles(path: &str) -> bool {
    collection(path).is_some()
}

fn count(draft: &Draft, key: &str) -> Result<usize, String> {
    let count = form::text(draft, &format!("{}/count", prefix(key)))?
        .parse::<usize>()
        .map_err(|_| "Enter a whole position count")?;
    let maximum = if key == "predrill_positions" { 32 } else { 1 };
    if count > maximum {
        return Err(format!(
            "Use at most {maximum} {}",
            if maximum == 1 {
                "preferred position"
            } else {
                "predrill positions"
            }
        ));
    }
    Ok(count)
}

fn extend_row(
    draft: &mut Draft,
    units: CamUnits,
    record: &Value,
    context: &Context,
    key: &str,
    label: &str,
) {
    let prefix = prefix(key);
    points::extend(draft, &prefix, label, original(record, key), units);
    let Some(index) = points::selected(draft, &prefix) else {
        return;
    };
    let path = format!("{prefix}/{index}/candidate");
    if draft.fields.iter().any(|field| field.path == path) {
        return;
    }
    let mut options = form::options(&[("manual", "Manual coordinates")]);
    options.extend(
        context
            .candidates(key)
            .iter()
            .map(|candidate| ChoiceOption {
                value: candidate.key.clone(),
                label: candidate.label.clone(),
                disabled: false,
            }),
    );
    form::push(
        draft,
        &path,
        &format!("{label} {} source", index + 1),
        InputKind::Choice,
        json!("manual"),
        units,
        Some(options),
    );
}

pub(super) fn extend(draft: &mut Draft, cam: &CamDocumentDto, record: &Value, context: &Context) {
    for (key, label) in COLLECTIONS {
        if supported(draft, key) {
            extend_row(draft, cam.units, record, context, key, label);
            form::push(
                draft,
                &picking::button(key),
                &format!("Viewport {}", label.to_lowercase()),
                InputKind::Boolean,
                json!(false),
                cam.units,
                None,
            );
        }
    }
}

pub(super) fn changed(
    draft: &mut Draft,
    cam: &CamDocumentDto,
    path: &str,
    record: &Value,
    context: &mut Context,
) -> Result<(), String> {
    let Some((key, label)) = collection(path) else {
        return Ok(());
    };
    if !supported(draft, key) {
        return Ok(());
    }
    count(draft, key)?;
    let prefix = prefix(key);
    points::changed(
        draft,
        &prefix,
        label,
        path,
        original(record, key),
        cam.units,
    )?;
    extend_row(draft, cam.units, record, context, key, label);
    let Some(index) = points::selected(draft, &prefix) else {
        return Ok(());
    };
    let candidate_path = format!("{prefix}/{index}/candidate");
    if path == candidate_path {
        let selected = form::text(draft, path)?;
        if selected != "manual" {
            let point = context
                .candidates(key)
                .iter()
                .find(|candidate| candidate.key == selected)
                .ok_or("Choose an available position or enter manual coordinates")?
                .point;
            for (axis, value) in [("x", point.x), ("y", point.y)] {
                form::set(
                    draft,
                    &format!("{prefix}/{index}/{axis}"),
                    &cam.units.from_mm(value).to_string(),
                );
            }
            context.copied.insert((key, index), point);
        }
    } else if path == format!("{prefix}/{index}/x") || path == format!("{prefix}/{index}/y") {
        form::set(draft, &candidate_path, "manual");
    }
    Ok(())
}

pub(super) fn visible(draft: &Draft, path: &str) -> bool {
    let Some((key, _)) = collection(path) else {
        return false;
    };
    supported(draft, key)
        && form::text(draft, "/native/linking/mode").unwrap_or("") == "custom"
        && (picking::is_button(path) || points::visible(draft, &prefix(key), path))
}

pub(super) fn apply(
    draft: &Draft,
    record: &mut Value,
    units: CamUnits,
    context: &Context,
) -> Result<(), String> {
    for (key, _) in COLLECTIONS {
        let prefix = prefix(key);
        if !supported(draft, key) || !form::changed(draft, &format!("{prefix}/")) {
            continue;
        }
        count(draft, key)?;
        let mut values = points::read(draft, &prefix, original(record, key), units)?;
        for (index, point) in values.iter_mut().enumerate() {
            let selected =
                form::text(draft, &format!("{prefix}/{index}/candidate")).unwrap_or("manual");
            if selected != "manual"
                && !context
                    .candidates(key)
                    .iter()
                    .any(|candidate| candidate.key == selected)
            {
                return Err("Choose an available position or enter manual coordinates".into());
            }
            if let Some(copied) = context.copied.get(&(key, index)) {
                for (axis, canonical, target) in
                    [("x", copied.x, &mut point.x), ("y", copied.y, &mut point.y)]
                {
                    if form::text(draft, &format!("{prefix}/{index}/{axis}"))
                        .is_ok_and(|text| text == units.from_mm(canonical).to_string())
                    {
                        *target = canonical;
                    }
                }
            }
        }
        record[key] = serde_json::to_value(values).map_err(|error| error.to_string())?;
    }
    Ok(())
}
