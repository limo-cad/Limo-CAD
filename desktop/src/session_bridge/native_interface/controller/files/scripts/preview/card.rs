//! Native card tokens shared by the demo and its help view.
use super::super::super::super::chrome::{self, rect};
use super::*;

pub(super) struct Card {
    pub x: f32,
    pub y: f32,
    pub width: f32,
}
impl Card {
    pub fn new(width: f32) -> Self {
        let w = (width - 40.).clamp(280., 392.);
        Self {
            x: (width - w - 20.).max(4.),
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
                border_radius: BorderRadius::all(px(8.)),
                ..rect(x, y, w, height)
            },
            theme.panel.with_alpha(1.),
            60,
        );
        let entity = widgets.entity(&key("card")).unwrap();
        let shadow = BoxShadow::new(theme.shadow, px(0.), px(6.), px(0.), px(16.));
        if world.get::<BoxShadow>(entity) != Some(&shadow) {
            world.entity_mut(entity).insert(shadow);
        }
        widgets.colored_text(
            world,
            camera,
            &key("kind"),
            rect(x + 16., y + 12., w - 68., 16.),
            "Show me",
            11.,
            61,
            theme.mute,
        );
        widgets.text(
            world,
            camera,
            &key("heading"),
            rect(x + 16., y + 30., w - 68., 28.),
            title,
            20.,
            61,
        );
        widgets.button(
            world,
            camera,
            &key("close"),
            InterfaceControl::button("document/scripts", "Close demonstration"),
            Some(""),
            NativeCommand::File(FileCommand::ScriptPreview(Action::Close)),
            rect(x + w - 44., y + 16., 28., 28.),
            Some(interface_shell::ribbon::Icon::Cancel),
            61,
        )?;
        Ok(())
    }
}
