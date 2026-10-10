//! Motion studio: named poses, typed driver drafts and bounded live playback.
use super::*;
use crate::native_forms::motion_study::{self as form, Edit, Form};
use crate::native_viewport::ViewportPresentation;
use limo_cad_sketch::{JointMotionStateDto, MotionStudyEvaluationDto};
use std::time::{Duration, Instant};
pub(super) mod panel;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Choice {
    Study,
    Joint(u64),
    Coordinate(u64),
    Interpolation(u64, usize),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Field(Edit),
    PositionName(u64),
    Rename(u64),
    Capture,
    ApplyPosition(u64),
    DeletePosition(u64),
    Create,
    Delete,
    Apply,
    Revert,
    Loop,
    AddDriver,
    DeleteDriver(u64),
    Enabled(u64),
    Law(u64, bool),
    AddKey(u64),
    DeleteKey(u64, usize),
    Choice(Choice),
    Select(Choice, String),
    Play,
    Stop,
    Time,
    Export,
}
struct Picker {
    revision: u64,
    study: u64,
    receive: Mutex<std::sync::mpsc::Receiver<Option<std::path::PathBuf>>>,
}
#[derive(Default)]
pub(super) struct State {
    selected: Option<u64>,
    form: Option<Form>,
    validation: Option<Result<limo_cad_sketch::MotionStudyDto, String>>,
    names: HashMap<u64, String>,
    choice: Option<Choice>,
    original: Option<ViewportPresentation>,
    evaluation: Option<MotionStudyEvaluationDto>,
    time: f64,
    started: Option<(Instant, f64)>,
    next_frame: Option<Instant>,
    serial: u64,
    error: Option<String>,
    picker: Option<Picker>,
}
impl State {
    pub(super) fn changed(&mut self, a: &AssemblyDocumentDto) {
        self.validation = None;
        self.started = None;
        self.next_frame = None;
        self.original = None;
        self.evaluation = None;
        self.time = 0.;
        self.choice = None;
        self.error = None;
        self.serial = self.serial.wrapping_add(1);
        self.names = a
            .positions
            .iter()
            .map(|p| (p.id.0, p.name.clone()))
            .collect();
        let study = a
            .motion_studies
            .iter()
            .find(|s| Some(s.id.0) == self.selected)
            .or_else(|| a.motion_studies.first());
        self.selected = study.map(|s| s.id.0);
        self.form = study.cloned().map(Form::new);
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
    s.started = None;
    s.next_frame = None;
    s.evaluation = None;
    s.time = 0.;
    s.serial = s.serial.wrapping_add(1);
    Ok(())
}
pub(crate) fn active(world: &World) -> bool {
    world.get_resource::<Browser>().is_some_and(|b| {
        b.studies.started.is_some() || b.studies.original.is_some() || b.studies.picker.is_some()
    })
}
pub(crate) fn cancel(
    world: &mut World,
    owner: &DocumentContext,
    revision: u64,
) -> Result<(), String> {
    let Some(mut b) = world.remove_resource::<Browser>() else {
        return Ok(());
    };
    let result = if b.owner.as_ref() == Some(owner) && b.revision == revision {
        restore(world, &mut b.studies, owner)
    } else {
        Ok(())
    };
    world.insert_resource(b);
    result
}
fn evaluate(
    world: &mut World,
    owner: &DocumentContext,
    revision: u64,
    s: &mut State,
    time: f64,
) -> Result<Value, String> {
    let study = s.selected.ok_or("Create a motion study")?;
    if s.original.is_none() {
        s.original = Some(native_viewport::interface_view_snapshot(world).2);
    }
    let previous = s.evaluation.as_ref().map(|e| e.sample.time_seconds);
    let serial = s.serial;
    let query_owner = owner.clone();
    s.error = None;
    worker::enqueue_query(
        world,
        owner.clone(),
        revision,
        "assembly_evaluate_motion_study".into(),
        json!({"study_id":study,"time_seconds":time,"previous_time_seconds":previous,"enforce_contacts":true}),
        move |world, services, result| {
            services.bridge.with_native_document_receipt(&services.engine,&query_owner,|current|{
            if current!=revision {return Err("Motion study changed during playback".into());}
            let mut b=world.remove_resource::<Browser>().ok_or("Assembly browser closed")?;
            let result=(||{
                if b.owner.as_ref()!=Some(&query_owner)||b.revision!=revision||b.studies.serial!=serial||b.studies.selected!=Some(study){return Err("Motion preview was superseded".into());}
                let s=&mut b.studies;
                match result {
                    Ok(result)=>{
                        let e:MotionStudyEvaluationDto=serde_json::from_value(result.value).map_err(|e|e.to_string())?;
                        if !e.sample.solution.solved {return Err("Motion study has an unsolved position".into());}
                        let (_,_,mut view,_)=native_viewport::interface_view_snapshot(world);
                        view.body_poses=e.sample.solution.body_poses.clone().into();view.instance_body_poses=e.sample.solution.instance_body_poses.clone().into();
                        native_viewport::apply_interface_view(world,&query_owner.document_id,None,Some(view))?;
                        s.time=e.sample.time_seconds;if e.stopped_by_contact.is_some(){s.started=None;}
                        let result=json!({"time_seconds":s.time,"stopped_by_contact":e.stopped_by_contact,"preview":true});s.evaluation=Some(e);Ok(result)
                    }
                    Err(e)=>{restore(world,s,&query_owner)?;s.error=Some(e.clone());Err(e)}
                }
            })();world.insert_resource(b);result
        })
        },
    )
}
fn key<T: serde::Serialize>(value: T) -> String {
    serde_json::to_value(value)
        .unwrap()
        .as_str()
        .unwrap()
        .into()
}
fn options(s: &State, a: &AssemblyDocumentDto, choice: Choice) -> Vec<(String, String)> {
    match choice {
        Choice::Study => a
            .motion_studies
            .iter()
            .map(|v| (v.id.0.to_string(), v.name.clone()))
            .collect(),
        Choice::Joint(_) => a
            .joints
            .iter()
            .filter(|j| !form::coordinates(a, j).is_empty())
            .map(|j| (j.id.0.to_string(), j.name.clone()))
            .collect(),
        Choice::Coordinate(id) => s
            .form
            .as_ref()
            .and_then(|f| f.driver(id).ok())
            .and_then(|d| a.joints.iter().find(|j| j.id == d.record.joint_id))
            .map(|j| {
                form::coordinates(a, j)
                    .into_iter()
                    .map(|(c, label)| (key(c), label.into()))
                    .collect()
            })
            .unwrap_or_default(),
        Choice::Interpolation(..) => ["step", "linear", "smooth"]
            .map(|v| (v.into(), v.into()))
            .into(),
    }
}
fn selected(s: &State, choice: Choice) -> String {
    match choice {
        Choice::Study => s.selected.map(|id| id.to_string()).unwrap_or_default(),
        Choice::Joint(id) => s
            .form
            .as_ref()
            .and_then(|f| f.driver(id).ok())
            .map(|d| d.record.joint_id.0.to_string())
            .unwrap_or_default(),
        Choice::Coordinate(id) => s
            .form
            .as_ref()
            .and_then(|f| f.driver(id).ok())
            .map(|d| key(d.record.coordinate))
            .unwrap_or_default(),
        Choice::Interpolation(id, i) => s
            .form
            .as_ref()
            .and_then(|f| f.driver(id).ok())
            .and_then(|d| d.keys.get(i))
            .map(|k| key(k.interpolation))
            .unwrap_or_default(),
    }
}
pub(super) fn reduce(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    (engine, bridge, owner): (&AppState, &SessionBridgeState, &DocumentContext),
    revision: u64,
    s: &mut State,
    a: &AssemblyDocumentDto,
    (action, input): (&Action, &ControlInput),
) -> Result<Value, String> {
    let activate = super::super::super::is_activation(input);
    if !matches!(
        action,
        Action::Time | Action::Play | Action::Stop | Action::Choice(_)
    ) {
        s.validation = None;
    }
    if let Action::Time = action {
        if let ControlInput::SetValue(v) = input {
            s.started = None;
            let time = form::number(v)?;
            let duration = s
                .form
                .as_ref()
                .ok_or("Create a study")?
                .original
                .duration_seconds;
            if !(0. ..=duration).contains(&time) {
                return Err("Time is outside the study duration".into());
            }
            return evaluate(world, owner, revision, s, time);
        }
        return if activate {
            Ok(json!({"focused":true}))
        } else {
            Err("Set the timeline position".into())
        };
    }
    if let Action::Field(edit) = action {
        match input {
            ControlInput::SetValue(text) => {
                restore(world, s, owner)?;
                s.form
                    .as_mut()
                    .ok_or("Create a study")?
                    .set(*edit, text.clone())?;
                return Ok(json!({"draft":true}));
            }
            ControlInput::Key(key) if key.key == "Enter" => {
                return reduce(
                    world,
                    handle,
                    (engine, bridge, owner),
                    revision,
                    s,
                    a,
                    (&Action::Apply, &ControlInput::Click),
                )
            }
            _ if activate => return Ok(json!({"focused":true})),
            _ => return Err("Edit the study field with text".into()),
        }
    }
    if let Action::PositionName(id) = action {
        match input {
            ControlInput::SetValue(value) => {
                *s.names.get_mut(id).ok_or("Position no longer exists")? = value.clone();
                return Ok(json!({"draft":true}));
            }
            ControlInput::Key(key) if key.key == "Enter" => {
                return reduce(
                    world,
                    handle,
                    (engine, bridge, owner),
                    revision,
                    s,
                    a,
                    (&Action::Rename(*id), &ControlInput::Click),
                )
            }
            _ if activate => return Ok(json!({"focused":true})),
            _ => return Err("Edit the position name with text".into()),
        }
    }
    if let Action::Choice(choice) = action {
        let value = match input {
            ControlInput::SetValue(v) => Some(v.clone()),
            ControlInput::Key(k) if !activate => {
                if k.ctrl || k.alt || k.meta || k.shift {
                    return Err("Unsupported choice key".into());
                }
                let options = options(s, a, *choice);
                let index = options
                    .iter()
                    .position(|(v, _)| *v == selected(s, *choice))
                    .unwrap_or(0);
                let index = match k.key.as_str() {
                    "Home" => 0,
                    "End" => options.len().saturating_sub(1),
                    "ArrowDown" => index.saturating_add(1).min(options.len().saturating_sub(1)),
                    "ArrowUp" => index.saturating_sub(1),
                    _ => return Err("Unsupported choice key".into()),
                };
                Some(options.get(index).ok_or("No available choices")?.0.clone())
            }
            _ if activate => {
                s.choice = if s.choice == Some(*choice) {
                    None
                } else {
                    Some(*choice)
                };
                return Ok(json!({"opened":s.choice.is_some()}));
            }
            _ => return Err("Choose an available option".into()),
        };
        return reduce(
            world,
            handle,
            (engine, bridge, owner),
            revision,
            s,
            a,
            (
                &Action::Select(*choice, value.unwrap()),
                &ControlInput::Click,
            ),
        );
    }
    if !activate {
        return Err("Activate a motion control".into());
    }
    if let Action::Select(choice, value) = action {
        if !options(s, a, *choice).iter().any(|(v, _)| v == value) {
            return Err("Choice no longer available".into());
        }
        restore(world, s, owner)?;
        s.choice = None;
        match *choice {
            Choice::Study => {
                s.selected = Some(value.parse().map_err(|_| "Choose a study")?);
                s.changed(a);
            }
            Choice::Joint(id) => {
                let joint = a
                    .joints
                    .iter()
                    .find(|j| j.id.0.to_string() == *value)
                    .ok_or("Joint missing")?;
                let d = s.form.as_mut().unwrap().driver_mut(id)?;
                d.record.joint_id = joint.id;
                d.record.coordinate = form::coordinates(a, joint)[0].0;
            }
            Choice::Coordinate(id) => {
                s.form.as_mut().unwrap().driver_mut(id)?.record.coordinate =
                    serde_json::from_value(json!(value)).map_err(|e| e.to_string())?
            }
            Choice::Interpolation(id, index) => {
                s.form
                    .as_mut()
                    .unwrap()
                    .driver_mut(id)?
                    .keys
                    .get_mut(index)
                    .ok_or("Keyframe missing")?
                    .interpolation =
                    serde_json::from_value(json!(value)).map_err(|e| e.to_string())?
            }
        }
        return Ok(json!({"selected":value}));
    }
    let request = match *action {
        Action::Capture => {
            let motions = s
                .evaluation
                .as_ref()
                .map(|e| e.sample.joint_motions.clone())
                .unwrap_or_else(|| {
                    a.joints
                        .iter()
                        .map(|j| JointMotionStateDto {
                            joint_id: j.id,
                            angle_offset_deg: j.angle_offset_deg,
                            linear_offset_mm: j.linear_offset_mm,
                            secondary_angle_offset_deg: j.advanced.secondary_angle_offset_deg,
                            tertiary_angle_offset_deg: j.advanced.tertiary_angle_offset_deg,
                            secondary_linear_offset_mm: j.advanced.secondary_linear_offset_mm,
                        })
                        .collect()
                });
            (
                "assembly_create_position",
                json!({"name":format!("Position {}",a.next_position_id),"motions":motions}),
            )
        }
        Action::Rename(id) => {
            let mut p = a
                .positions
                .iter()
                .find(|p| p.id.0 == id)
                .ok_or("Position missing")?
                .clone();
            p.name = s.names.get(&id).ok_or("Position missing")?.trim().into();
            if p.name.is_empty() {
                return Err("Enter a position name".into());
            }
            ("assembly_update_position", json!(p))
        }
        Action::ApplyPosition(id) => ("assembly_apply_position", json!({"position_id":id})),
        Action::DeletePosition(id) => ("assembly_delete_position", json!({"position_id":id})),
        Action::Create => {
            s.selected = Some(a.next_motion_study_id);
            (
                "assembly_create_motion_study",
                json!({"name":format!("Study {}",a.next_motion_study_id),"duration_seconds":5.}),
            )
        }
        Action::Delete => (
            "assembly_delete_motion_study",
            json!({"study_id":s.selected.ok_or("Choose a study")?}),
        ),
        Action::Apply => (
            "assembly_update_motion_study",
            json!(s.form.as_ref().ok_or("Create a study")?.value(a)?),
        ),
        Action::Revert => {
            restore(world, s, owner)?;
            s.changed(a);
            return Ok(json!({"reverted":true}));
        }
        Action::Play => {
            let f = s.form.as_ref().ok_or("Create a study")?;
            if f.value(a)? != f.original {
                return Err("Apply the study changes before playback".into());
            }
            if s.started.take().is_some() {
                return Ok(json!({"playing":false}));
            }
            if s.time >= f.original.duration_seconds {
                s.time = 0.;
                s.evaluation = None;
            }
            s.started = Some((Instant::now(), s.time));
            s.next_frame = None;
            return Ok(json!({"playing":true}));
        }
        Action::Stop => {
            restore(world, s, owner)?;
            return Ok(json!({"stopped":true}));
        }
        Action::Export => {
            if s.picker.is_some() {
                return Err("The export chooser is already open".into());
            }
            let f = s.form.as_ref().ok_or("Create a study")?;
            let name = format!(
                "{}-motion-path.csv",
                f.original
                    .name
                    .replace(['<', '>', ':', '"', '/', '\\', '|', '?', '*'], "_")
            );
            let (dialog, parent) = super::super::files::parented_dialog(
                world,
                rfd::FileDialog::new()
                    .add_filter("Motion path CSV", &["csv"])
                    .set_file_name(name),
            )?;
            let (tx, rx) = std::sync::mpsc::channel();
            let wake = handle.clone();
            std::thread::Builder::new()
                .name("cad-motion-path-picker".into())
                .spawn(move || {
                    let _parent = parent;
                    let path = dialog.save_file();
                    let _ = tx.send(path);
                    wake.request_redraw();
                })
                .map_err(|e| e.to_string())?;
            s.picker = Some(Picker {
                revision,
                study: f.original.id.0,
                receive: Mutex::new(rx),
            });
            return Ok(json!({"awaiting_input":true}));
        }
        _ => {
            restore(world, s, owner)?;
            let f = s.form.as_mut().ok_or("Create a study")?;
            match *action {
                Action::Loop => f.looped = !f.looped,
                Action::AddDriver => f.add_driver(a)?,
                Action::DeleteDriver(id) => f.drivers.retain(|d| d.record.id.0 != id),
                Action::Enabled(id) => {
                    let d = f.driver_mut(id)?;
                    d.record.enabled = !d.record.enabled;
                }
                Action::Law(id, motor) => f.driver_mut(id)?.is_motor = motor,
                Action::AddKey(id) => {
                    let duration = f.original.duration_seconds;
                    f.driver_mut(id)?.add_key(duration)?;
                }
                Action::DeleteKey(id, index) => {
                    let d = f.driver_mut(id)?;
                    if d.keys.len() <= 1 || index >= d.keys.len() {
                        return Err("Keep at least one keyframe".into());
                    }
                    d.keys.remove(index);
                }
                _ => unreachable!(),
            }
            return Ok(json!({"draft":true}));
        }
    };
    restore(world, s, owner)?;
    mutation(world, engine, bridge, owner, revision, request.0, request.1)
}
pub(crate) fn tick(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    owner: &DocumentContext,
) -> Result<(), String> {
    let Some(mut b) = world.remove_resource::<Browser>() else {
        return Ok(());
    };
    let result = (|| {
        if b.owner.as_ref() != Some(owner) {
            return Ok(());
        }
        let s = &mut b.studies;
        if let Some(picker) = &s.picker {
            let path = match picker
                .receive
                .lock()
                .map_err(|_| "Export chooser lock poisoned")?
                .try_recv()
            {
                Ok(p) => Some(p),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                Err(_) => return Err("Export chooser disconnected".into()),
            };
            if let Some(path) = path {
                let picker = s.picker.take().unwrap();
                if let Some(path) = path {
                    let owner = owner.clone();
                    worker::enqueue_document_io(
                        world,
                        "assembly_export_motion_path_csv".into(),
                        move |services, guard| {
                            services.bridge.with_native_document_receipt(&services.engine,&owner,|revision|{
                        if revision!=picker.revision {return Err("The study changed while choosing its export path".into());}
                        guard.validate()?;
                        let csv=parse_engine_envelope(services.engine.engine_call("assembly_export_motion_path_csv",&json!({"study_id":picker.study,"sample_rate_hz":60,"occurrence_ids":[]}).to_string()))?;
                        limo_cad_project_file::write_binary_file_atomic(&path,csv.as_str().ok_or("Motion path was not text")?.as_bytes()).map_err(|e|e.to_string())?;
                        Ok(NativeMutationResult{context:owner.clone(),engine_revision:revision,value:json!({"path":path,"exported":true})})
                    })
                        },
                        |_, _, r| Ok(r?.value),
                    )?;
                    return Ok(());
                }
            }
        }
        let Some((started, base)) = s.started else {
            return Ok(());
        };
        if !b.enabled || b.tab != Tab::Motion {
            return restore(world, s, owner);
        }
        handle.request_redraw();
        if s.next_frame.is_some_and(|next| Instant::now() < next) {
            return Ok(());
        }
        let revision = services
            .bridge
            .native_document_receipt(&services.engine, owner)?
            .revision;
        if revision != b.revision {
            s.started = None;
            return Ok(());
        }
        let f = s.form.as_ref().ok_or("Motion study disappeared")?;
        let duration = f.original.duration_seconds;
        let mut time = base + started.elapsed().as_secs_f64() * f.original.playback_speed;
        if time >= duration {
            if f.original.looped {
                time %= duration;
                s.evaluation = None;
                s.started = Some((Instant::now(), time));
            } else {
                time = duration;
                s.started = None;
            }
        }
        s.next_frame = Some(Instant::now() + Duration::from_millis(32));
        evaluate(world, owner, revision, s, time)?;
        Ok(())
    })();
    world.insert_resource(b);
    result
}
