//! Owned, nonblocking camera transitions driven by Bevy updates.
use super::super::workspace::DocumentReceipt;
use super::*;
use bevy::prelude::{Quat, Resource, Transform};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

#[derive(Resource, Default)]
struct Motions {
    next: u64,
    active: Option<Motion>,
    completed: BTreeMap<u64, Result<Value, String>>,
}
struct Motion {
    id: u64,
    receipt: DocumentReceipt,
    render_revision: u64,
    started: Instant,
    duration: Duration,
    expires_ms: u64,
    from: ViewportCamera,
    to: ViewportCamera,
    last: ViewportCamera,
    orbit: Option<f32>,
}
impl Motions {
    fn complete(&mut self, id: u64, result: Result<Value, String>) {
        self.completed.insert(id, result);
        while self.completed.len() > 16 {
            self.completed.pop_first();
        }
    }
}

pub(super) fn pose(camera: ViewportCamera) -> Result<(Vec3, f32, Quat), String> {
    let eye = Vec3::from_array(camera.position);
    let target = Vec3::from_array(camera.target);
    let up = Vec3::from_array(camera.up);
    let distance = eye.distance(target);
    if !eye.is_finite()
        || !target.is_finite()
        || !up.is_finite()
        || !distance.is_finite()
        || distance <= 1e-6
        || up.cross(eye - target).length_squared() <= 1e-12
    {
        return Err("Camera has no finite, non-degenerate orientation".into());
    }
    Ok((
        target,
        distance,
        Transform::from_translation(eye)
            .looking_at(target, up)
            .rotation,
    ))
}
fn sample(
    from: ViewportCamera,
    to: ViewportCamera,
    orbit: Option<f32>,
    fraction: f32,
) -> Result<ViewportCamera, String> {
    let t = fraction.clamp(0., 1.);
    if t == 0. {
        return Ok(from);
    }
    if let Some(degrees) = orbit {
        if t == 1. && (degrees.abs() == 360. || degrees == 0.) {
            return Ok(from);
        }
        let target = Vec3::from_array(from.target);
        let axis = Vec3::from_array(from.up)
            .try_normalize()
            .ok_or("Camera has no orbit axis")?;
        let rotation = Quat::from_axis_angle(axis, (degrees * t).to_radians());
        return Ok(ViewportCamera {
            position: (target + rotation * (Vec3::from_array(from.position) - target)).to_array(),
            ..from
        });
    }
    if t == 1. {
        return Ok(to);
    }
    let (a, ar, aq) = pose(from)?;
    let (b, br, bq) = pose(to)?;
    let q = aq.slerp(bq, t);
    let target = a.lerp(b, t);
    Ok(ViewportCamera {
        position: (target + q * Vec3::Z * (ar + (br - ar) * t)).to_array(),
        target: target.to_array(),
        up: (q * Vec3::Y).to_array(),
        vertical_fov_degrees: from.vertical_fov_degrees
            + (to.vertical_fov_degrees - from.vertical_fov_degrees) * t,
    })
}

pub(in super::super) fn request(
    world: &mut World,
    owner: &DocumentContext,
    revision: u64,
    request: &Value,
) -> Result<Value, String> {
    if request.get("named_view").is_some() {
        return Err("action view does not accept named_view; use recall_named_view with name to recall a saved camera, visibility and display offsets".into());
    }
    let view = request["view"].as_str().unwrap_or("current");
    let direction = if view == "current" {
        None
    } else {
        Some(ViewDirection::parse(view)?)
    };
    let fit = request
        .get("fit")
        .map(|v| v.as_bool().ok_or("View fit must be boolean"))
        .transpose()?
        .unwrap_or(false);
    let duration = request
        .get("duration_ms")
        .map(|v| {
            v.as_u64()
                .filter(|v| *v <= 10_000)
                .ok_or("View duration must be an integer from 0 to 10000 ms")
        })
        .transpose()?
        .unwrap_or(300);
    let orbit = request
        .get("orbit_degrees")
        .map(|v| {
            v.as_f64()
                .filter(|v| v.is_finite() && v.abs() <= 360. && view == "current")
                .map(|v| v as f32)
                .ok_or("Orbit requires current view and an angle from -360 to 360 degrees")
        })
        .transpose()?;
    let expires_ms = request["expires_ms"]
        .as_u64()
        .ok_or("View request has no valid deadline")?;
    if expires_ms <= crate::session_bridge::now_ms() {
        return Err("View request expired".into());
    }
    let mut targets = Vec::new();
    for (name, component) in [("body_id", false), ("component_id", true)] {
        if let Some(value) = request.get(name) {
            let id = value
                .as_u64()
                .ok_or("View geometry ID must be a nonnegative integer")?;
            targets.push(if component {
                Target::Component(id)
            } else {
                Target::Body(id)
            });
        }
    }
    if let Some(value) = request.get("target") {
        if value != "active_sketch" {
            return Err("Unknown view target".into());
        }
        targets.push(Target::ActiveSketch);
    }
    if targets.len() > 1 {
        return Err("Choose one view target".into());
    }
    let target = targets.first().copied().unwrap_or(Target::All);
    let (session, camera, presentation, size) = native_viewport::interface_view(world);
    if session != owner.document_id {
        return Err("The rendered document is not current".into());
    }
    pose(camera)?;
    if direction.is_none() && !fit && target == Target::All && orbit.is_none() {
        return Ok(json!({"camera":camera}));
    }
    let mut from = camera;
    let mut to = camera;
    if fit || target != Target::All || direction == Some(ViewDirection::Isometric) {
        let model = native_viewport::interface_geometry(world);
        if target == Target::ActiveSketch
            && (model.active_sketch.is_none()
                || presentation.mode != native_viewport::ViewportMode::Sketch)
        {
            return Err("There is no active sketch to frame".into());
        }
        let bounds = target_bounds(world, model, presentation, target);
        if bounds.is_none() && target != Target::All {
            return Err("The requested geometry is not visible".into());
        }
        to = fit_bounds(bounds, camera, size, direction)?;
        if orbit.is_some() {
            from = to;
        }
    } else if let Some(direction) = direction {
        let (axis, up) = direction.axes();
        let (target, distance, _) = pose(camera)?;
        to.position = (target + axis * distance).to_array();
        to.up = up.to_array();
    }
    if orbit.is_some() {
        to = sample(from, from, orbit, 1.)?;
    }
    pose(to)?;
    let mut state = world.remove_resource::<Motions>().unwrap_or_default();
    let result = (|| {
        let id = state
            .next
            .checked_add(1)
            .ok_or("Camera motion identities exhausted")?;
        native_viewport::apply_interface_view(
            world,
            &owner.document_id,
            Some(if duration == 0 { to } else { from }),
            None,
        )?;
        if let Some(previous) = state.active.take() {
            state.complete(
                previous.id,
                Err("Camera transition was replaced by another view command".into()),
            );
        }
        state.next = id;
        if duration == 0 {
            return Ok(json!({"camera":to}));
        }
        state.active = Some(Motion {
            id,
            receipt: DocumentReceipt {
                owner: owner.clone(),
                revision,
            },
            render_revision: native_viewport::interface_model_revision(world),
            started: Instant::now(),
            duration: Duration::from_millis(duration),
            expires_ms,
            from,
            to,
            last: from,
            orbit,
        });
        Ok(json!({"camera_pending":id}))
    })();
    world.insert_resource(state);
    result
}
pub(in super::super) fn pending(world: &World) -> bool {
    world
        .get_resource::<Motions>()
        .is_some_and(|s| s.active.is_some())
}
pub(in super::super) fn cancel(world: &mut World, reason: &str) {
    if let Some(mut state) = world.get_resource_mut::<Motions>() {
        if let Some(motion) = state.active.take() {
            state.complete(motion.id, Err(reason.into()));
        }
    }
}
pub(in super::super) fn advance(world: &mut World, receipt: &DocumentReceipt) {
    advance_at(
        world,
        receipt,
        Instant::now(),
        crate::session_bridge::now_ms(),
    );
}
fn advance_at(world: &mut World, receipt: &DocumentReceipt, now: Instant, wall_ms: u64) {
    let Some(mut state) = world.remove_resource::<Motions>() else {
        return;
    };
    if let Some(mut motion) = state.active.take() {
        let result = (|| {
            if motion.receipt != *receipt {
                return Err("Document changed during the camera transition".into());
            }
            if native_viewport::interface_model_revision(world) != motion.render_revision {
                return Err("Rendered geometry changed during the camera transition".into());
            }
            if wall_ms >= motion.expires_ms {
                return Err("Camera transition expired".into());
            }
            let (session, camera) = native_viewport::interface_camera_snapshot(world);
            if session != receipt.owner.document_id || camera != motion.last {
                return Err(
                    "Camera transition was interrupted by another camera or document change".into(),
                );
            }
            let fraction = (now.saturating_duration_since(motion.started).as_secs_f32()
                / motion.duration.as_secs_f32())
            .clamp(0., 1.);
            let eased = fraction * fraction * (3. - 2. * fraction);
            let camera = sample(motion.from, motion.to, motion.orbit, eased)?;
            native_viewport::apply_interface_view(world, &session, Some(camera), None)?;
            motion.last = camera;
            Ok(fraction >= 1.)
        })();
        match result {
            Ok(true) => state.complete(motion.id, Ok(json!({"camera":motion.last}))),
            Ok(false) => state.active = Some(motion),
            Err(error) => state.complete(motion.id, Err(error)),
        }
    }
    world.insert_resource(state);
}
pub(in super::super) fn poll(world: &mut World, id: u64) -> Option<Result<Value, String>> {
    let Some(mut state) = world.get_resource_mut::<Motions>() else {
        return Some(Err("Camera transition is no longer available".into()));
    };
    if state.active.as_ref().is_some_and(|m| m.id == id) {
        return None;
    }
    Some(
        state
            .completed
            .remove(&id)
            .unwrap_or_else(|| Err("Camera transition was replaced".into())),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_bridge::native_interface::tests::Fixture;

    #[test]
    fn unsupported_named_camera_does_not_change_view_or_start_motion() {
        let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
        let fixture = Fixture::new();
        let owner = fixture.owner();
        let receipt = fixture
            .bridge
            .native_document_receipt(&fixture.engine, &owner)
            .unwrap();
        let mut app = native_viewport::interface_scene_fixture();
        native_viewport::apply_interface_model(app.world_mut(), model_snapshot(&fixture.engine))
            .unwrap();
        let before = native_viewport::interface_camera_snapshot(app.world()).1;
        for duration in [0, 300] {
            let error = request(
                app.world_mut(),
                &owner,
                receipt.revision,
                &json!({"view":"current", "named_view":"Review", "duration_ms":duration,
                    "expires_ms":crate::session_bridge::now_ms()+5000}),
            )
            .unwrap_err();
            assert!(error.contains("recall_named_view"));
            assert_eq!(
                native_viewport::interface_camera_snapshot(app.world()).1,
                before
            );
            assert!(app.world().get_resource::<Motions>().is_none());
        }
    }

    #[test]
    fn motion_receipts_wait_for_completion_and_reject_interruption_without_overwriting_the_camera()
    {
        let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
        let fixture = Fixture::new();
        let owner = fixture.owner();
        let receipt = fixture
            .bridge
            .native_document_receipt(&fixture.engine, &owner)
            .unwrap();
        let mut app = native_viewport::interface_scene_fixture();
        native_viewport::apply_interface_model(app.world_mut(), model_snapshot(&fixture.engine))
            .unwrap();
        let launch = |world: &mut World| {
            request(world, &owner, receipt.revision, &json!({"view":"front","fit":true,"duration_ms":300,"expires_ms":crate::session_bridge::now_ms()+5000})).unwrap()["camera_pending"].as_u64().unwrap()
        };
        let before = native_viewport::interface_camera_snapshot(app.world()).1;
        let id = launch(app.world_mut());
        assert!(poll(app.world_mut(), id).is_none());
        let start = app
            .world()
            .resource::<Motions>()
            .active
            .as_ref()
            .unwrap()
            .started;
        advance_at(
            app.world_mut(),
            &receipt,
            start + Duration::from_millis(150),
            crate::session_bridge::now_ms(),
        );
        assert_ne!(
            native_viewport::interface_camera_snapshot(app.world()).1,
            before
        );
        assert!(poll(app.world_mut(), id).is_none());
        advance_at(
            app.world_mut(),
            &receipt,
            start + Duration::from_millis(300),
            crate::session_bridge::now_ms(),
        );
        assert!(poll(app.world_mut(), id).unwrap().is_ok());
        for reason in [
            "revision",
            "input",
            "expiry",
            "replacement",
            "close",
            "render",
        ] {
            let id = launch(app.world_mut());
            let start = app
                .world()
                .resource::<Motions>()
                .active
                .as_ref()
                .unwrap()
                .started;
            let expiry = app
                .world()
                .resource::<Motions>()
                .active
                .as_ref()
                .unwrap()
                .expires_ms;
            let mut current = receipt.clone();
            if reason == "revision" {
                current.revision += 1;
            }
            if reason == "input" {
                let mut moved = native_viewport::interface_camera_snapshot(app.world()).1;
                moved.position[0] += 50.;
                native_viewport::apply_interface_view(
                    app.world_mut(),
                    &owner.document_id,
                    Some(moved),
                    None,
                )
                .unwrap();
            }
            if reason == "replacement" {
                launch(app.world_mut());
            }
            if reason == "close" {
                cancel(app.world_mut(), "Window close requested");
            }
            if reason == "render" {
                native_viewport::apply_interface_model(
                    app.world_mut(),
                    model_snapshot(&fixture.engine),
                )
                .unwrap();
            }
            let preserved = native_viewport::interface_camera_snapshot(app.world()).1;
            if reason != "replacement" {
                advance_at(
                    app.world_mut(),
                    &current,
                    start + Duration::from_millis(150),
                    if reason == "expiry" {
                        expiry
                    } else {
                        crate::session_bridge::now_ms()
                    },
                );
            }
            assert!(poll(app.world_mut(), id).unwrap().is_err(), "{reason}");
            assert_eq!(
                native_viewport::interface_camera_snapshot(app.world()).1,
                preserved,
                "{reason}"
            );
        }
        assert_eq!(
            fixture
                .bridge
                .native_document_receipt(&fixture.engine, &owner)
                .unwrap(),
            receipt
        );
    }

    #[test]
    fn targeted_framing_uses_visible_placed_geometry_and_the_active_sketch() {
        let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
        let fixture = Fixture::new();
        let owner = fixture.owner();
        for (op, args) in [
            ("sketch_begin", json!({"type":"origin_plane","plane":"xy"})),
            (
                "sketch_add_rectangle",
                json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":20.,"y":20.},"ctrl_held":true}),
            ),
            ("sketch_finish", json!({})),
            (
                "solid_extrude",
                json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":10.}}),
            ),
        ] {
            fixture
                .bridge
                .apply_native_mutation(&fixture.engine, &owner, op, &args, || Ok(()))
                .unwrap();
        }
        let receipt = fixture
            .bridge
            .native_document_receipt(&fixture.engine, &owner)
            .unwrap();
        let mut model = model_snapshot(&fixture.engine);
        let body = model.document.scene.bodies[0].id.0;
        model.instance_body_poses = std::sync::Arc::new(serde_json::from_value(json!([
            {"occurrence_id":10,"component_id":7,"body_id":body,"translation":[100.,0.,0.],"rotation":[0.,0.,0.,1.],"visible":true},
            {"occurrence_id":11,"component_id":8,"body_id":body,"translation":[-100.,0.,0.],"rotation":[0.,0.,0.,1.],"visible":true}
        ])).unwrap());
        let mut app = native_viewport::interface_scene_fixture();
        native_viewport::apply_interface_model(app.world_mut(), model).unwrap();
        let focus = |world: &mut World, mut args: Value| {
            args["view"] = json!("isometric");
            args["duration_ms"] = json!(0);
            args["expires_ms"] = json!(crate::session_bridge::now_ms() + 5000);
            request(world, &owner, receipt.revision, &args)
        };
        let component = focus(app.world_mut(), json!({"component_id":7})).unwrap();
        assert_eq!(component["camera"]["target"], json!([110., 10., 5.]));
        let body_view = focus(app.world_mut(), json!({"body_id":body})).unwrap();
        assert_eq!(body_view["camera"]["target"], json!([10., 10., 5.]));
        let preserved = native_viewport::interface_camera_snapshot(app.world()).1;
        for invalid in [
            json!({"component_id":99}),
            json!({"body_id":body,"component_id":7}),
            json!({"target":"active_sketch"}),
        ] {
            assert!(focus(app.world_mut(), invalid).is_err());
            assert_eq!(
                native_viewport::interface_camera_snapshot(app.world()).1,
                preserved
            );
        }
        fixture
            .bridge
            .apply_native_mutation(
                &fixture.engine,
                &owner,
                "sketch_begin",
                &json!({"type":"origin_plane","plane":"xz"}),
                || Ok(()),
            )
            .unwrap();
        fixture
            .bridge
            .apply_native_mutation(
                &fixture.engine,
                &owner,
                "sketch_add_circle",
                &json!({"mode":"center_diameter","p1":{"x":80.,"y":30.},"p2":{"x":90.,"y":30.},"ctrl_held":true}),
                || Ok(()),
            )
            .unwrap();
        native_viewport::apply_interface_model(app.world_mut(), model_snapshot(&fixture.engine))
            .unwrap();
        let (_, _, mut presentation, _) = native_viewport::interface_view_snapshot(app.world());
        presentation.mode = native_viewport::ViewportMode::Sketch;
        native_viewport::apply_interface_view(
            app.world_mut(),
            &owner.document_id,
            None,
            Some(presentation),
        )
        .unwrap();
        let sketch = focus(app.world_mut(), json!({"target":"active_sketch"})).unwrap();
        assert_eq!(sketch["camera"]["target"], json!([80., 0., 30.]));
    }
    #[test]
    fn orbit_samples_a_full_turn_at_constant_radius_and_elevation() {
        let from = ViewportCamera {
            position: [15., -25., 30.],
            target: [5., -5., 10.],
            up: [0., 0., 1.],
            ..Default::default()
        };
        for angle in [360., -360., 180., -90.] {
            for t in [0., 0.25, 0.5, 0.75, 1.] {
                let camera = sample(from, from, Some(angle), t).unwrap();
                assert!((pose(camera).unwrap().1 - pose(from).unwrap().1).abs() < 1e-4);
                assert!((camera.position[2] - from.position[2]).abs() < 1e-4);
                assert_eq!(camera.target, from.target);
                if t > 0. && t < 1. {
                    assert_ne!(camera.position, from.position);
                }
            }
        }
        assert_eq!(sample(from, from, Some(360.), 1.).unwrap(), from);
    }
    #[test]
    fn opposite_views_never_pass_through_the_target_or_collapse_up() {
        let from = ViewportCamera {
            position: [0., 0., 100.],
            target: [0.; 3],
            up: [0., 1., 0.],
            ..Default::default()
        };
        let to = ViewportCamera {
            position: [0., 0., -100.],
            up: [0., -1., 0.],
            ..from
        };
        for i in 0..=100 {
            assert!(
                (pose(sample(from, to, None, i as f32 / 100.).unwrap())
                    .unwrap()
                    .1
                    - 100.)
                    .abs()
                    < 1e-3
            );
        }
        assert_eq!(sample(from, to, None, 1.).unwrap(), to);
    }
}
