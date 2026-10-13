use super::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc,
};

struct Inputs {
    scene: Arc<limo_cad_solid::SolidSceneDto>,
    setup: limo_cad_cam::CamSetupDto,
    holes: Vec<limo_cad_cam::CamHoleDto>,
}
fn prepare(
    context: &Inputs,
    geometry: physical_pick::Snapshot,
    cancelled: &AtomicBool,
) -> Result<(physical_pick::Prepared, Vec<Candidate>), String> {
    let physical = physical_pick::Prepared::new(context.scene.clone(), geometry, cancelled)?;
    if context.holes.len() > 4_096 {
        return Err(
            "Viewport picking exceeds its cylindrical face budget; use the geometry fields".into(),
        );
    }
    let mut candidates = Vec::new();
    for hole in &context.holes {
        if cancelled.load(Ordering::Acquire) {
            return Err("Geometry picking was cancelled".into());
        }
        let reference = hole
            .face_key
            .as_deref()
            .ok_or("Cylindrical face identity missing")?;
        let key = FaceKey::parse(reference)?;
        if !physical
            .snapshot
            .instances
            .iter()
            .any(|instance| instance.body_id == key.body_id)
        {
            continue;
        }
        let mut resolved = hole.clone();
        limo_cad_sketch::resolve_cam_hole_reference(
            reference,
            &mut resolved,
            &context.setup,
            &context.scene,
        )?;
        if ![
            resolved.point.x,
            resolved.point.y,
            resolved.top_z,
            resolved.bottom_z,
        ]
        .into_iter()
        .chain(resolved.axis)
        .all(f64::is_finite)
        {
            return Err("Cylindrical face has invalid hole coordinates".into());
        }
        let triangles = physical.face_triangles(key.body_id, key.face_id, cancelled)?;
        if !triangles.is_empty() {
            candidates.push(Candidate { key, triangles });
        }
    }
    Ok((physical, candidates))
}
pub(super) fn start(
    context: &operation_geometry::Context,
    geometry: physical_pick::Snapshot,
    handle: NativeInterfaceHandle,
) -> Result<worker::Worker, String> {
    if context.holes.len() > 4_096 {
        return Err(
            "Viewport picking exceeds its cylindrical face budget; use the geometry fields".into(),
        );
    }
    if context.setup.body_ids.len() > physical_pick::MAX_INSTANCES {
        return Err(
            "Viewport picking exceeds its setup body budget; use the geometry fields".into(),
        );
    }
    let setup = limo_cad_cam::CamSetupDto {
        machine: None,
        id: context.setup.id,
        name: String::new(),
        wcs: context.setup.wcs,
        wcs_origin: default(),
        work_offset: context.setup.work_offset,
        work_offset_count: context.setup.work_offset_count,
        stock_spec: default(),
        resolved_stock: default(),
        stock: context.setup.stock,
        stock_model_box: context.setup.stock_model_box,
        body_ids: context.setup.body_ids.clone(),
        legacy_clearance_z: None,
        legacy_retract_z: None,
        operations: Vec::new(),
    };
    let context = Inputs {
        scene: context.scene.clone(),
        setup,
        holes: context.holes.iter().map(|(_, hole)| hole.clone()).collect(),
    };
    let (sender, requests) = mpsc::sync_channel::<worker::Request>(1);
    let (send, receiver) = mpsc::channel();
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancel = cancelled.clone();
    std::thread::Builder::new()
        .name("cad-native-cam-pick".into())
        .spawn(move || {
            let prepared = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                prepare(&context, geometry, &cancel)
            }))
            .unwrap_or_else(|_| Err("Physical geometry worker stopped unexpectedly".into()));
            if cancel.load(Ordering::Acquire) {
                return;
            }
            let (physical, candidates) = match prepared {
                Ok(value) => value,
                Err(error) => {
                    let _ = send.send(worker::ResultMessage::Holes(Err(error)));
                    handle.request_redraw();
                    return;
                }
            };
            let allowed: std::collections::HashSet<_> =
                candidates.iter().map(|candidate| candidate.key).collect();
            if send
                .send(worker::ResultMessage::Holes(Ok(candidates)))
                .is_err()
            {
                return;
            }
            handle.request_redraw();
            while let Ok(worker::Request::Face(request)) = requests.recv() {
                if cancel.load(Ordering::Acquire) {
                    return;
                }
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    Ok(physical.pick(request.ray, &cancel)?.and_then(|hit| {
                        let key = FaceKey {
                            body_id: hit.body_id,
                            face_id: hit.face_id,
                        };
                        allowed.contains(&key).then_some(key)
                    }))
                }))
                .unwrap_or_else(|_| Err("Physical geometry resolver stopped unexpectedly".into()));
                if cancel.load(Ordering::Acquire) {
                    return;
                }
                if send
                    .send(worker::ResultMessage::Face(request, result))
                    .is_err()
                {
                    return;
                }
                handle.request_redraw();
            }
        })
        .map_err(|error| error.to_string())?;
    Ok(worker::Worker {
        sender: Some(sender),
        receiver: Mutex::new(receiver),
        cancelled,
    })
}
