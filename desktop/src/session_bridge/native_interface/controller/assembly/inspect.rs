//! Exact interference and contact-stop controls for the assembly sidebar.
use super::*;
use limo_cad_sketch::{InterferenceReportDto, SweptCollisionReportDto};
pub(super) mod panel;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Edit {
    Clearance,
    SampleRate,
    ContactName(u64),
    ContactClearance(u64),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Choice {
    First,
    Second,
    Study,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Field(Edit),
    Choice(Choice),
    Select(Choice, String),
    StopFirst,
    Check,
    Swept,
    CreateContact,
    ApplyContact(u64),
    Enabled(u64),
    Stop(u64),
    Delete(u64),
}
struct ContactDraft {
    name: String,
    clearance: MeasurementInput,
}
pub(super) struct State {
    clearance: MeasurementInput,
    sample_rate: String,
    first: Option<(u64, u64)>,
    second: Option<(u64, u64)>,
    study: Option<u64>,
    choice: Option<Choice>,
    stop_first: bool,
    contacts: HashMap<u64, ContactDraft>,
    report: Option<InterferenceReportDto>,
    swept: Option<SweptCollisionReportDto>,
    error: Option<String>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            clearance: MeasurementInput::new(DimensionKind::Length, 0., UnitSystem::Mm),
            sample_rate: "120".into(),
            first: None,
            second: None,
            study: None,
            choice: None,
            stop_first: false,
            contacts: default(),
            report: None,
            swept: None,
            error: None,
        }
    }
}
impl State {
    pub(super) fn changed(&mut self) {
        self.report = None;
        self.swept = None;
        self.contacts.clear();
        self.error = None;
    }
    fn clearance(&self, units: UnitSystem) -> Result<f64, String> {
        let value = self.clearance.evaluate(units, &[])?;
        if value < 0. {
            return Err("Clearance cannot be negative".into());
        }
        Ok(value)
    }
    fn sample_rate(&self) -> Result<f64, String> {
        let value = self
            .sample_rate
            .parse::<f64>()
            .map_err(|_| "Enter a sample rate from 1 to 240 Hz")?;
        if !value.is_finite() || !(1. ..=240.).contains(&value) {
            return Err("Sample rate must be from 1 to 240 Hz".into());
        }
        Ok(value)
    }
    fn update(&mut self, a: &AssemblyDocumentDto, units: UnitSystem, placed: &[(u64, u64)]) {
        if self.first.is_none_or(|key| !placed.contains(&key)) {
            self.first = placed.first().copied();
        }
        if self.second.is_none_or(|key| !placed.contains(&key)) {
            self.second = placed.iter().copied().find(|key| Some(*key) != self.first);
        }
        if !a.motion_studies.iter().any(|s| Some(s.id.0) == self.study) {
            self.study = a.motion_studies.first().map(|s| s.id.0);
        }
        for c in &a.contact_sets {
            self.contacts.entry(c.id.0).or_insert_with(|| ContactDraft {
                name: c.name.clone(),
                clearance: MeasurementInput::new(DimensionKind::Length, c.clearance_mm, units),
            });
        }
    }
}
fn choices(kind: Choice, a: &AssemblyDocumentDto, placed: &[(u64, u64)]) -> Vec<(String, String)> {
    if kind == Choice::Study {
        return a
            .motion_studies
            .iter()
            .map(|s| (s.id.0.to_string(), s.name.clone()))
            .collect();
    }
    placed
        .iter()
        .map(|(o, b)| {
            let name = a
                .component_structure
                .occurrences
                .iter()
                .find(|v| v.id.0 == *o)
                .map(|v| v.name.as_str())
                .unwrap_or("Component");
            (format!("{o}:{b}"), format!("{name} · Body {b} (O{o})"))
        })
        .collect()
}
fn selected(s: &State, choice: Choice) -> String {
    match choice {
        Choice::First => s.first.map(|(o, b)| format!("{o}:{b}")).unwrap_or_default(),
        Choice::Second => s
            .second
            .map(|(o, b)| format!("{o}:{b}"))
            .unwrap_or_default(),
        Choice::Study => s.study.map(|id| id.to_string()).unwrap_or_default(),
    }
}
fn placed(world: &World) -> Vec<(u64, u64)> {
    native_viewport::interface_view(world)
        .2
        .instance_body_poses
        .iter()
        .filter(|p| p.visible)
        .map(|p| (p.occurrence_id.0, p.body_id.0))
        .collect()
}
pub(super) fn reduce(
    world: &mut World,
    (engine, bridge, owner): (&AppState, &SessionBridgeState, &DocumentContext),
    revision: u64,
    state: &mut State,
    a: &AssemblyDocumentDto,
    units: UnitSystem,
    (action, input): (&Action, &ControlInput),
) -> Result<Value, String> {
    let placed = placed(world);
    state.update(a, units, &placed);
    let activate = super::super::super::is_activation(input);
    if let Action::Field(field) = action {
        if let ControlInput::SetValue(value) = input {
            match field {
                Edit::Clearance => state.clearance.set_text(value.clone()),
                Edit::SampleRate => state.sample_rate = value.clone(),
                Edit::ContactName(id) => {
                    state
                        .contacts
                        .get_mut(id)
                        .ok_or("Contact no longer exists")?
                        .name = value.clone()
                }
                Edit::ContactClearance(id) => state
                    .contacts
                    .get_mut(id)
                    .ok_or("Contact no longer exists")?
                    .clearance
                    .set_text(value.clone()),
            }
            return Ok(json!({"changed":true}));
        }
        return if activate {
            Ok(json!({"focused":true}))
        } else {
            Err("Edit the contact field with text".into())
        };
    }
    let selected_choice = match (action, input) {
        (Action::Select(kind, value), _) if activate => Some((*kind, value.clone())),
        (Action::Choice(kind), ControlInput::SetValue(value)) => Some((*kind, value.clone())),
        (Action::Choice(kind), ControlInput::Key(key))
            if !activate && !key.ctrl && !key.meta && !key.alt && !key.shift =>
        {
            let options = choices(*kind, a, &placed);
            let current = selected(state, *kind);
            let index = options.iter().position(|(v, _)| *v == current).unwrap_or(0);
            let next = match key.key.as_str() {
                "ArrowDown" => (index + 1).min(options.len().saturating_sub(1)),
                "ArrowUp" => index.saturating_sub(1),
                "Home" => 0,
                "End" => options.len().saturating_sub(1),
                _ => return Err("Unsupported choice key".into()),
            };
            Some((
                *kind,
                options.get(next).ok_or("No available choices")?.0.clone(),
            ))
        }
        _ => None,
    };
    if let Some((kind, value)) = selected_choice {
        if !choices(kind, a, &placed).iter().any(|(v, _)| *v == value) {
            return Err("Choose an available body instance or study".into());
        }
        match kind {
            Choice::Study => state.study = value.parse().ok(),
            Choice::First | Choice::Second => {
                let (o, b) = value.split_once(':').ok_or("Choose a placed body")?;
                let key = Some((
                    o.parse().map_err(|_| "Invalid occurrence")?,
                    b.parse().map_err(|_| "Invalid body")?,
                ));
                if kind == Choice::First {
                    state.first = key;
                } else {
                    state.second = key;
                }
            }
        }
        state.choice = None;
        return Ok(json!({"changed":true}));
    }
    if !activate {
        return Err("Activate the inspection control".into());
    }
    let mut request = None;
    match action {
        Action::Choice(kind) => {
            state.choice = if state.choice == Some(*kind) {
                None
            } else {
                Some(*kind)
            }
        }
        Action::StopFirst => state.stop_first = !state.stop_first,
        Action::Check | Action::Swept => {
            let clearance = state.clearance(units)?;
            let swept = matches!(action, Action::Swept);
            let (operation, args) = if swept {
                (
                    "assembly_swept_collision_check",
                    json!({"study_id":state.study.ok_or("Create a motion study first")?,"sample_rate_hz":state.sample_rate()?,"clearance_threshold_mm":clearance,"stop_at_first":state.stop_first}),
                )
            } else {
                (
                    "assembly_interference_check",
                    json!({"occurrence_ids":[],"clearance_threshold_mm":clearance}),
                )
            };
            state.error = None;
            let query_owner = owner.clone();
            return worker::enqueue_query(
                world,
                owner.clone(),
                revision,
                operation.into(),
                args,
                move |world, services, result| {
                    services.bridge.with_native_document_receipt(
                        &services.engine,
                        &query_owner,
                        |current| {
                            if current != revision {
                                return Err(
                                    "The assembly changed during inspection; run the check again"
                                        .into(),
                                );
                            }
                            let mut browser = world
                                .get_resource_mut::<Browser>()
                                .ok_or("Assembly inspector closed")?;
                            if browser.owner.as_ref() != Some(&query_owner)
                                || browser.revision != revision
                            {
                                return Err("Assembly inspector changed".into());
                            }
                            match result {
                                Ok(result) => {
                                    if swept {
                                        browser.inspect.swept = Some(
                                            serde_json::from_value(result.value.clone())
                                                .map_err(|e| e.to_string())?,
                                        );
                                    } else {
                                        browser.inspect.report = Some(
                                            serde_json::from_value(result.value.clone())
                                                .map_err(|e| e.to_string())?,
                                        );
                                    }
                                    Ok(result.value)
                                }
                                Err(error) => {
                                    browser.inspect.error = Some(error.clone());
                                    Err(error)
                                }
                            }
                        },
                    )
                },
            );
        }
        Action::CreateContact => {
            let ca = state.first.ok_or("Choose the first placed body")?;
            let cb = state.second.ok_or("Choose the second placed body")?;
            if ca == cb {
                return Err("Choose two different placed bodies".into());
            }
            request = Some((
                "assembly_create_contact_set",
                json!({"name":format!("Contact {}",a.next_contact_set_id),"occurrence_a":ca.0,"body_a":ca.1,"occurrence_b":cb.0,"body_b":cb.1,"clearance_mm":state.clearance(units)?,"stop_motion":true}),
            ));
        }
        Action::ApplyContact(id) | Action::Enabled(id) | Action::Stop(id) | Action::Delete(id) => {
            let mut contact = a
                .contact_sets
                .iter()
                .find(|c| c.id.0 == *id)
                .cloned()
                .ok_or("Contact no longer exists")?;
            if matches!(action, Action::Delete(_)) {
                request = Some(("assembly_delete_contact_set", json!({"contact_id":id})));
            } else {
                match action {
                    Action::Enabled(_) => contact.enabled = !contact.enabled,
                    Action::Stop(_) => contact.stop_motion = !contact.stop_motion,
                    Action::ApplyContact(_) => {
                        let draft = state.contacts.get(id).ok_or("Contact editor changed")?;
                        contact.name = draft.name.trim().into();
                        contact.clearance_mm = draft.clearance.evaluate(units, &[])?;
                        if contact.name.is_empty() || contact.clearance_mm < 0. {
                            return Err("Enter a name and a nonnegative contact clearance".into());
                        }
                    }
                    _ => unreachable!(),
                }
                request = Some((
                    "assembly_update_contact_set",
                    serde_json::to_value(contact).map_err(|e| e.to_string())?,
                ));
            }
        }
        Action::Field(_) | Action::Select(..) => unreachable!(),
    }
    if let Some((operation, args)) = request {
        mutation(world, engine, bridge, owner, revision, operation, args)
    } else {
        Ok(json!({"changed":true}))
    }
}
