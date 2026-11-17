//! Localized navigation over the actual retained frames, including Cut variants.
use super::super::super::super::chrome::{self, rect};
use super::*;

fn visible(index: usize, count: usize) -> std::ops::Range<usize> {
    let start = index.saturating_sub(1).min(count.saturating_sub(3));
    start..(start + 3).min(count)
}

const ROW_HEIGHT: f32 = 32.;

fn page_range(
    page: Option<usize>,
    current: usize,
    count: usize,
    height: f32,
) -> (usize, std::ops::Range<usize>) {
    let capacity = ((height - 36.).max(ROW_HEIGHT) / ROW_HEIGHT).floor() as usize;
    let page = page
        .unwrap_or(current / capacity)
        .min(count.saturating_sub(1) / capacity);
    let start = page * capacity;
    (page, start..(start + capacity).min(count))
}

fn label(language: &str, caption: &str) -> String {
    let label = limo_cad_mcp::lessons::text(language, &format!("step.{caption}"));
    if label.is_empty() {
        limo_cad_mcp::lessons::caption(language, caption)
    } else {
        label.into()
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn paint(
    world: &mut World,
    camera: Entity,
    widgets: &mut chrome::Widgets,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    expanded: bool,
) -> Result<(), String> {
    let language = lesson::language(world);
    let state = &world.resource::<Files>().script.preview;
    let Some(retained) = &state.retained else {
        return Ok(());
    };
    let count = state.count();
    let current = state.display_index();
    let (page, range) = if expanded {
        page_range(state.steps_page, current, count, height)
    } else {
        (0, visible(current, count))
    };
    let pages = expanded && (range.start > 0 || range.end < count);
    let previous_page = range.start > 0;
    let next_page = range.end < count;
    let page_label = format!("{}–{} / {}", range.start + 1, range.end, count);
    let frames: Vec<_> = range
        .map(|index| {
            let actual = if index == current && state.image.is_some() {
                state.display_frame_index()
            } else if state.cut_choices() {
                state.scenario.frame(index)
            } else {
                index
            };
            let caption = &retained.descriptor.captions[actual];
            (
                index,
                if expanded {
                    limo_cad_mcp::lessons::text(language, &format!("action.{caption}")).into()
                } else {
                    label(language, caption)
                },
                limo_cad_mcp::lessons::caption(language, caption),
            )
        })
        .collect();
    let cell = if expanded { width } else { (width - 24.) / 3. };
    let row = if expanded { ROW_HEIGHT } else { 28. };
    if expanded {
        world.resource_mut::<Files>().script.preview.steps_page = Some(page);
    }
    for (position, (index, title, caption)) in frames.into_iter().enumerate() {
        let left = x + if expanded {
            0.
        } else {
            position as f32 * (cell + 12.)
        };
        let top = y + if expanded { position as f32 * row } else { 0. };
        let text = if expanded {
            format!("{}. {title}", index + 1)
        } else {
            title
        };
        let mut control =
            InterfaceControl::button("document/scripts", format!("{text}: {caption}"));
        control.selected = Some(index == current);
        let entity = widgets.button(
            world,
            camera,
            &format!("lesson-step-{index}"),
            control,
            Some(&text),
            NativeCommand::File(FileCommand::ScriptPreview(Action::Step(index))),
            rect(left, top, cell, row),
            None,
            62,
        )?;
        if expanded && index == current {
            feature::focus_control(world, entity);
        }
        if !expanded && position > 0 {
            widgets.text(
                world,
                camera,
                &format!("lesson-step-separator-{position}"),
                rect(left - 12., top + 4., 12., 20.),
                "›",
                12.,
                62,
            );
        }
    }
    if pages {
        for (next, enabled, left, title) in [
            (false, previous_page, x, "ui.previous"),
            (true, next_page, x + width - 96., "ui.next"),
        ] {
            let title = limo_cad_mcp::lessons::text(language, title);
            let mut control = InterfaceControl::button("document/scripts", title);
            control.disabled = !enabled;
            widgets.button(
                world,
                camera,
                if next {
                    "lesson-steps-page-next"
                } else {
                    "lesson-steps-page-previous"
                },
                control,
                Some(title),
                NativeCommand::File(FileCommand::ScriptPreview(Action::StepsPage(next))),
                Node {
                    border: UiRect::all(px(1.)),
                    ..rect(left, y + height - ROW_HEIGHT, 96., ROW_HEIGHT)
                },
                None,
                62,
            )?;
        }
        widgets.text(
            world,
            camera,
            "lesson-steps-page-count",
            rect(x + 100., y + height - ROW_HEIGHT, width - 200., ROW_HEIGHT),
            &page_label,
            12.,
            62,
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_script_preview_step_pages_preserve_readable_rows_and_reach_every_step() {
        assert_eq!(page_range(None, 4, 5, 196.), (0, 0..5));
        assert_eq!(page_range(None, 15, 16, 196.), (3, 15..16));
        assert_eq!(page_range(Some(99), 0, 16, 196.), (3, 15..16));
        let visited: Vec<_> = (0..4)
            .flat_map(|p| page_range(Some(p), 0, 16, 196.).1)
            .collect();
        assert_eq!(visited, (0..16).collect::<Vec<_>>());
        assert_eq!(page_range(None, 4, 5, 64.), (4, 4..5));
    }
    #[test]
    fn native_script_preview_step_window_keeps_neighbors_and_endpoints() {
        assert_eq!(visible(0, 5), 0..3);
        assert_eq!(visible(2, 5), 1..4);
        assert_eq!(visible(4, 5), 2..5);
        assert_eq!(visible(0, 1), 0..1);
        assert_eq!(visible(0, 0), 0..0);
        for locale in limo_cad_mcp::lessons::LOCALES {
            for id in lesson::ALL {
                for caption in id.captions() {
                    assert_eq!(
                        label(locale, caption),
                        limo_cad_mcp::lessons::text(locale, &format!("step.{caption}"))
                    );
                    assert!(
                        !limo_cad_mcp::lessons::text(locale, &format!("action.{caption}"))
                            .is_empty()
                    );
                }
            }
            assert_eq!(label(locale, "unknown.caption"), "unknown.caption");
            for key in [
                "sketch.outline",
                "sketch.close",
                "sketch.finish",
                "cut.through",
                "cut.measured.depth",
            ] {
                assert!(!label(locale, key).is_empty());
            }
        }
    }

    #[test]
    fn native_script_preview_sketch_step_labels_are_operation_specific() {
        for locale in limo_cad_mcp::lessons::LOCALES {
            for (left, right) in [
                (lesson::Id::SketchLine, lesson::Id::SketchMidpointLine),
                (lesson::Id::SketchFillet, lesson::Id::SketchChamfer),
                (lesson::Id::SketchCoincident, lesson::Id::SketchConcentric),
            ] {
                let left = left
                    .captions()
                    .iter()
                    .map(|caption| label(locale, caption))
                    .collect::<Vec<_>>();
                let right = right
                    .captions()
                    .iter()
                    .map(|caption| label(locale, caption))
                    .collect::<Vec<_>>();
                assert_ne!(
                    left, right,
                    "{locale}: operation steps must remain distinct"
                );
            }
        }
    }
}
