//! Per-host UI locale over the shared product dictionaries.
//!
//! The preference controller publishes the effective locale here. Rendering
//! never reads preferences from disk, and no process-global locale can leak
//! between independently owned windows or test worlds.

use bevy::prelude::{Resource, World};

use crate::app_preferences::{locale as dictionary, Locale};

#[derive(Resource, Default)]
pub(crate) struct NativeLocale(Locale);

pub(crate) fn locale(world: &World) -> Locale {
    world
        .get_resource::<NativeLocale>()
        .map_or(Locale::En, |locale| locale.0)
}

/// Returns whether visible translations changed. The caller owns redraw and
/// preference persistence; updating presentation never changes a document.
pub(crate) fn set_locale(world: &mut World, value: Locale) -> bool {
    let changed = locale(world) != value;
    if changed || !world.contains_resource::<NativeLocale>() {
        world.insert_resource(NativeLocale(value));
    }
    changed
}

pub(crate) fn translate<'a>(world: &World, key: &'a str) -> &'a str {
    dictionary::translate(locale(world), key)
}

pub(crate) fn locale_of(value: Option<&NativeLocale>) -> Locale {
    value.map(|locale| locale.0).unwrap_or(Locale::En)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_changes_are_local_to_each_host_and_report_only_visible_changes() {
        let mut first = World::new();
        let second = World::new();
        assert_eq!(translate(&first, "topbar.settings"), "Settings");
        assert!(!set_locale(&mut first, Locale::En));
        assert!(set_locale(&mut first, Locale::ZhCn));
        assert_eq!(translate(&first, "topbar.settings"), "设置");
        assert_eq!(translate(&second, "topbar.settings"), "Settings");
        assert!(!set_locale(&mut first, Locale::ZhCn));
        assert!(set_locale(&mut first, Locale::De));
        assert_eq!(translate(&first, "topbar.settings"), "Einstellungen");
        assert_eq!(
            translate(&first, "untranslated.native.key"),
            "untranslated.native.key"
        );
        assert!(set_locale(&mut first, Locale::En));
        assert_eq!(translate(&first, "topbar.settings"), "Settings");
        assert_eq!(translate(&first, "file.keepWorking"), "Keep working");
        assert_eq!(
            translate(&first, "ribbon.drawing.sheetStatus"),
            "{name} · {count} views"
        );
        assert_eq!(
            dictionary::translate(Locale::ZhCn, "file.keepWorking"),
            "继续工作"
        );
        assert_eq!(
            dictionary::translate(Locale::ZhCn, "ribbon.drawing.bottom"),
            "仰视图"
        );
        assert_eq!(
            dictionary::translate(Locale::Es, "ribbon.drawing.bottom"),
            "Vista inferior"
        );
        assert_eq!(
            dictionary::translate(Locale::Es, "file.closeMenu"),
            "Cerrar menú Archivo"
        );
        assert_eq!(
            dictionary::translate(Locale::De, "workspace.unavailable"),
            "Nicht verfügbar"
        );
        assert_eq!(
            dictionary::translate(Locale::De, "drawing.workspace.reassociateReferences"),
            "Referenzen neu verknüpfen"
        );
        assert_eq!(locale_of(None), Locale::En);
        assert_eq!(locale_of(first.get_resource::<NativeLocale>()), Locale::En);
    }
}
