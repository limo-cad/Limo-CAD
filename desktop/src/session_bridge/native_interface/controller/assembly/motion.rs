//! Joint motion previews retain source geometry and commit only on Save position.
use super::*;
use crate::native_forms::joint::Form;
use crate::native_viewport::ViewportPresentation;
use limo_cad_sketch::{AssemblySolutionDto, JointMotionStateDto};
use std::time::{Duration, Instant};
pub(super) mod panel;
#[cfg(test)]
mod tests;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Select(u64),
    Field(usize),
    Demo,
    Revert,
    Save,
}
#[derive(Default)]
pub(super) struct State {
    selected: Option<u64>,
    form: Option<Form>,
    serial: u64,
    original: Option<ViewportPresentation>,
    preview: bool,
    started: Option<Instant>,
    next_frame: Option<Instant>,
    base: [f64; 5],
    error: Option<String>,
}
pub(super) fn selected(state: &State) -> Option<u64> {
    state.selected
}
impl State {
    pub(super) fn changed(&mut self, a: &AssemblyDocumentDto) {
        self.started = None;
        self.next_frame = None;
        self.preview = false;
        self.original = None;
        self.error = None;
        self.form = self
            .selected
            .and_then(|id| a.joints.iter().find(|j| j.id.0 == id))
            .map(|j| Form::new(a, Some(j.clone()), UnitSystem::Mm));
        if self.form.is_none() {
            self.selected = None;
        }
        self.serial = self.serial.wrapping_add(1);
    }
    fn motion(&self) -> Result<JointMotionStateDto, String> {
        let form = self.form.as_ref().ok_or("Select a joint")?;
        let mut values = [0.; 5];
        for (i, coordinate) in form.coordinates.iter().enumerate() {
            values[i] = coordinate.values[0].evaluate(UnitSystem::Mm, &[])?;
        }
        for (i, _) in form.axes() {
            if form.coordinates[i].limited {
                let c = &form.coordinates[i];
                let min = c.values[1].evaluate(UnitSystem::Mm, &[])?;
                let max = c.values[2].evaluate(UnitSystem::Mm, &[])?;
                if values[i] < min || values[i] > max {
                    return Err("The requested position exceeds the joint limits".into());
                }
            }
        }
        Ok(JointMotionStateDto {
            joint_id: form.original.as_ref().ok_or("Joint definition missing")?.id,
            angle_offset_deg: values[0],
            linear_offset_mm: values[1],
            secondary_angle_offset_deg: values[2],
            tertiary_angle_offset_deg: values[3],
            secondary_linear_offset_mm: values[4],
        })
    }
}
pub(super) fn restore(
    world: &mut World,
    s: &mut State,
    owner: &DocumentContext,
) -> Result<(), String> {
    if let Some(original) = s.original.take() {
        let (id, _, mut view, _) = native_viewport::interface_view_snapshot(world);
        if id == owner.document_id {
            view.body_poses = original.body_poses;
            view.instance_body_poses = original.instance_body_poses;
            native_viewport::apply_interface_view(world, &id, None, Some(view))?;
        }
    }
    s.preview = false;
    s.started = None;
    s.next_frame = None;
    Ok(())
}
pub(crate) fn active(world: &World) -> bool {
    world
        .get_resource::<Browser>()
        .is_some_and(|b| b.motion.original.is_some() || b.motion.started.is_some())
}
pub(crate) fn cancel(
    world: &mut World,
    owner: &DocumentContext,
    revision: u64,
) -> Result<(), String> {
    let Some(mut browser) = world.remove_resource::<Browser>() else {
        return Ok(());
    };
    let result = if browser.owner.as_ref() == Some(owner) && browser.revision == revision {
        restore(world, &mut browser.motion, owner)
    } else {
        Ok(())
    };
    if let Some(a) = browser.assembly.as_ref() {
        browser.motion.changed(a);
    }
    world.insert_resource(browser);
    result
}
fn preview(
    world: &mut World,
    owner: &DocumentContext,
    revision: u64,
    s: &mut State,
) -> Result<Value, String> {
    let motion = match s.motion() {
        Ok(value) => value,
        Err(error) => {
            restore(world, s, owner)?;
            s.error = Some(error.clone());
            return Ok(json!({"valid":false,"error":error}));
        }
    };
    s.error = None;
    if s.original.is_none() {
        s.original = Some(native_viewport::interface_view_snapshot(world).2);
    }
    let serial = s.serial;
    let query_owner = owner.clone();
    worker::enqueue_query(
        world,
        owner.clone(),
        revision,
        "assembly_preview_joint_coordinates".into(),
        json!({"motion":motion}),
        move |world, services, result| {
            services.bridge.with_native_document_receipt(
                &services.engine,
                &query_owner,
                |current| {
                    if current != revision {
                        return Err("Joint motion preview belongs to an earlier model".into());
                    }
                    let mut browser = world
                        .remove_resource::<Browser>()
                        .ok_or("Assembly browser closed")?;
                    let result = (|| {
                        if browser.owner.as_ref() != Some(&query_owner)
                            || browser.revision != revision
                            || browser.motion.serial != serial
                        {
                            return Err("Joint selection changed during preview".into());
                        }
                        let s = &mut browser.motion;
                        match result {
                            Ok(result) => {
                                let solution: AssemblySolutionDto =
                                    serde_json::from_value(result.value)
                                        .map_err(|e| e.to_string())?;
                                if !solution.solved {
                                    restore(world, s, &query_owner)?;
                                    s.error = Some("The joint position cannot be solved".into());
                                    return Err("The joint position cannot be solved".into());
                                }
                                let (_, _, mut view, _) =
                                    native_viewport::interface_view_snapshot(world);
                                view.body_poses = solution.body_poses.into();
                                view.instance_body_poses = solution.instance_body_poses.into();
                                native_viewport::apply_interface_view(
                                    world,
                                    &query_owner.document_id,
                                    None,
                                    Some(view),
                                )?;
                                s.preview = true;
                                Ok(json!({"preview":true,"motion":motion,"solved":true}))
                            }
                            Err(error) => {
                                restore(world, s, &query_owner)?;
                                s.error = Some(error.clone());
                                Err(error)
                            }
                        }
                    })();
                    world.insert_resource(browser);
                    result
                },
            )
        },
    )
}
pub(super) fn reduce(
    world: &mut World,
    (engine, bridge, owner): (&AppState, &SessionBridgeState, &DocumentContext),
    revision: u64,
    s: &mut State,
    a: &AssemblyDocumentDto,
    action: &Action,
    input: &ControlInput,
) -> Result<Value, String> {
    let activate = super::super::super::is_activation(input);
    if let Action::Select(id) = action {
        if !activate {
            return Err("Select a joint".into());
        }
        let joint = a
            .joints
            .iter()
            .find(|j| j.id.0 == *id)
            .ok_or("Joint no longer exists")?;
        restore(world, s, owner)?;
        s.selected = Some(*id);
        s.changed(a);
        let (_, _, mut view, _) = native_viewport::interface_view_snapshot(world);
        view.selected_occurrence_id = None;
        view.selected_body_ids = vec![joint.connector_a.body_id.0, joint.connector_b.body_id.0];
        view.selected_face_ids.clear();
        view.selected_edge_ids.clear();
        for c in [&joint.connector_a, &joint.connector_b] {
            if let Some(edge) = c.edge_id {
                view.selected_edge_ids.push(edge.0);
            } else {
                view.selected_face_ids.push(c.face_id.0);
            }
        }
        native_viewport::apply_interface_view(world, &owner.document_id, None, Some(view))?;
        return Ok(json!({"selected_joint":id}));
    }
    let form = s.form.as_ref().ok_or("Select a joint")?;
    if !form.original.as_ref().is_some_and(|j| j.enabled) || form.axes().is_empty() {
        return Err("This joint has no enabled motion coordinates".into());
    }
    if let Action::Field(index) = action {
        if let ControlInput::SetValue(value) = input {
            if !form.axes().iter().any(|(i, _)| i == index) {
                return Err("This joint does not have that motion coordinate".into());
            }
            s.form.as_mut().unwrap().coordinates[*index].values[0].set_text(value.clone());
            s.started = None;
            s.next_frame = None;
            return preview(world, owner, revision, s);
        }
        return if activate {
            Ok(json!({"focused":true}))
        } else {
            Err("Edit the motion coordinate with its field or slider".into())
        };
    }
    if !activate {
        return Err("Activate the motion control".into());
    }
    match action {
        Action::Revert => {
            restore(world, s, owner)?;
            s.changed(a);
            Ok(json!({"reverted":true}))
        }
        Action::Demo => {
            restore(world, s, owner)?;
            s.changed(a);
            for (i, c) in s.form.as_ref().unwrap().coordinates.iter().enumerate() {
                s.base[i] = c.values[0].evaluate(UnitSystem::Mm, &[])?;
            }
            s.started = Some(Instant::now());
            s.next_frame = Some(Instant::now());
            Ok(json!({"playing":true,"duration_ms":1800}))
        }
        Action::Save => {
            s.started = None;
            s.next_frame = None;
            let motion = s.motion()?;
            mutation(
                world,
                engine,
                bridge,
                owner,
                revision,
                "assembly_set_joint_coordinates",
                json!({"motion":motion}),
            )
        }
        Action::Select(_) | Action::Field(_) => unreachable!(),
    }
}
pub(crate) fn tick(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    owner: &DocumentContext,
) -> Result<(), String> {
    let Some(mut browser) = world.remove_resource::<Browser>() else {
        return Ok(());
    };
    let result = (|| {
        let s = &mut browser.motion;
        let Some(started) = s.started else {
            return Ok(());
        };
        if !browser.enabled || browser.owner.as_ref() != Some(owner) {
            s.started = None;
            return Ok(());
        }
        handle.request_redraw();
        if s.next_frame.is_some_and(|next| Instant::now() < next) {
            return Ok(());
        }
        let revision = services
            .bridge
            .native_document_receipt(&services.engine, owner)?
            .revision;
        if revision != browser.revision {
            s.started = None;
            return Ok(());
        }
        let progress = (started.elapsed().as_secs_f64() / 1.8).min(1.);
        if progress >= 1. {
            restore(world, s, owner)?;
            if let Some(a) = &browser.assembly {
                s.changed(a);
            }
            return Ok(());
        }
        let phase = (progress * std::f64::consts::TAU).sin();
        let form = s.form.as_mut().ok_or("Joint motion editor closed")?;
        for (i, _) in form.axes() {
            let c = &mut form.coordinates[i];
            let mut amplitude = if matches!(i, 1 | 4) { 8. } else { 35_f64 };
            if c.limited {
                amplitude = amplitude
                    .min((s.base[i] - c.values[1].evaluate(UnitSystem::Mm, &[])?).max(0.))
                    .min((c.values[2].evaluate(UnitSystem::Mm, &[])? - s.base[i]).max(0.));
            }
            c.values[0].set_text((s.base[i] + phase * amplitude).to_string());
        }
        s.next_frame = Some(Instant::now() + Duration::from_millis(32));
        preview(world, owner, revision, s)?;
        Ok(())
    })();
    world.insert_resource(browser);
    result
}
