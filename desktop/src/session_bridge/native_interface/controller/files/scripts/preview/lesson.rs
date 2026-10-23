//! Native presentation uses the shared, localized Help teaching catalog.
use super::*;
pub(super) use limo_cad_mcp::lessons::{CutScenario, Id, ALL, RECIPE};
pub(super) fn language(world: &World) -> &'static str {
    crate::app_preferences::locale::code(crate::native_viewport::localization::locale(world))
}
pub(super) fn text(world: &World, key: &str) -> &'static str {
    limo_cad_mcp::lessons::text(language(world), key)
}
pub(super) fn current(world: &World) -> Option<Id> {
    let preview = &world.resource::<Files>().script.preview;
    preview.lesson.or_else(|| {
        preview.retained.as_ref().and_then(|r| {
            let caption = r.descriptor.captions.get(preview.index)?;
            Id::from_key(caption.split_once('.')?.0)
        })
    })
}
