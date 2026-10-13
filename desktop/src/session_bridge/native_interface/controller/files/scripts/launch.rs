//! Pre-run choices use the same mode/speed arguments as the live runner.
use super::super::super::chrome::{self, rect};
use super::*;

const SPEEDS: [f64; 7] = [0.25, 0.5, 1., 2., 4., 8., 16.];

#[derive(Clone, Copy)]
pub(super) struct Options {
    present: bool,
    speed_index: usize,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            present: true,
            speed_index: 3,
        }
    }
}
impl Options {
    pub fn mode(self) -> &'static str {
        if self.present {
            "present"
        } else {
            "fast"
        }
    }
    pub fn speed(self) -> f64 {
        SPEEDS[self.speed_index]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Mode,
    Slower,
    Faster,
}

pub(crate) fn command(world: &mut World, action: Action) -> Result<Value, String> {
    available(world)?;
    let state = &mut world.resource_mut::<Files>().script.launch;
    match action {
        Action::Mode => state.present = !state.present,
        Action::Slower => state.speed_index = state.speed_index.saturating_sub(1),
        Action::Faster => state.speed_index = (state.speed_index + 1).min(SPEEDS.len() - 1),
    }
    Ok(json!({"mode":state.mode(),"speed":state.speed()}))
}

pub(crate) fn paint(
    world: &mut World,
    camera: Entity,
    widgets: &mut chrome::Widgets,
    x: f32,
    y: f32,
    width: f32,
    busy: bool,
) -> Result<(), String> {
    let options = world.resource::<Files>().script.launch;
    let gap = 6.;
    let mode_width = width * 0.52;
    let speed_width = (width - mode_width - 2. * gap) * 0.5;
    for (key, label, caption, action, bounds, disabled) in [
        (
            "scripts-launch-mode",
            "Script run mode",
            format!("Mode: {}", if options.present { "Present" } else { "Fast" }),
            Action::Mode,
            rect(x, y, mode_width, 26.),
            busy,
        ),
        (
            "scripts-launch-slower",
            "Slower script presentation",
            format!("− {}×", options.speed()),
            Action::Slower,
            rect(x + mode_width + gap, y, speed_width, 26.),
            busy || !options.present || options.speed_index == 0,
        ),
        (
            "scripts-launch-faster",
            "Faster script presentation",
            "+".into(),
            Action::Faster,
            rect(x + mode_width + speed_width + 2. * gap, y, speed_width, 26.),
            busy || !options.present || options.speed_index + 1 == SPEEDS.len(),
        ),
    ] {
        let mut control = InterfaceControl::button("document/scripts", label);
        control.disabled = disabled;
        widgets.button(
            world,
            camera,
            key,
            control,
            Some(&caption),
            NativeCommand::File(FileCommand::ScriptLaunch(action)),
            bounds,
            None,
            61,
        )?;
    }
    Ok(())
}
