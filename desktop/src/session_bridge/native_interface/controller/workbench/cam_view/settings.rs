//! The existing desktop's workpiece simulation detail and comparison tolerance.
//! These are view preferences, never a second set of CAM document units.
use super::*;
use limo_cad_cam::CamUnits;
use limo_cad_interface::{ChoiceOption, ControlInput, Field, KeyChord};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Detail {
    #[default]
    Auto,
    Fine,
    Balanced,
    Fast,
}
impl Detail {
    fn key(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Fine => "fine",
            Self::Balanced => "balanced",
            Self::Fast => "fast",
        }
    }
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "auto" => Ok(Self::Auto),
            "fine" => Ok(Self::Fine),
            "balanced" => Ok(Self::Balanced),
            "fast" => Ok(Self::Fast),
            _ => Err("Choose Auto, Fine, Balanced or Fast simulation detail".into()),
        }
    }
    fn options() -> Vec<ChoiceOption> {
        [
            ("auto", "Auto"),
            ("fine", "Fine"),
            ("balanced", "Balanced"),
            ("fast", "Fast"),
        ]
        .into_iter()
        .map(|(value, label)| ChoiceOption {
            value: value.into(),
            label: label.into(),
            disabled: false,
        })
        .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Settings {
    pub detail: Detail,
    pub tolerance_mm: f64,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            detail: Detail::Auto,
            tolerance_mm: 0.1,
        }
    }
}
impl Settings {
    pub fn apply(self, setup: &CamSetupDto, request: &mut CamSimulationRequestDto) {
        let longest = (setup.stock.max.x - setup.stock.min.x)
            .max(setup.stock.max.y - setup.stock.min.y)
            .max(setup.stock.max.z - setup.stock.min.z);
        let (samples, budget) = match self.detail {
            Detail::Auto => (352., 8_000_000),
            Detail::Fine => (512., 8_000_000),
            Detail::Balanced => (256., 4_000_000),
            Detail::Fast => (128., 1_000_000),
        };
        request.voxel_size = Some(longest / samples);
        request.max_voxels = Some(budget);
        if let Some(target) = &mut request.target {
            target.tolerance_mm = self.tolerance_mm;
        }
    }

    fn edited(
        self,
        command: &Command,
        input: &ControlInput,
        units: CamUnits,
    ) -> Result<Self, String> {
        let mut next = self;
        match command {
            Command::Detail => {
                next.detail =
                    Detail::parse(&cam::choose(&Detail::options(), self.detail.key(), input)?)?
            }
            Command::Tolerance => {
                let ControlInput::SetValue(value) = input else {
                    return Err("Set the simulation preference's value".into());
                };
                let value: f64 = value
                    .trim()
                    .parse()
                    .map_err(|_| "Enter a numeric comparison tolerance")?;
                let mm = units.to_mm(value);
                if !mm.is_finite() || mm < 0. {
                    return Err("Comparison tolerance must be finite and nonnegative".into());
                }
                next.tolerance_mm = mm;
            }
            _ => return Err("Unknown simulation preference".into()),
        }
        Ok(next)
    }
}

pub(crate) fn reduce(
    world: &mut World,
    owner: &DocumentContext,
    revision: u64,
    command: &Command,
    input: &ControlInput,
) -> Result<Value, String> {
    if cam::editing_dirty(world) {
        return Err("Apply or cancel CAM edits before changing simulation settings".into());
    }
    let mut state = world
        .get_resource_mut::<State>()
        .ok_or("Open CAM simulation settings")?;
    let key = state.key.as_ref().ok_or("CAM view changed")?;
    if &key.owner != owner || key.revision != revision || !state.settings_open {
        return Err("CAM settings changed before the edit was applied".into());
    }
    let units = state.document.as_ref().ok_or("CAM document changed")?.units;
    let next = state.settings.edited(command, input, units)?;
    if next != state.settings {
        state.settings = next;
        if let Some(pending) = &state.pending {
            pending.cancellation.cancel();
        }
        state.generation = state.generation.wrapping_add(1);
        state.player = None;
        state.playback_action = None;
        state.seek_target = None;
        state.prepared = None;
        state.request_pending = state.setup.is_some();
        state.error.clear();
        state.dirty = true;
    }
    Ok(json!({"handled":true}))
}

pub(super) fn paint(
    world: &mut World,
    camera: Entity,
    state: &mut State,
    width: f32,
    side: f32,
) -> Result<(), String> {
    let units = state
        .document
        .as_ref()
        .map_or(CamUnits::Millimeters, |d| d.units);
    let w = (width - side - 28.).clamp(260., 330.);
    let x = (width - w - 14.).max(4.);
    let y = 132.;
    {
        let fill = crate::native_viewport::ui::theme(world)
            .panel
            .with_alpha(1.);
        super::super::card(
            (&mut state.widgets, world, camera),
            "cam-simulation-settings-card",
            rect(x, y, w, 232.),
            fill,
            6.,
            80,
        )
    };
    state.widgets.text(
        world,
        camera,
        "cam-simulation-settings-title",
        rect(x + 12., y + 10., w - 24., 24.),
        "Simulation settings",
        16.,
        82,
    );
    for (i, (label, command, field)) in [
        (
            "Simulation detail".to_string(),
            Command::Detail,
            Field::Choice {
                value: state.settings.detail.key().into(),
                options: Detail::options(),
            },
        ),
        (
            format!("Comparison tolerance ({})", units.length_label()),
            Command::Tolerance,
            Field::Text {
                value: units.from_mm(state.settings.tolerance_mm).to_string(),
                read_only: false,
                selection: None,
            },
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let fy = y + 44. + i as f32 * 58.;
        state.widgets.text(
            world,
            camera,
            &format!("cam-simulation-preference-label-{i}"),
            rect(x + 12., fy, w - 24., 17.),
            &label,
            11.,
            82,
        );
        let mut control = InterfaceControl::button("cam-simulation-settings", &label);
        control.modal_scope = Some("cam-simulation-settings".into());
        if matches!(field, Field::Choice { .. }) {
            control.role = "combobox".into();
            control.owned_keys = [
                "ArrowUp",
                "ArrowDown",
                "ArrowLeft",
                "ArrowRight",
                "Home",
                "End",
            ]
            .map(KeyChord::plain)
            .into();
        }
        let caption = match &field {
            Field::Choice { value, options } => options
                .iter()
                .find(|o| &o.value == value)
                .map(|o| o.label.clone()),
            _ => None,
        };
        control.field = field;
        state.widgets.button(
            world,
            camera,
            &format!("cam-simulation-preference-{i}"),
            control,
            caption.as_deref(),
            NativeCommand::Workbench(super::super::Command::CamView(command)),
            rect(x + 12., fy + 18., w - 24., 28.),
            None,
            82,
        )?;
    }
    let mut close =
        InterfaceControl::button("cam-simulation-settings", "Close simulation settings");
    close.modal_scope = Some("cam-simulation-settings".into());
    state.widgets.button(
        world,
        camera,
        "cam-simulation-settings-close",
        close,
        Some("Close"),
        NativeCommand::Workbench(super::super::Command::CamView(Command::CloseSettings)),
        rect(x + w - 92., y + 194., 80., 26.),
        None,
        82,
    )?;
    Ok(())
}
