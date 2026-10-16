//! Content for a feature demo; geometry and playback stay in the shared runner.
use crate::native_viewport::script_preview::PreviewCue;

pub(super) struct Demo {
    pub recipe: &'static str,
    pub title: &'static str,
    pub help: &'static str,
    pub steps: [(PreviewCue, &'static str); 4],
}
pub(super) const EXTRUDE: Demo = Demo {
    recipe: "extrude-basics",
    title: "Extrude",
    help: "concepts.extrude-basics",
    steps: [
        (PreviewCue::Profile, "Profile"),
        (PreviewCue::Extrude, "Extrude"),
        (PreviewCue::Depth, "Distance"),
        (PreviewCue::Apply, "Apply"),
    ],
};
