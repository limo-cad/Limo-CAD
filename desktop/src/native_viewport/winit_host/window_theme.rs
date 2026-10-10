//! Capture OS appearance on Winit's thread before the controller resolves System.
use bevy::{
    ecs::system::NonSendMarker,
    prelude::*,
    window::{PrimaryWindow, WindowCreated, WindowTheme, WindowThemeChanged},
    winit::{converters::convert_winit_theme, WINIT_WINDOWS},
};

#[derive(Resource, Default)]
pub(crate) struct SystemTheme {
    window: Option<Entity>,
    theme: Option<WindowTheme>,
}

impl SystemTheme {
    pub(crate) fn is_dark(&self) -> bool {
        self.theme == Some(WindowTheme::Dark)
    }
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<SystemTheme>()
        .add_systems(First, capture_system_theme);
}

fn capture_system_theme(
    primary: Query<Entity, With<PrimaryWindow>>,
    mut created: MessageReader<WindowCreated>,
    mut changed: MessageReader<WindowThemeChanged>,
    mut theme: ResMut<SystemTheme>,
    _main_thread: NonSendMarker,
) {
    let Ok(window) = primary.single() else {
        return;
    };
    let was_created = created.read().any(|event| event.window == window);
    if theme.window != Some(window) || was_created {
        theme.window = Some(window);
        theme.theme = WINIT_WINDOWS.with_borrow(|windows| {
            windows
                .get_window(window)
                .and_then(|window| window.theme())
                .map(convert_winit_theme)
        });
    }
    for event in changed.read().filter(|event| event.window == window) {
        theme.theme = Some(event.theme);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_theme_changes_without_a_world_winit_resource_and_ignores_other_windows() {
        let mut app = App::new();
        app.add_message::<WindowCreated>()
            .add_message::<WindowThemeChanged>();
        install(&mut app);
        let primary = app.world_mut().spawn(PrimaryWindow).id();
        let other = app.world_mut().spawn_empty().id();
        for (window, reported, expected) in [
            (primary, WindowTheme::Dark, true),
            (other, WindowTheme::Light, true),
            (primary, WindowTheme::Light, false),
            (primary, WindowTheme::Dark, true),
        ] {
            app.world_mut().write_message(WindowThemeChanged {
                window,
                theme: reported,
            });
            app.update();
            assert_eq!(app.world().resource::<SystemTheme>().is_dark(), expected);
        }
    }
}
