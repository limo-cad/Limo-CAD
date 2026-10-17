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
}
pub const ALL: [Id; 4] = [Id::Sketch, Id::Extrude, Id::Cut, Id::Fillet];

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
    pub const fn key(self) -> &'static str {
        match self {
            Self::Sketch => "sketch",
            Self::Extrude => "extrude",
            Self::Cut => "cut",
            Self::Fillet => "fillet",
        }
    }
    pub const fn help(self) -> &'static str {
        match self {
            Self::Sketch => "concepts.bracket-sketch",
            Self::Extrude => "concepts.extrude-basics",
            Self::Cut => "concepts.bracket-cut",
            Self::Fillet => "concepts.bracket-fillet",
        }
    }
    pub const fn prerequisite(self) -> Option<Id> {
        match self {
            Self::Sketch => None,
            Self::Extrude => Some(Self::Sketch),
            Self::Cut | Self::Fillet => Some(Self::Extrude),
        }
    }
    pub const fn cues(self) -> [&'static str; 4] {
        match self {
            Self::Sketch => ["plane", "profile", "dimension", "apply"],
            Self::Extrude => ["profile", "extrude", "depth", "apply"],
            Self::Cut => ["profile", "cut", "depth", "apply"],
            Self::Fillet => ["body", "edges", "radius", "apply"],
        }
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
        topics: vec![
            "sketch".into(),
            "extrude".into(),
            "cut".into(),
            "fillet".into(),
        ],
        keywords: vec!["bracket".into(), id.key().into()],
        description: id.summary(locale).into(),
        body,
        related_recipes: vec![RECIPE.into()],
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
    fn lessons_have_complete_locales_and_one_connected_build() {
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
                assert_eq!(page.related_recipes, [RECIPE]);
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
