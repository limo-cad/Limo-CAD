//! Compact native structure tree and instance inspector using the original
//! sidebar dimensions and SVGs. No controls exist outside their clipped area.
use super::*;
use crate::native_viewport::interface_shell::{ribbon::Icon, InterfaceControl};
use limo_cad_interface::{ChoiceOption, KeyChord};

pub(super) struct Paint<'a> {
    pub(super) world: &'a mut World,
    widgets: &'a mut chrome::Widgets,
    camera: Entity,
    x: f32,
    top: f32,
    bottom: f32,
    scroll: f32,
}
impl Paint<'_> {
    pub(super) fn card(&mut self, key: &str, y: f32, w: f32, h: f32) {
        let top = (self.top + y - self.scroll).max(self.top);
        let bottom = (self.top + y + h - self.scroll).min(self.bottom);
        if bottom <= top {
            return;
        }
        let theme = crate::native_viewport::ui::theme(self.world);
        let mut bounds = chrome::rect(self.x + 8., top, w - 16., bottom - top);
        bounds.border = UiRect::all(px(1.));
        self.widgets
            .panel(self.world, self.camera, key, bounds, theme.header, 34);
        if let Some(e) = self.widgets.entity(key) {
            self.world
                .entity_mut(e)
                .insert(BorderColor::all(theme.edge));
        }
    }
    pub(super) fn heading(&mut self, key: &str, label: &str, icon: Icon, y: f32, w: f32) {
        self.text(key, label, (28., y, w - 38., 20.), 10.);
        let top = self.top + y - self.scroll;
        if top < self.top || top + 20. > self.bottom {
            return;
        }
        let theme = crate::native_viewport::ui::theme(self.world);
        self.widgets.glyph(
            (self.world, self.camera),
            &format!("{key}-icon"),
            chrome::rect(self.x + 10., top + 4., 12., 12.),
            icon,
            theme.accent,
            35,
        );
    }
    pub(super) fn warning(&mut self, key: &str) {
        if let Some(e) = self.widgets.entity(key) {
            self.world
                .entity_mut(e)
                .insert(TextColor(Color::srgb_u8(239, 94, 103)));
        }
    }
    pub(super) fn button(
        &mut self,
        (key, label, caption): (&str, &str, Option<&str>),
        command: Command,
        (x, y, w, h): (f32, f32, f32, f32),
        icon: Option<Icon>,
        (disabled, selected): (bool, Option<bool>),
        field: Field,
    ) -> Result<(), String> {
        let y = self.top + y - self.scroll;
        if y < self.top || y + h > self.bottom {
            return Ok(());
        }
        let mut control = InterfaceControl::button(
            if matches!(command, Command::Study(_)) {
                "assembly/motion"
            } else if matches!(command, Command::Inspect(_)) {
                "assembly/inspect"
            } else {
                "assembly/joints"
            },
            label,
        );
        control.disabled = disabled;
        control.selected = selected;
        if matches!(command, Command::Components | Command::Inspector) {
            control.selected = None;
            control.expanded = selected;
        }
        control.field = field;
        if matches!(command, Command::Tab(_)) {
            control.role = "tab".into();
        }
        let toggle = if let Field::Toggle(value) = control.field {
            Some(value)
        } else {
            None
        };
        if toggle.is_some() {
            control.role = "checkbox".into();
            control.selected = toggle;
        }
        if matches!(command, Command::Select(_)) {
            control.role = "treeitem".into();
            if selected == Some(true) {
                control.owned_keys = vec![KeyChord::plain("Delete")];
            }
        }
        if matches!(control.field, Field::Text { .. }) {
            control.role = "textbox".into();
            control.owned_keys = vec![KeyChord {
                key: "Enter".into(),
                ..default()
            }];
        }
        let choice_caption = if let Field::Choice { value, options } = &control.field {
            control.role = "combobox".into();
            control.expanded = selected;
            control.selected = None;
            control.owned_keys = ["ArrowUp", "ArrowDown", "Home", "End"]
                .map(KeyChord::plain)
                .into();
            Some(
                options
                    .iter()
                    .find(|o| o.value == *value)
                    .map(|o| o.label.clone())
                    .unwrap_or_else(|| value.clone()),
            )
        } else {
            None
        };
        let mut bounds = chrome::rect(self.x + x, y, w, h);
        let primary = matches!(
            command,
            Command::Inspect(inspect::Action::Check)
                | Command::Study(studies::Action::Capture | studies::Action::Play)
        );
        if matches!(control.field, Field::Text { .. } | Field::Choice { .. })
            || matches!(
                command,
                Command::Create(_)
                    | Command::Add(_)
                    | Command::Rename(..)
                    | Command::Remove(_)
                    | Command::ApplyTransform(..)
                    | Command::Study(_)
                    | Command::Motion(
                        motion::Action::Demo | motion::Action::Revert | motion::Action::Save
                    )
                    | Command::Inspect(
                        inspect::Action::Check
                            | inspect::Action::Swept
                            | inspect::Action::CreateContact
                            | inspect::Action::ApplyContact(_)
                    )
            )
        {
            bounds.border = UiRect::all(px(1.));
            if matches!(control.field, Field::Text { .. }) {
                bounds.padding = UiRect::axes(px(6.), px(2.));
            }
        }
        let text_field = matches!(control.field, Field::Text { .. });
        let choice = matches!(control.field, Field::Choice { .. });
        let fresh = self.widgets.entity(key).is_none();
        let entity = self.widgets.button(
            self.world,
            self.camera,
            key,
            control,
            choice_caption.as_deref().or(caption),
            NativeCommand::Assembly(command),
            bounds,
            icon,
            35,
        )?;
        if primary {
            interface_shell::primary_button(self.world, entity);
        }
        if let Some(value) = toggle {
            interface_shell::checkbox_button(self.world, entity, self.camera, value);
        }
        if text_field {
            if let Some(mut font) = self.world.get_mut::<TextFont>(entity) {
                if font.font_size != bevy::text::FontSize::Px(11.) {
                    font.font_size = bevy::text::FontSize::Px(11.);
                }
            }
        } else {
            interface_shell::caption_size(self.world, entity, 10.);
        }
        if choice && fresh {
            let glyph =
                interface_shell::ribbon::compact_glyph(self.world, entity, Icon::Chevron, 0., 10.);
            let mut node = self.world.get::<Node>(glyph).unwrap().clone();
            node.left = Val::Auto;
            node.right = px(7.);
            node.top = px(9.);
            self.world.entity_mut(glyph).insert(node);
        }
        Ok(())
    }
    pub(super) fn text(
        &mut self,
        key: &str,
        value: &str,
        (x, y, w, h): (f32, f32, f32, f32),
        size: f32,
    ) {
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
            35,
        );
    }
    pub(super) fn input(
        &mut self,
        key: &str,
        label: &str,
        value: &str,
        command: Command,
        (x, y, w): (f32, f32, f32),
        disabled: bool,
    ) -> Result<(), String> {
        self.button(
            (key, label, None),
            command,
            (x, y, w, 28.),
            None,
            (disabled, None),
            Field::Text {
                value: value.into(),
                read_only: false,
                selection: None,
            },
        )
    }
}

pub(super) fn paint(
    world: &mut World,
    state: &mut Browser,
    a: &AssemblyDocumentDto,
    bounds: InterfaceRect,
) -> Result<(), String> {
    let camera = world
        .query_filtered::<Entity, With<InterfaceCamera>>()
        .single(world)
        .map_err(|_| "Native interface camera is missing")?;
    let width = bounds.width as f32;
    let height = bounds.height as f32;
    let top = bounds.y as f32;
    let (_, _, view, _) = native_viewport::interface_view(world);
    let selected = view.selected_occurrence_id;
    let no_bodies_selected = view.selected_body_ids.is_empty();
    let form = feature::panel(world);
    let blocked = form.is_some()
        || joint::active(world)
        || view.mode == native_viewport::ViewportMode::Sketch;
    let mut p = Paint {
        world,
        widgets: &mut state.widgets,
        camera,
        x: bounds.x as f32,
        top,
        bottom: top + height,
        scroll: 0.,
    };
    p.button(
        ("back", "Back to model browser", Some("MODEL")),
        Command::Show(false),
        (4., 2., 100., 28.),
        Some(Icon::ArrowLeft),
        (false, None),
        Field::None,
    )?;
    p.text("title", "ASSEMBLY", (width - 110., 5., 100., 22.), 10.);
    for (i, (tab, label)) in [
        (Tab::Structure, "Structure"),
        (Tab::Motion, "Motion"),
        (Tab::Inspect, "Inspect"),
    ]
    .into_iter()
    .enumerate()
    {
        p.button(
            (
                &format!("tab-{i}"),
                &format!("Assembly {label}"),
                Some(label),
            ),
            Command::Tab(tab),
            (
                4. + i as f32 * (width - 8.) / 3.,
                34.,
                (width - 8.) / 3.,
                28.,
            ),
            None,
            (blocked, Some(state.tab == tab)),
            Field::None,
        )?;
    }
    p.top += 64.;
    p.bottom -= 24.;
    p.scroll = state.scroll;
    let mut y = 0.;
    if state.tab == Tab::Motion {
        y = studies::panel::paint(&mut p, &mut state.studies, a, width, blocked)?;
    } else if state.tab == Tab::Inspect {
        y = inspect::panel::paint(&mut p, &mut state.inspect, a, state.units, width, blocked)?;
    } else {
        p.button(
            ("components", "Components", Some("COMPONENTS")),
            Command::Components,
            (4., y, width - 8., 28.),
            Some(if state.components {
                Icon::ChevronDown
            } else {
                Icon::ChevronRight
            }),
            (false, Some(state.components)),
            Field::None,
        )?;
        y += 34.;
        if state.components {
            let half = (width - 18.) * 0.5;
            p.button(
                ("create", "Make component", None),
                Command::Create(false),
                (6., y, half, 28.),
                Some(Icon::Box),
                (blocked || no_bodies_selected, None),
                Field::None,
            )?;
            p.button(
                ("group", "Create subassembly", Some("Subassembly")),
                Command::Create(true),
                (12. + half, y, half, 28.),
                Some(Icon::FolderTree),
                (blocked, None),
                Field::None,
            )?;
            y += 38.;
            let mut rows = vec![];
            let mut pending = a
                .component_structure
                .occurrences
                .iter()
                .filter(|o| o.parent_occurrence_id.is_none())
                .rev()
                .map(|o| (o, 0))
                .collect::<Vec<_>>();
            while let Some((o, depth)) = pending.pop() {
                rows.push((o, depth));
                if !state.collapsed.contains(&o.id.0) {
                    pending.extend(
                        a.component_structure
                            .occurrences
                            .iter()
                            .filter(|child| child.parent_occurrence_id == Some(o.id))
                            .rev()
                            .map(|child| (child, depth + 1)),
                    );
                }
            }
            if rows.is_empty() {
                p.text(
                    "empty",
                    "Make a component from selected bodies",
                    (12., y, width - 24., 38.),
                    10.,
                );
                y += 44.;
            }
            for (o, depth) in rows {
                let id = o.id.0;
                let key = format!("occurrence-{id}");
                let children = a
                    .component_structure
                    .occurrences
                    .iter()
                    .any(|child| child.parent_occurrence_id == Some(o.id));
                let d = a
                    .component_structure
                    .definitions
                    .iter()
                    .find(|d| d.id == o.component_id);
                let x = 6. + 13. * (depth as f32).min(5.);
                p.button(
                    (
                        &format!("{key}-expand"),
                        &format!(
                            "{} {}",
                            if state.collapsed.contains(&id) {
                                "Expand"
                            } else {
                                "Collapse"
                            },
                            o.name
                        ),
                        Some(""),
                    ),
                    Command::Expand(id),
                    (x, y, 18., 28.),
                    Some(if state.collapsed.contains(&id) {
                        Icon::ChevronRight
                    } else {
                        Icon::ChevronDown
                    }),
                    (!children, None),
                    Field::None,
                )?;
                p.button(
                    (&key, &format!("Component {}", o.name), Some(&o.name)),
                    Command::Select(id),
                    (x + 20., y, (width - x - 110.).max(20.), 28.),
                    Some(if children || d.is_some_and(|d| d.body_ids.is_empty()) {
                        Icon::FolderTree
                    } else {
                        Icon::Box
                    }),
                    (false, Some(selected == Some(id))),
                    Field::None,
                )?;
                for (index, (suffix, label, command, icon, on)) in [
                    (
                        "move",
                        format!("Move {}", o.name),
                        Command::Move(id),
                        Icon::MoveCopy,
                        false,
                    ),
                    (
                        "ground",
                        format!(
                            "{} {}",
                            if o.grounded { "Release" } else { "Ground" },
                            o.name
                        ),
                        Command::Ground(id),
                        Icon::Anchor,
                        o.grounded,
                    ),
                    (
                        "copy",
                        format!("Duplicate {}", o.name),
                        Command::Duplicate(id),
                        Icon::Copy,
                        false,
                    ),
                    (
                        "visible",
                        format!("{} {}", if o.visible { "Hide" } else { "Show" }, o.name),
                        Command::Visibility(id),
                        if o.visible { Icon::Eye } else { Icon::EyeOff },
                        false,
                    ),
                ]
                .into_iter()
                .enumerate()
                {
                    p.button(
                        (&format!("{key}-{suffix}"), &label, Some("")),
                        command,
                        (width - 88. + index as f32 * 21., y, 20., 28.),
                        Some(icon),
                        (blocked, Some(on)),
                        Field::None,
                    )?;
                }
                y += 32.;
            }
            if !a.component_structure.definitions.is_empty() {
                y += 8.;
                let value = state
                    .definition
                    .map(|id| id.to_string())
                    .unwrap_or_default();
                let options = a
                    .component_structure
                    .definitions
                    .iter()
                    .map(|d| ChoiceOption {
                        value: d.id.0.to_string(),
                        label: format!(
                            "{} · {} {}",
                            d.name,
                            d.body_ids.len(),
                            if d.body_ids.len() == 1 {
                                "body"
                            } else {
                                "bodies"
                            }
                        ),
                        disabled: false,
                    })
                    .collect();
                p.button(
                    ("definitions", "Reusable component definition", None),
                    Command::Definitions,
                    (6., y, width - 104., 28.),
                    None,
                    (blocked, Some(state.definitions_open)),
                    Field::Choice { value, options },
                )?;
                p.button(
                    ("add-root", "Add root instance", Some("+ Root")),
                    Command::Add(false),
                    (width - 92., y, 44., 28.),
                    None,
                    (blocked, None),
                    Field::None,
                )?;
                p.button(
                    ("add-child", "Add child instance", Some("+ Child")),
                    Command::Add(true),
                    (width - 46., y, 44., 28.),
                    None,
                    (blocked || selected.is_none(), None),
                    Field::None,
                )?;
                y += 34.;
                if state.definitions_open {
                    for d in &a.component_structure.definitions {
                        p.button(
                            (
                                &format!("definition-{}", d.id.0),
                                &format!("Use {}", d.name),
                                Some(&d.name),
                            ),
                            Command::Definition(d.id.0),
                            (6., y, width - 12., 28.),
                            Some(Icon::Box),
                            (blocked, Some(state.definition == Some(d.id.0))),
                            Field::None,
                        )?;
                        y += 30.;
                    }
                }
            }
        }
        if let Some(draft) = &state.draft {
            let o = find(a, draft.occurrence)?;
            let d = a
                .component_structure
                .definitions
                .iter()
                .find(|d| d.id == o.component_id)
                .ok_or("Component definition disappeared")?;
            let id = o.id.0;
            p.button(
                ("inspector", "Selected instance", Some("SELECTED INSTANCE")),
                Command::Inspector,
                (4., y, width - 8., 28.),
                Some(Icon::Box),
                (false, Some(state.inspector)),
                Field::None,
            )?;
            y += 34.;
            if state.inspector {
                p.button(
                    (
                        "remove-instance",
                        &format!("Remove instance {}", o.name),
                        Some("Remove instance"),
                    ),
                    Command::Remove(id),
                    (8., y, width - 16., 28.),
                    None,
                    (blocked, None),
                    Field::None,
                )?;
                y += 34.;
                p.text(
                    "remove-instance-hint",
                    "Retains the reusable definition and source geometry. Undo restores this instance.",
                    (8., y, width - 16., 40.),
                    9.,
                );
                y += 46.;
                for definition in [false, true] {
                    let tag = if definition { "definition" } else { "instance" };
                    let label = if definition {
                        "Reusable definition"
                    } else {
                        "Instance name"
                    };
                    let name = if definition {
                        &draft.definition_name
                    } else {
                        &draft.name
                    };
                    let original = if definition { &d.name } else { &o.name };
                    p.text(
                        &format!("{tag}-name-label"),
                        &label.to_uppercase(),
                        (8., y, width - 16., 18.),
                        9.,
                    );
                    y += 20.;
                    p.input(
                        &format!("{tag}-name"),
                        label,
                        name,
                        Command::Edit(
                            id,
                            if definition {
                                EditField::DefinitionName
                            } else {
                                EditField::Name
                            },
                        ),
                        (8., y, width - 80.),
                        blocked,
                    )?;
                    p.button(
                        (
                            &format!("{tag}-rename"),
                            &format!("Rename {tag}"),
                            Some("Rename"),
                        ),
                        Command::Rename(id, definition),
                        (width - 68., y, 60., 28.),
                        None,
                        (
                            blocked || name.trim().is_empty() || name.trim() == original,
                            None,
                        ),
                        Field::None,
                    )?;
                    y += 36.;
                    if !definition {
                        p.text(
                            "parent-label",
                            "PARENT COORDINATE SYSTEM",
                            (8., y, width - 16., 18.),
                            9.,
                        );
                        y += 20.;
                        let excluded = descendants(a, id);
                        let mut options = vec![ChoiceOption {
                            value: "root".into(),
                            label: "Document root".into(),
                            disabled: false,
                        }];
                        options.extend(
                            a.component_structure
                                .occurrences
                                .iter()
                                .filter(|o| !excluded.contains(&o.id.0))
                                .map(|o| ChoiceOption {
                                    value: o.id.0.to_string(),
                                    label: o.name.clone(),
                                    disabled: false,
                                }),
                        );
                        p.button(
                            ("parent", "Parent coordinate system", None),
                            Command::Parents,
                            (8., y, width - 16., 28.),
                            None,
                            (blocked, Some(state.parents_open)),
                            Field::Choice {
                                value: o
                                    .parent_occurrence_id
                                    .map(|id| id.0.to_string())
                                    .unwrap_or("root".into()),
                                options: options.clone(),
                            },
                        )?;
                        y += 34.;
                        if state.parents_open {
                            for option in options {
                                let parent = option.value.parse::<u64>().ok();
                                p.button(
                                    (
                                        &format!("parent-{}", option.value),
                                        &format!("Parent {}", option.label),
                                        Some(&option.label),
                                    ),
                                    Command::Parent(id, parent),
                                    (8., y, width - 16., 28.),
                                    None,
                                    (
                                        blocked,
                                        Some(o.parent_occurrence_id.map(|id| id.0) == parent),
                                    ),
                                    Field::None,
                                )?;
                                y += 30.;
                            }
                        }
                    }
                    let title = if definition {
                        "Component coordinate system"
                    } else {
                        "Instance placement"
                    };
                    let transform = if definition {
                        &draft.origin
                    } else {
                        &draft.placement
                    };
                    p.text(
                        &format!("{tag}-pose-label"),
                        &title.to_uppercase(),
                        (8., y, width - 16., 18.),
                        9.,
                    );
                    y += 22.;
                    let w = (width - 42.) / 3.;
                    for rotation in [false, true] {
                        p.text(
                            &format!("{tag}-{}-units", rotation),
                            if rotation {
                                "deg"
                            } else {
                                match state.units {
                                    UnitSystem::Mm => "mm",
                                    UnitSystem::Cm => "cm",
                                    UnitSystem::In => "in",
                                }
                            },
                            (5., y, 24., 28.),
                            8.,
                        );
                        for i in 0..3 {
                            let field = if rotation {
                                EditField::Rotation(definition, i)
                            } else {
                                EditField::Translation(definition, i)
                            };
                            let value = if rotation {
                                transform.rotation[i].text()
                            } else {
                                transform.translation[i].text()
                            };
                            p.input(
                                &format!("{tag}-{rotation}-{i}"),
                                &format!(
                                    "{title} {} {}",
                                    ["X", "Y", "Z"][i],
                                    if rotation { "rotation" } else { "translation" }
                                ),
                                value,
                                Command::Edit(id, field),
                                (28. + i as f32 * (w + 3.), y, w),
                                blocked,
                            )?;
                        }
                        y += 32.;
                    }
                    let error = transform.value(state.units).err();
                    p.text(&format!("{tag}-xyz"), "X · Y · Z", (28., y, 76., 28.), 8.);
                    p.button(
                        (
                            &format!("{tag}-apply"),
                            if definition {
                                "Apply component origin"
                            } else {
                                "Apply placement"
                            },
                            None,
                        ),
                        Command::ApplyTransform(id, definition),
                        (108., y, width - 116., 28.),
                        None,
                        (blocked || error.is_some(), None),
                        Field::None,
                    )?;
                    y += 34.;
                    if let Some(error) = error {
                        p.text(
                            &format!("{tag}-error"),
                            &error,
                            (8., y, width - 16., 44.),
                            10.,
                        );
                        y += 46.;
                    }
                    p.text(
                        &format!("{tag}-hint"),
                        if definition {
                            "Source parts stay in their authored coordinates."
                        } else {
                            "Placement is relative to the parent coordinate system."
                        },
                        (8., y, width - 16., 40.),
                        9.,
                    );
                    y += 48.;
                }
            }
        }
        p.text(
            "joints-label",
            "JOINTS",
            (8., y + 10., width - 90., 20.),
            10.,
        );
        p.button(
            ("new-joint", "Create joint", Some("+ Joint")),
            Command::Joint(joint::Command::Open(None)),
            (width - 74., y + 5., 66., 28.),
            None,
            (blocked, None),
            Field::None,
        )?;
        y += 42.;
        for j in &a.joints {
            p.button(
                (
                    &format!("joint-{}", j.id.0),
                    &format!("Joint {}", j.name),
                    Some(&j.name),
                ),
                Command::Motion(motion::Action::Select(j.id.0)),
                (8., y, width - 88., 28.),
                Some(Icon::Joint),
                (
                    blocked,
                    Some(motion::selected(&state.motion) == Some(j.id.0)),
                ),
                Field::None,
            )?;
            p.button(
                (
                    &format!("joint-{}-edit", j.id.0),
                    &format!("Edit joint {}", j.name),
                    Some(""),
                ),
                Command::Joint(joint::Command::Open(Some(j.id.0))),
                (width - 76., y, 22., 28.),
                Some(Icon::Pencil),
                (blocked, None),
                Field::None,
            )?;
            p.button(
                (
                    &format!("joint-{}-enabled", j.id.0),
                    &format!(
                        "{} joint {}",
                        if j.enabled { "Suppress" } else { "Unsuppress" },
                        j.name
                    ),
                    Some(""),
                ),
                Command::Joint(joint::Command::Enabled(j.id.0)),
                (width - 52., y, 22., 28.),
                Some(if j.enabled { Icon::Eye } else { Icon::EyeOff }),
                (blocked, None),
                Field::None,
            )?;
            p.button(
                (
                    &format!("joint-{}-delete", j.id.0),
                    &format!("Delete joint {}", j.name),
                    Some("×"),
                ),
                Command::Joint(joint::Command::Delete(j.id.0)),
                (width - 28., y, 22., 28.),
                None,
                (blocked, None),
                Field::None,
            )?;
            y += 32.;
        }
        if a.joints.is_empty() {
            p.text(
                "no-joints",
                "Create joints to relate component instances.",
                (12., y, width - 24., 36.),
                10.,
            );
            y += 40.;
        }
        motion::panel::paint(&mut p, &state.motion, &mut y, width, blocked)?;
    }
    state.max_scroll = (y - (height - 88.)).max(0.);
    state.scroll = state.scroll.min(state.max_scroll);
    p.scroll = 0.;
    p.top = top + height - 24.;
    p.bottom = top + height;
    for (key, label, command, x, disabled, icon) in [
        (
            "up",
            "Scroll assembly up",
            Command::Scroll(-160),
            width - 48.,
            state.scroll <= 0.,
            Icon::ArrowLeft,
        ),
        (
            "down",
            "Scroll assembly down",
            Command::Scroll(160),
            width - 24.,
            state.scroll >= state.max_scroll,
            Icon::ArrowRight,
        ),
    ] {
        if state.max_scroll > 0. {
            p.button(
                (key, label, Some("")),
                command,
                (x, 0., 22., 22.),
                Some(icon),
                (disabled, None),
                Field::None,
            )?;
        }
    }
    Ok(())
}
