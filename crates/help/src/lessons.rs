pub mod coverage;

//! One localized teaching catalog for native hints, demonstrations and Help.
use crate::Page;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::OnceLock;

pub const LOCALES: [&str; 4] = ["en", "zh-CN", "es", "de"];
pub const RECIPE: &str = "extrude-basics";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Id {
    Sketch,
    Extrude,
    Cut,
    Fillet,
    Revolve,
    Sweep,
    Loft,
    Rib,
    Hole,
    ExternalThread,
    Chamfer,
    Shell,
    Mirror,
    RectangularPattern,
    CircularPattern,
    MoveCopy,
    Combine,
    SplitBody,
    OffsetPlane,
    Midplane,
    PlaneAtAngle,
}
pub const ALL: [Id; 21] = [
    Id::Sketch,
    Id::Extrude,
    Id::Cut,
    Id::Fillet,
    Id::Revolve,
    Id::Sweep,
    Id::Loft,
    Id::Rib,
    Id::Hole,
    Id::ExternalThread,
    Id::Chamfer,
    Id::Shell,
    Id::Mirror,
    Id::RectangularPattern,
    Id::CircularPattern,
    Id::MoveCopy,
    Id::Combine,
    Id::SplitBody,
    Id::OffsetPlane,
    Id::Midplane,
    Id::PlaneAtAngle,
];

/// A small choice of outcomes within Cut, independent of lesson navigation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CutScenario {
    #[default]
    ThroughAll,
    MeasuredDepth,
}
impl CutScenario {
    pub const ALL: [Self; 2] = [Self::ThroughAll, Self::MeasuredDepth];
    pub const fn key(self) -> &'static str {
        match self {
            Self::ThroughAll => "through_all",
            Self::MeasuredDepth => "measured_depth",
        }
    }
    pub fn label(self, locale: &str) -> &'static str {
        text(locale, &format!("cut.scenario.{}", self.key()))
    }
    pub const fn frame(self, index: usize) -> usize {
        match (self, index) {
            (Self::MeasuredDepth, 2..=3) => index + 2,
            _ => index,
        }
    }
}
impl Id {
    /// The editable build that authored this guide's immutable geometry.
    pub const fn recipe(self) -> &'static str {
        match self {
            Self::Sketch => "extrude-basics",
            Self::Extrude => "extrude-basics",
            Self::Cut => "extrude-basics",
            Self::Fillet => "extrude-basics",
            Self::Revolve => "show-me-revolve",
            Self::Sweep => "show-me-sweep",
            Self::Loft => "show-me-loft",
            Self::Rib => "show-me-rib",
            Self::Hole => "show-me-hole",
            Self::ExternalThread => "show-me-external-thread",
            Self::Chamfer => "show-me-chamfer",
            Self::Shell => "show-me-shell",
            Self::Mirror => "show-me-mirror",
            Self::RectangularPattern => "show-me-rectangular-pattern",
            Self::CircularPattern => "show-me-circular-pattern",
            Self::MoveCopy => "show-me-move-copy",
            Self::Combine => "show-me-combine",
            Self::SplitBody => "show-me-split-body",
            Self::OffsetPlane => "show-me-offset-plane",
            Self::Midplane => "show-me-midplane",
            Self::PlaneAtAngle => "show-me-plane-at-angle",
        }
    }
    pub const fn frame_count(self) -> usize {
        match self {
            Self::Sketch => 5,
            Self::Extrude => 5,
            Self::Cut => 6,
            Self::Fillet => 4,
            Self::Revolve => 4,
            Self::Sweep => 4,
            Self::Loft => 4,
            Self::Rib => 4,
            Self::Hole => 4,
            Self::ExternalThread => 4,
            Self::Chamfer => 4,
            Self::Shell => 4,
            Self::Mirror => 4,
            Self::RectangularPattern => 4,
            Self::CircularPattern => 4,
            Self::MoveCopy => 4,
            Self::Combine => 4,
            Self::SplitBody => 4,
            Self::OffsetPlane => 4,
            Self::Midplane => 4,
            Self::PlaneAtAngle => 4,
        }
    }
    pub const fn key(self) -> &'static str {
        match self {
            Self::Sketch => "sketch",
            Self::Extrude => "extrude",
            Self::Cut => "cut",
            Self::Fillet => "fillet",
            Self::Revolve => "revolve",
            Self::Sweep => "sweep",
            Self::Loft => "loft",
            Self::Rib => "rib",
            Self::Hole => "hole",
            Self::ExternalThread => "external_thread",
            Self::Chamfer => "chamfer",
            Self::Shell => "shell",
            Self::Mirror => "mirror",
            Self::RectangularPattern => "rectangular_pattern",
            Self::CircularPattern => "circular_pattern",
            Self::MoveCopy => "move_copy",
            Self::Combine => "combine",
            Self::SplitBody => "split_body",
            Self::OffsetPlane => "offset_plane",
            Self::Midplane => "midplane",
            Self::PlaneAtAngle => "plane_at_angle",
        }
    }
    pub const fn help(self) -> &'static str {
        match self {
            Self::Sketch => "concepts.bracket-sketch",
            Self::Extrude => "concepts.extrude-basics",
            Self::Cut => "concepts.bracket-cut",
            Self::Fillet => "concepts.bracket-fillet",
            Self::Revolve => "concepts.show-me-revolve",
            Self::Sweep => "concepts.show-me-sweep",
            Self::Loft => "concepts.show-me-loft",
            Self::Rib => "concepts.show-me-rib",
            Self::Hole => "concepts.show-me-hole",
            Self::ExternalThread => "concepts.show-me-external-thread",
            Self::Chamfer => "concepts.show-me-chamfer",
            Self::Shell => "concepts.show-me-shell",
            Self::Mirror => "concepts.show-me-mirror",
            Self::RectangularPattern => "concepts.show-me-rectangular-pattern",
            Self::CircularPattern => "concepts.show-me-circular-pattern",
            Self::MoveCopy => "concepts.show-me-move-copy",
            Self::Combine => "concepts.show-me-combine",
            Self::SplitBody => "concepts.show-me-split-body",
            Self::OffsetPlane => "concepts.show-me-offset-plane",
            Self::Midplane => "concepts.show-me-midplane",
            Self::PlaneAtAngle => "concepts.show-me-plane-at-angle",
        }
    }
    pub const fn prerequisite(self) -> Option<Id> {
        match self {
            Self::Sketch => None,
            Self::Extrude => Some(Self::Sketch),
            Self::Cut => Some(Self::Extrude),
            Self::Fillet => Some(Self::Extrude),
            Self::Revolve => Some(Self::Sketch),
            Self::Sweep => Some(Self::Sketch),
            Self::Loft => Some(Self::Sketch),
            Self::Rib => Some(Self::Extrude),
            Self::Hole => Some(Self::Extrude),
            Self::ExternalThread => Some(Self::Extrude),
            Self::Chamfer => Some(Self::Extrude),
            Self::Shell => Some(Self::Extrude),
            Self::Mirror => Some(Self::Extrude),
            Self::RectangularPattern => Some(Self::Extrude),
            Self::CircularPattern => Some(Self::Extrude),
            Self::MoveCopy => Some(Self::Extrude),
            Self::Combine => Some(Self::Extrude),
            Self::SplitBody => Some(Self::Extrude),
            Self::OffsetPlane => Some(Self::Extrude),
            Self::Midplane => Some(Self::Extrude),
            Self::PlaneAtAngle => Some(Self::Extrude),
        }
    }
    pub const fn cues(self) -> [&'static str; 4] {
        match self {
            Self::Sketch => ["plane", "profile", "dimension", "apply"],
            Self::Extrude => ["profile", "extrude", "depth", "apply"],
            Self::Cut => ["profile", "cut", "depth", "apply"],
            Self::Fillet => ["body", "edges", "radius", "apply"],
            Self::Revolve => ["profile", "dimension", "dimension", "apply"],
            Self::Sweep => ["profile", "profile", "dimension", "apply"],
            Self::Loft => ["profile", "profile", "dimension", "apply"],
            Self::Rib => ["profile", "dimension", "dimension", "apply"],
            Self::Hole => ["body", "depth", "dimension", "apply"],
            Self::ExternalThread => ["body", "depth", "depth", "apply"],
            Self::Chamfer => ["body", "edges", "dimension", "apply"],
            Self::Shell => ["body", "depth", "dimension", "apply"],
            Self::Mirror => ["body", "plane", "dimension", "apply"],
            Self::RectangularPattern => ["body", "dimension", "dimension", "apply"],
            Self::CircularPattern => ["body", "dimension", "dimension", "apply"],
            Self::MoveCopy => ["body", "dimension", "dimension", "apply"],
            Self::Combine => ["body", "profile", "dimension", "apply"],
            Self::SplitBody => ["body", "plane", "dimension", "apply"],
            Self::OffsetPlane => ["body", "plane", "depth", "apply"],
            Self::Midplane => ["body", "plane", "dimension", "apply"],
            Self::PlaneAtAngle => ["edges", "plane", "dimension", "apply"],
        }
    }
    /// Exact authored captions, including both retained Cut scenarios.
    pub const fn captions(self) -> &'static [&'static str] {
        match self {
            Self::Sketch => &[
                "sketch.plane",
                "sketch.outline",
                "sketch.close",
                "sketch.dimension",
                "sketch.finish",
            ],
            Self::Extrude => &[
                "extrude.profile",
                "extrude.start",
                "extrude.distance",
                "extrude.width",
                "extrude.apply",
            ],
            Self::Cut => &[
                "cut.profile",
                "cut.operation",
                "cut.through",
                "cut.apply",
                "cut.measured.depth",
                "cut.measured.apply",
            ],
            Self::Fillet => &[
                "fillet.body",
                "fillet.edges",
                "fillet.radius",
                "fillet.apply",
            ],
            Self::Revolve => &[
                "revolve.inputs",
                "revolve.preview",
                "revolve.parameter",
                "revolve.apply",
            ],
            Self::Sweep => &[
                "sweep.inputs",
                "sweep.preview",
                "sweep.parameter",
                "sweep.apply",
            ],
            Self::Loft => &[
                "loft.inputs",
                "loft.preview",
                "loft.parameter",
                "loft.apply",
            ],
            Self::Rib => &["rib.inputs", "rib.preview", "rib.parameter", "rib.apply"],
            Self::Hole => &[
                "hole.inputs",
                "hole.preview",
                "hole.parameter",
                "hole.apply",
            ],
            Self::ExternalThread => &[
                "external_thread.inputs",
                "external_thread.preview",
                "external_thread.parameter",
                "external_thread.apply",
            ],
            Self::Chamfer => &[
                "chamfer.inputs",
                "chamfer.preview",
                "chamfer.parameter",
                "chamfer.apply",
            ],
            Self::Shell => &[
                "shell.inputs",
                "shell.preview",
                "shell.parameter",
                "shell.apply",
            ],
            Self::Mirror => &[
                "mirror.inputs",
                "mirror.preview",
                "mirror.parameter",
                "mirror.apply",
            ],
            Self::RectangularPattern => &[
                "rectangular_pattern.inputs",
                "rectangular_pattern.preview",
                "rectangular_pattern.parameter",
                "rectangular_pattern.apply",
            ],
            Self::CircularPattern => &[
                "circular_pattern.inputs",
                "circular_pattern.preview",
                "circular_pattern.parameter",
                "circular_pattern.apply",
            ],
            Self::MoveCopy => &[
                "move_copy.inputs",
                "move_copy.preview",
                "move_copy.parameter",
                "move_copy.apply",
            ],
            Self::Combine => &[
                "combine.inputs",
                "combine.preview",
                "combine.parameter",
                "combine.apply",
            ],
            Self::SplitBody => &[
                "split_body.inputs",
                "split_body.preview",
                "split_body.parameter",
                "split_body.apply",
            ],
            Self::OffsetPlane => &[
                "offset_plane.inputs",
                "offset_plane.preview",
                "offset_plane.parameter",
                "offset_plane.apply",
            ],
            Self::Midplane => &[
                "midplane.inputs",
                "midplane.preview",
                "midplane.parameter",
                "midplane.apply",
            ],
            Self::PlaneAtAngle => &[
                "plane_at_angle.inputs",
                "plane_at_angle.preview",
                "plane_at_angle.parameter",
                "plane_at_angle.apply",
            ],
        }
    }
    /// Stable extension point for additional operation and Sketch guides.
    pub fn from_key(key: &str) -> Option<Self> {
        Some(match key {
            "sketch" => Self::Sketch,
            "extrude" => Self::Extrude,
            "cut" => Self::Cut,
            "fillet" => Self::Fillet,
            "revolve" => Self::Revolve,
            "sweep" => Self::Sweep,
            "loft" => Self::Loft,
            "rib" => Self::Rib,
            "hole" => Self::Hole,
            "external_thread" => Self::ExternalThread,
            "chamfer" => Self::Chamfer,
            "shell" => Self::Shell,
            "mirror" => Self::Mirror,
            "rectangular_pattern" => Self::RectangularPattern,
            "circular_pattern" => Self::CircularPattern,
            "move_copy" => Self::MoveCopy,
            "combine" => Self::Combine,
            "split_body" => Self::SplitBody,
            "offset_plane" => Self::OffsetPlane,
            "midplane" => Self::Midplane,
            "plane_at_angle" => Self::PlaneAtAngle,
            _ => return None,
        })
    }
    pub fn title(self, locale: &str) -> &'static str {
        text(locale, &format!("{}.title", self.key()))
    }
    pub fn summary(self, locale: &str) -> &'static str {
        text(locale, &format!("{}.summary", self.key()))
    }
}
fn catalog() -> &'static Value {
    static DATA: OnceLock<Value> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(include_str!("lessons.json")).expect("embedded teaching catalog")
    })
}
pub fn text(locale: &str, key: &str) -> &'static str {
    catalog()[locale][key]
        .as_str()
        .or_else(|| catalog()["en"][key].as_str())
        .unwrap_or("")
}
pub fn caption(locale: &str, key: &str) -> String {
    let value = text(locale, key);
    if value.is_empty() {
        key.to_owned()
    } else {
        value.to_owned()
    }
}
pub fn page(locale: &str, id: Id) -> Page {
    let key = format!("{}.instructions", id.key());
    let values = catalog()[locale][&key]
        .as_array()
        .unwrap_or_else(|| catalog()["en"][&key].as_array().unwrap());
    let body = values
        .iter()
        .enumerate()
        .map(|(index, value)| format!("{}. {}", index + 1, value.as_str().unwrap()))
        .collect::<Vec<_>>()
        .join("\n\n");
    Page {
        id: id.help().into(),
        title: id.title(locale).into(),
        topics: if matches!(id, Id::Sketch | Id::Extrude | Id::Cut | Id::Fillet) {
            vec![
                "sketch".into(),
                "extrude".into(),
                "cut".into(),
                "fillet".into(),
            ]
        } else {
            vec!["show-me".into(), id.key().into()]
        },
        keywords: if matches!(id, Id::Sketch | Id::Extrude | Id::Cut | Id::Fillet) {
            vec!["bracket".into(), id.key().into()]
        } else {
            vec![id.key().replace('_', " "), id.recipe().into()]
        },
        description: id.summary(locale).into(),
        body,
        related_recipes: vec![id.recipe().into()],
        status: "stable".into(),
    }
}
pub fn from_help(id: &str) -> Option<Id> {
    ALL.into_iter().find(|lesson| lesson.help() == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_lesson_has_a_unique_round_trip_identity_and_source_contract() {
        let mut keys = std::collections::HashSet::new();
        let mut pages = std::collections::HashSet::new();
        for id in ALL {
            assert!(keys.insert(id.key()));
            assert!(pages.insert(id.help()));
            assert_eq!(Id::from_key(id.key()), Some(id));
            assert_eq!(from_help(id.help()), Some(id));
            assert_eq!(
                serde_json::from_value::<Id>(serde_json::json!(id.key())).unwrap(),
                id
            );
            assert_eq!(id.captions().len(), id.frame_count());
            assert!(id.captions().iter().all(|caption| caption
                .split_once('.')
                .is_some_and(|(key, _)| key == id.key())));
            if let Some(prerequisite) = id.prerequisite() {
                assert_ne!(prerequisite, id);
            }
        }
        assert!(Id::from_key("unknown-operation").is_none());
    }

    #[test]
    fn all_guides_share_localized_help_and_search_identity() {
        let store = crate::HelpStore::bundled();
        for id in ALL {
            let hits = store.search(id.title("en"), Some(10));
            assert!(
                hits.iter().any(|hit| hit.id == id.help()),
                "{} must be searchable",
                id.key()
            );
            for locale in LOCALES {
                let result = store.get_localized(id.help(), locale).unwrap();
                let authored = page(locale, id);
                assert_eq!(result.body, authored.body);
                assert_eq!(result.title, authored.title);
                assert_eq!(result.description, authored.description);
                assert_eq!(result.related_recipes, [id.recipe()]);
                assert_eq!(result.demonstrations, [id]);
                assert!(!result.truncated);
            }
            assert_eq!(page("unknown-locale", id), page("en", id));
        }
    }

    #[test]
    fn step_chooser_labels_preserve_translated_unicode() {
        for locale in LOCALES {
            let dictionary = catalog()[locale].as_object().unwrap();
            for (key, value) in dictionary {
                if key.starts_with("action.") || key == "ui.chooseStep" {
                    let value = value.as_str().unwrap();
                    assert!(
                        !value.is_empty() && !value.contains(['?', '\u{fffd}']),
                        "Corrupted chooser label {locale}/{key}: {value}"
                    );
                }
            }
        }
        assert_eq!(text("zh-CN", "ui.chooseStep"), "选择步骤");
        assert_eq!(text("de", "ui.chooseStep"), "Schritt wählen");
        assert_eq!(text("es", "action.extrude.apply"), "Aplicar la extrusión");
    }

    #[test]
    fn guided_examples_have_explicit_localized_identity_and_playback_labels() {
        for locale in LOCALES {
            for key in [
                "ui.guidedExamples",
                "ui.instructions",
                "ui.hideInstructions",
                "ui.resume",
                "sketch.example",
                "extrude.example",
                "cut.example",
                "fillet.example",
            ] {
                let translated = catalog()[locale][key]
                    .as_str()
                    .expect("Explicit locale required");
                assert!(
                    !translated.is_empty()
                        && !translated.contains('?')
                        && !translated.contains('\u{fffd}'),
                    "{locale}: {key}"
                );
            }
            for lesson in ALL {
                for suffix in ["example", "outcome"] {
                    let key = format!("{}.{suffix}", lesson.key());
                    assert!(
                        catalog()[locale][&key]
                            .as_str()
                            .is_some_and(|text| !text.is_empty() && !text.contains('\u{fffd}')),
                        "{locale}/{key}"
                    );
                }
            }
            assert_ne!(text(locale, "extrude.example"), text(locale, "cut.example"));
            assert_ne!(text(locale, "extrude.example"), Id::Extrude.title(locale));
            assert_ne!(text(locale, "cut.example"), Id::Cut.title(locale));
        }
    }

    #[test]
    fn lessons_have_complete_locales_and_matching_authored_builds() {
        let english = catalog()["en"].as_object().unwrap();
        for locale in LOCALES {
            let dictionary = catalog()[locale].as_object().unwrap();
            assert_eq!(
                dictionary.keys().collect::<Vec<_>>(),
                english.keys().collect::<Vec<_>>(),
                "{locale}"
            );
            for (key, value) in dictionary {
                assert!(
                    value.as_str().is_some_and(|s| !s.is_empty())
                        || value.as_array().is_some_and(|a| !a.is_empty()
                            && a.iter().all(|s| s.as_str().is_some_and(|s| !s.is_empty()))),
                    "{locale}/{key}"
                );
            }
            for lesson in ALL {
                let page = page(locale, lesson);
                assert!(!page.body.contains("limo-cad-help"));
                assert_eq!(page.related_recipes, [lesson.recipe()]);
                for caption in lesson.captions() {
                    for key in [
                        caption.to_string(),
                        format!("step.{caption}"),
                        format!("action.{caption}"),
                    ] {
                        assert!(
                            dictionary[&key]
                                .as_str()
                                .is_some_and(|text| !text.is_empty()),
                            "{locale}/{key}"
                        );
                    }
                }
                for cue in lesson.cues() {
                    assert!(!text(locale, &format!("step.{cue}")).is_empty());
                }
            }
        }
        assert_eq!(Id::Cut.prerequisite(), Some(Id::Extrude));
        assert_eq!(Id::Extrude.prerequisite(), Some(Id::Sketch));
        for id in ALL {
            let file = crate::knowledge_files()
                .iter()
                .find(|file| {
                    crate::parse_markdown(file.path, file.text)
                        .is_some_and(|page| page.id == id.help())
                })
                .unwrap();
            let embedded = crate::parse_markdown(file.path, file.text).unwrap();
            let authored = page("en", id);
            assert_eq!(
                embedded.body, authored.body,
                "English knowledge must be generated from the lesson catalog"
            );
            assert_eq!(embedded.title, authored.title);
            assert_eq!(embedded.description, authored.description);
        }
    }
}
