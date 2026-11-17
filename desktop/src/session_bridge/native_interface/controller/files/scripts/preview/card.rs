//! Native card tokens shared by the demo and its help view.
use super::super::super::super::chrome::{self, rect};
use super::*;

pub(super) fn navigation_height(world: &World) -> f32 {
    if world
        .resource::<Files>()
        .script
        .preview
        .history
        .peek()
        .is_some()
    {
        36.
    } else {
        0.
    }
}

pub(super) struct Card {
    pub x: f32,
    pub y: f32,
    pub width: f32,
}
impl Card {
    pub fn new(width: f32) -> Self {
        let w = (width - 32.).clamp(280., 360.);
        Self {
            x: (width - w - 16.).max(4.),
            y: 44.,
            width: w,
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub fn paint(
        &self,
        world: &mut World,
        camera: Entity,
        widgets: &mut chrome::Widgets,
        view: &str,
        height: f32,
        title: &str,
        theme: ViewportUiTheme,
    ) -> Result<(), String> {
        let (x, y, w) = (self.x, self.y, self.width);
        let key = |part: &str| format!("show-me-{view}-{part}");
        widgets.panel(
            world,
            camera,
            &key("card"),
            Node {
                border: UiRect::all(px(1.)),
                border_radius: BorderRadius::all(px(10.)),
                ..rect(x, y, w, height)
            },
            theme.panel.with_alpha(1.),
            60,
        );
        let entity = widgets.entity(&key("card")).unwrap();
        let shadow = BoxShadow::new(theme.shadow, px(0.), px(4.), px(0.), px(10.));
        if world.get::<BoxShadow>(entity) != Some(&shadow) {
            world.entity_mut(entity).insert(shadow);
        }
        let heading = title.to_owned();
        let close = lesson::text(world, "ui.close");
        widgets.text(
            world,
            camera,
            &key("heading"),
            rect(x + 12., y + 10., w - 52., 24.),
            &heading,
            14.,
            61,
        );
        widgets.button(
            world,
            camera,
            &key("close"),
            InterfaceControl::button("document/scripts", close),
            Some(""),
            NativeCommand::File(FileCommand::ScriptPreview(Action::Close)),
            rect(x + w - 36., y + 10., 24., 24.),
            Some(interface_shell::ribbon::Icon::Cancel),
            61,
        )?;
        if let Some(origin) = world.resource::<Files>().script.preview.history.peek() {
            let label = lesson::text(
                world,
                match origin {
                    navigation::Location::Index { .. } => "ui.backToGuides",
                    navigation::Location::Lesson(_) => "ui.back",
                },
            );
            let mut control = InterfaceControl::button("document/scripts", label);
            control.disabled = world.resource::<Files>().script.preview.building();
            widgets.button(
                world,
                camera,
                &key("back"),
                control,
                Some(&format!("‹  {label}")),
                NativeCommand::File(FileCommand::ScriptPreview(Action::Back)),
                rect(x + 12., y + 44., w - 24., 28.),
                None,
                61,
            )?;
        }
        Ok(())
    }
}
