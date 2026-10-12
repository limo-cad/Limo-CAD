//! Mouse gestures for the model/sketch viewport. UI controls and drawing paper
//! have separate input owners; keyboard shortcuts are not mouse bindings.
use super::{locale::translate, Locale};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Button {
    Disabled,
    Middle,
    Right,
    Left,
}
impl Button {
    pub(crate) const ALL: [Self; 4] = [Self::Disabled, Self::Middle, Self::Right, Self::Left];
    pub(crate) fn label(self, locale: Locale) -> &'static str {
        translate(
            locale,
            match self {
                Self::Disabled => "navigation.disabled",
                Self::Middle => "navigation.middle",
                Self::Right => "navigation.right",
                Self::Left => "navigation.leftReserved",
            },
        )
    }
}

/// Exact chords use the four normal modifier bits. The three broad predicates
/// retain the original gestures, including extra modifiers and AltGraph.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Modifiers {
    Any,
    NoShift,
    ShiftHeld,
    None,
    Shift,
    Ctrl,
    Alt,
    Meta,
    CtrlShift,
    AltShift,
    MetaShift,
    CtrlAlt,
    CtrlMeta,
    AltMeta,
    CtrlAltShift,
    CtrlMetaShift,
    AltMetaShift,
    CtrlAltMeta,
    CtrlAltMetaShift,
}

pub(crate) const SHIFT: u8 = 1;
pub(crate) const CTRL: u8 = 2;
pub(crate) const ALT: u8 = 4;
pub(crate) const META: u8 = 8;
pub(crate) const ALT_GRAPH: u8 = 16;

impl Modifiers {
    pub(crate) const ALL: [Self; 19] = [
        Self::Any,
        Self::NoShift,
        Self::ShiftHeld,
        Self::None,
        Self::Shift,
        Self::Ctrl,
        Self::Alt,
        Self::Meta,
        Self::CtrlShift,
        Self::AltShift,
        Self::MetaShift,
        Self::CtrlAlt,
        Self::CtrlMeta,
        Self::AltMeta,
        Self::CtrlAltShift,
        Self::CtrlMetaShift,
        Self::AltMetaShift,
        Self::CtrlAltMeta,
        Self::CtrlAltMetaShift,
    ];
    pub(crate) fn matches(self, bits: u8) -> bool {
        match self {
            Self::Any => true,
            Self::NoShift => bits & SHIFT == 0,
            Self::ShiftHeld => bits & SHIFT != 0,
            exact => exact.bits() == bits,
        }
    }
    fn bits(self) -> u8 {
        match self {
            Self::Any | Self::NoShift | Self::ShiftHeld | Self::None => 0,
            Self::Shift => SHIFT,
            Self::Ctrl => CTRL,
            Self::Alt => ALT,
            Self::Meta => META,
            Self::CtrlShift => CTRL | SHIFT,
            Self::AltShift => ALT | SHIFT,
            Self::MetaShift => META | SHIFT,
            Self::CtrlAlt => CTRL | ALT,
            Self::CtrlMeta => CTRL | META,
            Self::AltMeta => ALT | META,
            Self::CtrlAltShift => CTRL | ALT | SHIFT,
            Self::CtrlMetaShift => CTRL | META | SHIFT,
            Self::AltMetaShift => ALT | META | SHIFT,
            Self::CtrlAltMeta => CTRL | ALT | META,
            Self::CtrlAltMetaShift => CTRL | ALT | META | SHIFT,
        }
    }
    pub(crate) fn label(self, locale: Locale) -> String {
        match self {
            Self::Any => translate(locale, "navigation.anyModifiers").into(),
            Self::NoShift => translate(locale, "navigation.noShift").into(),
            Self::ShiftHeld => translate(locale, "navigation.shiftHeld").into(),
            Self::None => translate(locale, "navigation.noModifiers").into(),
            _ => modifier_label(self.bits()),
        }
    }
}

fn modifier_label(bits: u8) -> String {
    [
        (CTRL, "Ctrl"),
        (ALT, "Alt"),
        (META, "Cmd/Super"),
        (SHIFT, "Shift"),
    ]
    .into_iter()
    .filter_map(|(bit, label)| (bits & bit != 0).then_some(label))
    .collect::<Vec<_>>()
    .join("+")
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct DragBinding {
    pub button: Button,
    pub modifiers: Modifiers,
}
impl DragBinding {
    fn matches(self, button: Button, modifiers: u8) -> bool {
        self.button != Button::Disabled
            && self.button == button
            && self.modifiers.matches(modifiers)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Action {
    Inherit,
    Disabled,
    Pan,
    Orbit,
    Zoom,
}
impl Action {
    pub(crate) const ALL: [Self; 5] = [
        Self::Inherit,
        Self::Disabled,
        Self::Pan,
        Self::Orbit,
        Self::Zoom,
    ];
    pub(crate) fn label(self, locale: Locale) -> &'static str {
        translate(
            locale,
            match self {
                Self::Inherit => "navigation.inherit",
                Self::Disabled => "navigation.disabled",
                Self::Pan => "navigation.pan",
                Self::Orbit => "navigation.orbit",
                Self::Zoom => "navigation.zoom",
            },
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DragSlot {
    Pan,
    Orbit,
    OrbitAlternative,
    Zoom,
}
impl DragSlot {
    pub(crate) const ALL: [Self; 4] = [Self::Pan, Self::Orbit, Self::OrbitAlternative, Self::Zoom];
    pub(crate) fn label(self, locale: Locale) -> &'static str {
        translate(
            locale,
            match self {
                Self::Pan => "navigation.panDrag",
                Self::Orbit => "navigation.orbitDrag",
                Self::OrbitAlternative => "navigation.orbitAlternative",
                Self::Zoom => "navigation.zoomDrag",
            },
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScrollSlot {
    Wheel,
    Trackpad,
    Shift,
    Ctrl,
    Alt,
    Meta,
}
impl ScrollSlot {
    pub(crate) const ALL: [Self; 6] = [
        Self::Wheel,
        Self::Trackpad,
        Self::Shift,
        Self::Ctrl,
        Self::Alt,
        Self::Meta,
    ];
    pub(crate) fn label(self, locale: Locale) -> &'static str {
        translate(
            locale,
            match self {
                Self::Wheel => "navigation.wheel",
                Self::Trackpad => "navigation.trackpad",
                Self::Shift => "navigation.shiftScroll",
                Self::Ctrl => "navigation.ctrlScroll",
                Self::Alt => "navigation.altScroll",
                Self::Meta => "navigation.metaScroll",
            },
        )
    }
    pub(crate) fn is_base(self) -> bool {
        matches!(self, Self::Wheel | Self::Trackpad)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Bindings {
    pub pan: DragBinding,
    pub orbit: DragBinding,
    pub orbit_alternative: DragBinding,
    pub zoom: DragBinding,
    pub wheel: Action,
    pub trackpad: Action,
    pub shift_scroll: Action,
    pub ctrl_scroll: Action,
    pub alt_scroll: Action,
    pub meta_scroll: Action,
}
impl Default for Bindings {
    fn default() -> Self {
        Self {
            pan: DragBinding {
                button: Button::Middle,
                modifiers: Modifiers::NoShift,
            },
            orbit: DragBinding {
                button: Button::Middle,
                modifiers: Modifiers::ShiftHeld,
            },
            orbit_alternative: DragBinding {
                button: Button::Right,
                modifiers: Modifiers::Any,
            },
            zoom: DragBinding {
                button: Button::Disabled,
                modifiers: Modifiers::None,
            },
            wheel: Action::Zoom,
            trackpad: Action::Pan,
            shift_scroll: Action::Orbit,
            ctrl_scroll: Action::Zoom,
            alt_scroll: Action::Inherit,
            meta_scroll: Action::Inherit,
        }
    }
}
impl Bindings {
    pub(crate) fn drag_mut(&mut self, slot: DragSlot) -> &mut DragBinding {
        match slot {
            DragSlot::Pan => &mut self.pan,
            DragSlot::Orbit => &mut self.orbit,
            DragSlot::OrbitAlternative => &mut self.orbit_alternative,
            DragSlot::Zoom => &mut self.zoom,
        }
    }
    pub(crate) fn drag(mut self, slot: DragSlot) -> DragBinding {
        *self.drag_mut(slot)
    }
    pub(crate) fn scroll_mut(&mut self, slot: ScrollSlot) -> &mut Action {
        match slot {
            ScrollSlot::Wheel => &mut self.wheel,
            ScrollSlot::Trackpad => &mut self.trackpad,
            ScrollSlot::Shift => &mut self.shift_scroll,
            ScrollSlot::Ctrl => &mut self.ctrl_scroll,
            ScrollSlot::Alt => &mut self.alt_scroll,
            ScrollSlot::Meta => &mut self.meta_scroll,
        }
    }
    pub(crate) fn scroll(mut self, slot: ScrollSlot) -> Action {
        *self.scroll_mut(slot)
    }
    pub(crate) fn drag_action(self, button: Button, modifiers: u8) -> Option<Action> {
        DragSlot::ALL.into_iter().find_map(|slot| {
            self.drag(slot)
                .matches(button, modifiers)
                .then_some(match slot {
                    DragSlot::Pan => Action::Pan,
                    DragSlot::Orbit | DragSlot::OrbitAlternative => Action::Orbit,
                    DragSlot::Zoom => Action::Zoom,
                })
        })
    }
    /// Modifier scroll contexts are disjoint by precedence, preserving Shift
    /// over Ctrl from the original router. Inherit skips that modifier context.
    pub(crate) fn scroll_action(self, pixel: bool, modifiers: u8) -> Action {
        for (bit, action) in [
            (SHIFT, self.shift_scroll),
            (CTRL, self.ctrl_scroll),
            (ALT, self.alt_scroll),
            (META, self.meta_scroll),
        ] {
            if modifiers & bit != 0 && action != Action::Inherit {
                return action;
            }
        }
        if pixel {
            self.trackpad
        } else {
            self.wheel
        }
    }
    pub(crate) fn conflicts(self) -> Vec<Conflict> {
        let mut conflicts = Vec::new();
        for (index, first) in DragSlot::ALL.into_iter().enumerate() {
            let a = self.drag(first);
            if a.button == Button::Left {
                conflicts.push(Conflict::Selection(first));
            }
            if a.button == Button::Disabled {
                continue;
            }
            for second in DragSlot::ALL.into_iter().skip(index + 1) {
                let b = self.drag(second);
                if let Some(bits) =
                    (0..32).find(|bits| a.matches(b.button, *bits) && b.matches(a.button, *bits))
                {
                    conflicts.push(Conflict::Overlap(first, second, a.button, bits));
                }
            }
        }
        for slot in [ScrollSlot::Wheel, ScrollSlot::Trackpad] {
            if self.scroll(slot) == Action::Inherit {
                conflicts.push(Conflict::BaseScroll(slot));
            }
        }
        conflicts
    }
    pub(crate) fn validate(self) -> Result<(), String> {
        match self.conflicts().first() {
            Some(conflict) => Err(conflict.message(Locale::En)),
            None => Ok(()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Conflict {
    Selection(DragSlot),
    Overlap(DragSlot, DragSlot, Button, u8),
    BaseScroll(ScrollSlot),
}
impl Conflict {
    pub(crate) fn message(self, locale: Locale) -> String {
        match self {
            Self::Selection(slot) => translate(locale, "navigation.conflictSelection")
                .replace("{action}", slot.label(locale)),
            Self::Overlap(first, second, button, bits) => {
                let modifiers = modifier_label(bits);
                let gesture = if modifiers.is_empty() {
                    button.label(locale).to_owned()
                } else {
                    format!("{modifiers}+{}", button.label(locale))
                };
                translate(locale, "navigation.conflictOverlap")
                    .replace("{first}", first.label(locale))
                    .replace("{second}", second.label(locale))
                    .replace("{gesture}", &gesture)
            }
            Self::BaseScroll(slot) => translate(locale, "navigation.conflictBaseScroll")
                .replace("{action}", slot.label(locale)),
        }
    }
}

#[cfg(test)]
mod tests;
