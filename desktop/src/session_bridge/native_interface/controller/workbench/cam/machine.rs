//! Explicit machine/profile selection and post settings on the shared setup.
//! Unknown shop coordinates remain operator inputs. Existing machine topology,
//! legacy metadata and profile extensions survive unrelated edits unchanged.
use super::*;
use limo_cad_cam::{
    CamMachineAssignmentDto, CamPostConfigDto, CamUnits, PostDialect, Siemens828dPostConfigDto,
};
use std::sync::Arc;

mod library;
pub(super) mod management;
mod post;

const PREFIX: &str = "/native/machine/";
const SOURCE: &str = "/native/machine/source";
const SECTION: &str = "/native/ui/setup_section";

#[derive(Default)]
pub(super) struct Snapshot {
    pub(super) profiles: Vec<(String, CamMachineAssignmentDto)>,
    pub(super) notice: String,
}

pub(super) use library::{busy, poll, reload, save_profile, snapshot};

pub(super) struct Context {
    profiles: HashMap<String, CamMachineAssignmentDto>,
    selected: Option<CamMachineAssignmentDto>,
    selected_source: String,
}

fn starter_labels() -> Vec<ChoiceOption> {
    form::options(&[
        ("siemens828d", "Siemens 828D · native · 3-axis starter"),
        ("fanuc", "Generic FANUC-style · 3-axis starter"),
        ("haas", "Haas NGC · 3-axis starter"),
        ("mitsubishi", "Mitsubishi M80/M800 · 3-axis starter"),
        ("mazak", "Mazak EIA milling · 3-axis starter"),
        ("syntec", "Syntec milling · 3-axis starter"),
        ("okuma", "Okuma OSP milling · 3-axis starter"),
        ("heidenhain", "Heidenhain TNC · 3-axis starter"),
        (
            "hermle_heidenhain",
            "Hermle / Heidenhain · fixed 3-axis starter",
        ),
        ("linux_cnc", "LinuxCNC · 3-axis starter"),
        ("grbl", "GRBL · 3-axis starter"),
    ])
}
fn starter(dialect: &str) -> Result<CamMachineAssignmentDto, String> {
    let dialect: PostDialect = serde_json::from_value(json!(dialect))
        .map_err(|_| "Choose an available controller starter")?;
    let post = CamPostConfigDto {
        dialect,
        sequence_numbers: matches!(
            dialect,
            PostDialect::Siemens828d | PostDialect::Heidenhain | PostDialect::HermleHeidenhain
        ),
        siemens_828d: (dialect == PostDialect::Siemens828d).then(Siemens828dPostConfigDto::default),
        ..Default::default()
    };
    let mut assignment = CamMachineAssignmentDto::three_axis(post);
    assignment.profile.id = uuid::Uuid::new_v4().to_string();
    Ok(assignment)
}

pub(super) fn extend(
    draft: &mut Draft,
    cam: &CamDocumentDto,
    private: Arc<Snapshot>,
) -> Result<(), String> {
    let Selection::Setup(id) = draft.selection else {
        return Ok(());
    };
    let setup = cam.setup(id).ok_or("Setup was removed")?;
    let selected = setup.machine.clone();
    let selected_source = if selected.is_some() {
        "snapshot"
    } else {
        "generic"
    }
    .to_owned();
    let mut profiles = HashMap::new();
    if let Some(selected) = selected.as_ref() {
        profiles.insert("snapshot".into(), selected.clone());
    }
    let section = draft.fields.len();
    form::push(
        draft,
        SECTION,
        "Setup section",
        InputKind::Choice,
        json!("setup"),
        cam.units,
        Some(form::options(&[
            ("setup", "Stock and coordinates"),
            ("machine", "Machine and post"),
            ("private_posts", "Private post library"),
        ])),
    );
    let field = draft.fields.remove(section);
    draft.fields.insert(0, field);
    form::push(
        draft,
        SOURCE,
        "Machine / controller",
        InputKind::Choice,
        json!(selected_source),
        cam.units,
        None,
    );
    draft.machine = Some(Context {
        profiles,
        selected,
        selected_source,
    });
    refresh_library(draft, cam, private);
    extend_fields(draft, cam.units)?;
    Ok(())
}

pub(super) fn refresh_library(draft: &mut Draft, cam: &CamDocumentDto, private: Arc<Snapshot>) {
    let Some(context) = draft.machine.as_mut() else {
        return;
    };
    let mut options = form::options(&[("generic", "No machine assigned")]);
    if let Some(snapshot) = context.profiles.get("snapshot") {
        options.push(ChoiceOption {
            value: "snapshot".into(),
            label: format!("Project snapshot · {}", snapshot.profile.name),
            disabled: false,
        });
    }
    for setup in &cam.setups {
        if let Some(machine) = setup.machine.as_ref() {
            if Some(setup.id)
                == match draft.selection {
                    Selection::Setup(id) => Some(id),
                    _ => None,
                }
            {
                continue;
            }
            let key = format!("setup:{}", setup.id);
            context.profiles.insert(key.clone(), machine.clone());
            options.push(ChoiceOption {
                value: key,
                label: format!("{} · {}", setup.name, machine.profile.name),
                disabled: false,
            });
        }
    }
    options.extend(starter_labels().into_iter().map(|mut option| {
        option.value = format!("starter:{}", option.value);
        option
    }));
    for (file, machine) in &private.profiles {
        let key = format!("private:{file}");
        context.profiles.insert(key.clone(), machine.clone());
        options.push(ChoiceOption {
            value: key,
            label: format!("Private · {} · {file}", machine.profile.name),
            disabled: false,
        });
    }
    if !private.notice.is_empty() {
        options.push(ChoiceOption {
            value: "library_notice".into(),
            label: private.notice.clone(),
            disabled: true,
        });
    }
    if let Some(selected) = context.selected.as_ref() {
        if context.selected_source.starts_with("private:") {
            let label = format!("Selected snapshot · {}", selected.profile.name);
            if let Some(option) = options
                .iter_mut()
                .find(|option| option.value == context.selected_source)
            {
                option.label = label;
            } else {
                options.push(ChoiceOption {
                    value: context.selected_source.clone(),
                    label,
                    disabled: false,
                });
            }
        }
    }
    if let Some(field) = draft.fields.iter_mut().find(|field| field.path == SOURCE) {
        field.options = Some(options);
    }
}

fn extend_fields(draft: &mut Draft, units: CamUnits) -> Result<(), String> {
    draft
        .fields
        .retain(|field| !field.path.starts_with(PREFIX) || field.path == SOURCE);
    let Some(machine) = draft
        .machine
        .as_ref()
        .and_then(|context| context.selected.as_ref())
    else {
        return Ok(());
    };
    let record = serde_json::to_value(machine).map_err(|e| e.to_string())?;
    form::push(
        draft,
        &format!("{PREFIX}name"),
        "Shop machine name",
        InputKind::Name,
        record["profile"]["name"].clone(),
        units,
        None,
    );
    post::extend(draft, &record["profile"]["post"], units);
    Ok(())
}

pub(super) fn changed(draft: &mut Draft, units: CamUnits, path: &str) -> Result<(), String> {
    if path != SOURCE {
        return Ok(());
    }
    let source = form::text(draft, SOURCE)?.to_owned();
    if draft
        .machine
        .as_ref()
        .is_some_and(|context| context.selected_source == source)
    {
        return Ok(());
    }
    let mut context = draft.machine.take().ok_or("Reopen the machine editor")?;
    let result: Result<(), String> = (|| {
        let selected = if source == "generic" {
            None
        } else if let Some(dialect) = source.strip_prefix("starter:") {
            Some(starter(dialect)?)
        } else {
            Some(
                context
                    .profiles
                    .get(&source)
                    .cloned()
                    .ok_or("Choose an available machine profile")?,
            )
        };
        context.selected = selected;
        context.selected_source = source.clone();
        Ok(())
    })();
    draft.machine = Some(context);
    result?;
    extend_fields(draft, units)?;
    if source == "starter:siemens828d" {
        form::set(
            draft,
            &format!("{PREFIX}post/siemens_828d/supa_retract_z"),
            "",
        );
    }
    Ok(())
}

pub(super) fn visible(draft: &Draft, path: &str) -> bool {
    if draft.machine.is_none() {
        return true;
    }
    if path == SECTION {
        return true;
    }
    if management_visible(draft) {
        return false;
    }
    if form::text(draft, SECTION).unwrap_or("setup") != "machine" {
        return !path.starts_with(PREFIX);
    }
    path.starts_with(PREFIX) && post::visible(draft, path)
}
pub(super) fn management_visible(draft: &Draft) -> bool {
    draft.machine.is_some() && form::text(draft, SECTION).unwrap_or("setup") == "private_posts"
}
pub(super) fn retain_section(previous: Option<&Draft>, next: &mut Draft) {
    if let Some(previous) = previous.filter(|previous| previous.selection == next.selection) {
        if let Ok(section) = form::text(previous, SECTION) {
            form::set(next, SECTION, section);
        }
    }
}

pub(super) fn preview(draft: &Draft, private: Arc<Snapshot>) -> Option<String> {
    if draft.machine.is_none() || form::text(draft, SECTION).ok()? != "machine" {
        return None;
    }
    if !private.notice.is_empty() {
        return Some(private.notice.clone());
    }
    Some(if draft.dirty() {
        "Unapplied machine settings. Apply before posting or saving a profile.".into()
    } else {
        "Machine coordinates come from your machine settings.\nSave profile adds this snapshot to the private library.".into()
    })
}

pub(super) fn apply(draft: &Draft, record: &mut Value, units: CamUnits) -> Result<(), String> {
    if !form::changed(draft, PREFIX) {
        return Ok(());
    }
    let context = draft.machine.as_ref().ok_or("Reopen the machine editor")?;
    if form::text(draft, SOURCE)? != context.selected_source {
        return Err("Resolve the selected machine profile before applying".into());
    }
    let Some(original) = context.selected.as_ref() else {
        record["machine"] = Value::Null;
        return Ok(());
    };
    let mut machine = serde_json::to_value(original).map_err(|e| e.to_string())?;
    if form::changed(draft, &format!("{PREFIX}name")) {
        let name = form::text(draft, &format!("{PREFIX}name"))?;
        if name.is_empty() {
            return Err("Enter a shop machine name".into());
        }
        machine["profile"]["name"] = json!(name);
    }
    post::apply(draft, &mut machine["profile"]["post"], units)?;
    let mut machine: CamMachineAssignmentDto =
        serde_json::from_value(machine).map_err(|e| e.to_string())?;
    if machine
        .profile
        .post
        .siemens_828d
        .as_ref()
        .is_some_and(|post| post.spindle_stop_subprogram.is_some())
    {
        machine.profile.schema_version = machine.profile.schema_version.max(2);
    }
    if machine != *original {
        machine.profile.revision = original
            .profile
            .revision
            .checked_add(1)
            .ok_or("Machine revision counter exhausted")?;
    }
    machine.validate()?;
    record["machine"] = serde_json::to_value(machine).map_err(|e| e.to_string())?;
    Ok(())
}
