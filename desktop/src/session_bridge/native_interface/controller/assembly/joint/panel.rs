//! Joint dialog geometry follows the original 380 px side form. Its controls
//! remain ordinary native fields while connector picks stay on the live canvas.
use super::*;
use crate::native_viewport::interface_shell::{
    self, ribbon::Icon, InterfaceCamera, InterfaceControl,
};
use limo_cad_interface::{ChoiceOption, Field as ValueField, KeyChord};
struct Paint<'a> {
    world: &'a mut World,
    widgets: &'a mut chrome::Widgets,
    camera: Entity,
    x: f32,
    top: f32,
    bottom: f32,
    width: f32,
    scroll: f32,
    id: u64,
}
impl Paint<'_> {
    fn card(&mut self, key: &str, x: f32, y: f32, w: f32, h: f32, accent: bool) {
        let y = self.top + y - self.scroll;
        if y < self.top || y + h > self.bottom {
            return;
        }
        let theme = crate::native_viewport::ui::theme(self.world);
        let mut bounds = chrome::rect(self.x + x, y, w, h);
        bounds.border = UiRect::all(px(1.));
        self.widgets.panel(
            self.world,
            self.camera,
            key,
            bounds,
            if accent {
                theme.accent_soft
            } else {
                theme.header
            },
            42,
        );
        if let Some(entity) = self.widgets.entity(key) {
            self.world
                .entity_mut(entity)
                .insert(BorderColor::all(if accent {
                    theme.accent
                } else {
                    theme.edge
                }));
        }
    }
    fn text(&mut self, key: &str, value: &str, (x, y, w, h): (f32, f32, f32, f32), size: f32) {
        let y = self.top + y - self.scroll;
        if y < self.top || y + h > self.bottom {
            return;
        }
        self.widgets.text(
            self.world,
            self.camera,
            key,
            chrome::rect(self.x + x, y, w, h),
            value,
            size,
            43,
        );
    }
    fn button(
        &mut self,
        (key, label, caption): (&str, &str, Option<&str>),
        action: Action,
        (x, y, w, h): (f32, f32, f32, f32),
        field: ValueField,
        selected: Option<bool>,
        disabled: bool,
    ) -> Result<Entity, String> {
        let y = self.top + y - self.scroll;
        if y < self.top || y + h > self.bottom {
            return Ok(Entity::PLACEHOLDER);
        }
        let mut control = InterfaceControl::button("assembly/joints", label);
        control.field = field;
        control.selected = selected;
        control.disabled = disabled;
        let choice = matches!(control.field, ValueField::Choice { .. });
        let text = matches!(control.field, ValueField::Text { .. });
        let toggle = if let ValueField::Toggle(v) = control.field {
            Some(v)
        } else {
            None
        };
        let mut bounds = chrome::rect(self.x + x, y, w, h);
        bounds.border = UiRect::all(px(1.));
        if text {
            bounds.padding = UiRect::axes(px(7.), px(3.));
        }
        if toggle.is_some() {
            control.role = "checkbox".into();
            control.selected = toggle;
            bounds.border = default();
        }
        if choice {
            control.role = "combobox".into();
            control.expanded = selected;
            control.selected = None;
            control.owned_keys = ["ArrowUp", "ArrowDown", "Home", "End"]
                .map(KeyChord::plain)
                .into();
        }
        if action == Action::Orientation {
            control.expanded = selected;
            control.selected = None;
        }
        let current = if let ValueField::Choice { value, options } = &control.field {
            Some(
                options
                    .iter()
                    .find(|o| o.value == *value)
                    .map_or(value.clone(), |o| o.label.clone()),
            )
        } else {
            None
        };
        let fresh = self.widgets.entity(key).is_none();
        let entity = self.widgets.button(
            self.world,
            self.camera,
            key,
            control,
            current.as_deref().or(caption),
            NativeCommand::Assembly(super::super::Command::Joint(Command::Control {
                id: self.id,
                action,
            })),
            bounds,
            None,
            44,
        )?;
        if let Some(checked) = toggle {
            interface_shell::checkbox_button(self.world, entity, self.camera, checked);
        }
        if !text {
            interface_shell::caption_size(self.world, entity, 12.);
        }
        if choice && fresh {
            let icon =
                interface_shell::ribbon::compact_glyph(self.world, entity, Icon::Chevron, 0., 10.);
            let mut n = self.world.get::<Node>(icon).unwrap().clone();
            n.left = Val::Auto;
            n.right = px(8.);
            n.top = px(11.);
            self.world.entity_mut(icon).insert(n);
        }
        Ok(entity)
    }
    fn input(
        &mut self,
        key: &str,
        label: &str,
        value: &str,
        field: form::Field,
        (x, y, w): (f32, f32, f32),
    ) -> Result<(), String> {
        self.button(
            (key, label, None),
            Action::Field(field),
            (x, y, w, 32.),
            ValueField::Text {
                value: value.into(),
                read_only: false,
                selection: None,
            },
            None,
            false,
        )?;
        Ok(())
    }
    fn label(&mut self, key: &str, label: &str, y: f32) {
        self.text(key, label, (12., y, self.width - 24., 18.), 10.);
    }
}
pub(super) fn paint(
    world: &mut World,
    widgets: &mut chrome::Widgets,
    e: &mut Editor,
    a: &AssemblyDocumentDto,
    bounds: InterfaceRect,
) -> Result<(), String> {
    let camera = world
        .query_filtered::<Entity, With<InterfaceCamera>>()
        .single(world)
        .map_err(|_| "Interface camera unavailable")?;
    let (x, top, w, h) = (
        bounds.x as f32,
        bounds.y as f32,
        bounds.width as f32,
        bounds.height as f32,
    );
    let theme = crate::native_viewport::ui::theme(world);
    let mut background = chrome::rect(x, top, w, h);
    background.border = UiRect::all(px(1.));
    background.border_radius = BorderRadius::all(px(12.));
    widgets.panel(
        world,
        camera,
        "joint-background",
        background,
        theme.panel.with_alpha(1.),
        41,
    );
    if let Some(entity) = widgets.entity("joint-background") {
        world
            .entity_mut(entity)
            .insert(BorderColor::all(theme.accent));
    }
    let mut header = chrome::rect(x + 1., top + 1., w - 2., 43.);
    header.border_radius = BorderRadius::px(11., 11., 0., 0.);
    widgets.panel(world, camera, "joint-header", header, theme.header, 42);
    let mut footer_bounds = chrome::rect(x + 1., top + h - 44., w - 2., 43.);
    footer_bounds.border_radius = BorderRadius::px(0., 0., 11., 11.);
    widgets.panel(
        world,
        camera,
        "joint-footer",
        footer_bounds,
        theme.header,
        42,
    );
    widgets.glyph(
        (world, camera),
        "joint-title-icon",
        chrome::rect(x + 12., top + 15., 16., 16.),
        Icon::Joint,
        theme.accent,
        43,
    );
    let mut p = Paint {
        world,
        widgets,
        camera,
        x,
        top,
        bottom: top + h,
        width: w,
        scroll: 0.,
        id: e.id,
    };
    p.text(
        "title",
        if e.form.original.is_some() {
            "Edit joint"
        } else {
            "Create joint"
        },
        (36., 10., w - 79., 24.),
        12.,
    );
    p.button(
        ("close", "Close joint editor", Some("×")),
        Action::Cancel,
        (w - 36., 6., 28., 28.),
        ValueField::None,
        None,
        false,
    )?;
    let footer = h - 37.;
    p.button(
        ("cancel", "Cancel joint", Some("Cancel")),
        Action::Cancel,
        (w - 192., footer, 78., 28.),
        ValueField::None,
        None,
        false,
    )?;
    let invalid = e.form.request(a).err();
    let apply = p.button(
        (
            "apply",
            "Apply joint",
            Some(if e.form.original.is_some() {
                "Save joint"
            } else {
                "Create joint"
            }),
        ),
        Action::Apply,
        (w - 106., footer, 94., 28.),
        ValueField::None,
        None,
        invalid.is_some() || e.error.is_some(),
    )?;
    if apply != Entity::PLACEHOLDER {
        interface_shell::primary_button(p.world, apply);
    }
    for (key, label, action, offset, disabled) in [
        (
            "up",
            "Scroll joint up",
            Action::Scroll(-160),
            12.,
            e.scroll <= 0.,
        ),
        (
            "down",
            "Scroll joint down",
            Action::Scroll(160),
            38.,
            e.scroll >= e.max_scroll,
        ),
    ] {
        if e.max_scroll > 0. {
            p.button(
                (key, label, Some(if key == "up" { "↑" } else { "↓" })),
                action,
                (offset, footer, 24., 28.),
                ValueField::None,
                None,
                disabled,
            )?;
        }
    }
    p.top += 44.;
    p.bottom -= 44.;
    p.scroll = e.scroll;
    let mut y = 10.;
    p.card("hint-card", 12., y, w - 24., 84., true);
    p.text(
        "hint-title",
        "Selecting joint connectors",
        (22., y + 9., w - 80., 18.),
        11.,
    );
    p.text(
        "hint-count",
        &format!("{}/2", e.form.connectors.iter().flatten().count()),
        (w - 46., y + 9., 28., 18.),
        10.,
    );
    p.text("hint",
        "Pick two connectors on different components. Clear and repick them to repair a broken topology reference.",
        (22., y + 29., w - 44., 48.),
        11.);
    y += 96.;
    let cw = (w - 32.) * 0.5;
    for i in 0..2 {
        let label = format!("Connector {}", ["A", "B"][i]);
        p.text(
            &format!("connector-{i}-label"),
            &label.to_uppercase(),
            (12. + i as f32 * (cw + 8.), y, cw, 18.),
            10.,
        );
        let caption = e.form.connectors[i]
            .as_ref()
            .map_or("Pick connector", |c| c.label.as_str());
        let entity = p.button(
            (
                &format!("connector-{i}"),
                &format!("Pick {label}"),
                Some(caption),
            ),
            Action::Pick(i),
            (12. + i as f32 * (cw + 8.), y + 20., cw, 36.),
            ValueField::None,
            Some(e.pick == Some(i)),
            false,
        )?;
        if entity != Entity::PLACEHOLDER {
            interface_shell::caption_size(p.world, entity, 11.);
        }
    }
    y += 66.;
    p.button(
        ("clear", "Clear joint connectors", Some("Clear selection")),
        Action::Clear,
        (12., y, 118., 28.),
        ValueField::None,
        None,
        false,
    )?;
    y += 40.;
    if e.form.connectors.iter().all(Option::is_some) {
        p.label("fixed-label", "FIXED COMPONENT", y);
        y += 20.;
        for i in 0..2 {
            let entity = p.button(
                (
                    &format!("fixed-{i}"),
                    &format!("Fix connector {}", ["A", "B"][i]),
                    e.form.connectors[i].as_ref().map(|c| c.label.as_str()),
                ),
                Action::Ground(i),
                (12. + i as f32 * (cw + 8.), y, cw, 38.),
                ValueField::None,
                Some(e.form.fixed == Some(i)),
                false,
            )?;
            if entity != Entity::PLACEHOLDER {
                interface_shell::radio_card(p.world, entity, p.camera, e.form.fixed == Some(i));
            }
        }
        y += 44.;
        p.text(
            "ground-hint",
            if e.form.fixed.is_none() {
                "The existing fixed component is preserved."
            } else {
                "The fixed component stays in place when the joint solves."
            },
            (12., y, w - 24., 32.),
            10.,
        );
        y += 40.;
    }
    p.label("name-label", "NAME", y);
    y += 20.;
    p.input(
        "name",
        "Joint name",
        &e.form.name,
        form::Field::Name,
        (12., y, w - 24.),
    )?;
    y += 42.;
    p.label("kind-label", "JOINT TYPE", y);
    y += 20.;
    let value = form::KINDS
        .iter()
        .find(|(k, _, _)| *k == e.form.kind)
        .unwrap()
        .1;
    p.button(
        ("kind", "Joint type", None),
        Action::Field(form::Field::Kind),
        (12., y, w - 24., 32.),
        ValueField::Choice {
            value: value.into(),
            options: form::KINDS
                .iter()
                .map(|(_, v, l)| ChoiceOption {
                    value: (*v).into(),
                    label: (*l).into(),
                    disabled: false,
                })
                .collect(),
        },
        Some(e.choice),
        false,
    )?;
    y += 38.;
    if e.choice {
        for (i, (kind, _, label)) in form::KINDS.iter().enumerate() {
            p.button(
                (
                    &format!("kind-{i}"),
                    &format!("Joint type {label}"),
                    Some(label),
                ),
                Action::ChooseKind(i),
                (12., y, w - 24., 28.),
                ValueField::None,
                Some(*kind == e.form.kind),
                false,
            )?;
            y += 30.;
        }
    }
    use limo_cad_sketch::JointKindDto;
    let help = match e.form.kind {
        JointKindDto::Rigid => "Locks every relative degree of freedom.",
        JointKindDto::Revolute => "Allows rotation around the connector axis.",
        JointKindDto::Slider => "Allows travel along the connector axis.",
        JointKindDto::Cylindrical => "Allows rotation and travel along one axis.",
        JointKindDto::Planar => "Allows two in-plane translations and one rotation.",
        JointKindDto::Ball => "Allows three rotations around a common origin.",
        JointKindDto::PinSlot => "Allows a pin to rotate and slide along a slot.",
        JointKindDto::Screw => "Couples rotation to axial travel using the thread pitch.",
        JointKindDto::Universal => "Allows two rotations around perpendicular axes.",
    };
    p.text("kind-help", help, (12., y, w - 24., 28.), 10.);
    y += 34.;
    for (index, title) in e.form.axes() {
        let coord = &e.form.coordinates[index];
        p.label(
            &format!("coordinate-{index}-label"),
            &title.to_uppercase(),
            y,
        );
        y += 20.;
        let unit = if matches!(index, 1 | 4) {
            match e.form.units {
                UnitSystem::Mm => "mm",
                UnitSystem::Cm => "cm",
                UnitSystem::In => "in",
            }
        } else {
            "deg"
        };
        p.text(
            &format!("offset-{index}-label"),
            &format!("Offset ({unit})"),
            (12., y, w - 24., 18.),
            10.,
        );
        y += 20.;
        p.input(
            &format!("coordinate-{index}"),
            &format!("{title} offset"),
            coord.values[0].text(),
            form::Field::Coordinate(index, 0),
            (12., y, w - 24.),
        )?;
        y += 36.;
        p.button(
            (
                &format!("limit-{index}"),
                &format!("Limit {title}"),
                Some("Limit motion"),
            ),
            Action::Field(form::Field::Limited(index)),
            (12., y, w - 24., 28.),
            ValueField::Toggle(coord.limited),
            None,
            false,
        )?;
        y += 34.;
        if coord.limited {
            for j in 1..3 {
                let label = if j == 1 { "Minimum" } else { "Maximum" };
                let x = 12. + (j - 1) as f32 * (cw + 8.);
                p.text(
                    &format!("limit-{index}-{j}-label"),
                    label,
                    (x, y, cw, 18.),
                    10.,
                );
                p.input(
                    &format!("limit-{index}-{j}"),
                    &format!("{title} {label}"),
                    coord.values[j].text(),
                    form::Field::Coordinate(index, j),
                    (x, y + 20., cw),
                )?;
            }
            y += 62.;
        }
    }
    if e.form.kind == limo_cad_sketch::JointKindDto::Screw {
        p.label("pitch-label", "SCREW PITCH · DISTANCE PER REVOLUTION", y);
        y += 20.;
        p.input(
            "pitch",
            "Screw pitch",
            e.form.pitch.text(),
            form::Field::Pitch,
            (12., y, w - 24.),
        )?;
        y += 42.;
    }
    p.button(
        ("orientation", "Connector orientation", None),
        Action::Orientation,
        (12., y, w - 24., 30.),
        ValueField::None,
        Some(e.orientation),
        false,
    )?;
    y += 36.;
    if e.orientation {
        for i in 0..2 {
            let x = 12. + i as f32 * (cw + 8.);
            let label = format!("Connector {} twist", ["A", "B"][i]);
            p.text(
                &format!("twist-{i}-label"),
                &format!("{label} (deg)"),
                (x, y, cw, 18.),
                10.,
            );
            p.input(
                &format!("twist-{i}"),
                &label,
                e.form.twists[i].text(),
                form::Field::Twist(i),
                (x, y + 20., cw),
            )?;
        }
        y += 62.;
    }
    p.button(
        ("flip", "Flip joint direction", Some("Flip direction")),
        Action::Field(form::Field::Flipped),
        (12., y, w - 24., 28.),
        ValueField::Toggle(e.form.flipped),
        None,
        false,
    )?;
    y += 34.;
    p.text("preview-note",
        "Offsets and connector orientation preview in the model. Apply saves the joint; Cancel restores its original placement.",
        (12., y, w-24., 45.),
        10.);
    y += 51.;
    if let Some(error) = e.error.as_ref().or(invalid.as_ref()) {
        p.text("error", error, (12., y, w - 24., 44.), 11.);
        y += 50.;
    }
    e.max_scroll = (y - (h - 88.)).max(0.);
    e.scroll = e.scroll.min(e.max_scroll);
    Ok(())
}
