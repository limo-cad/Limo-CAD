use super::*;
use interface_shell::fields::limits::{self, ByteLimit};

const SOURCE: &str = "G21 G90\nT1 M6\nG0 X2 Y2 Z3\nG1 Z-1 F120\nM30\n";

fn fixture() -> (World, Entity, DocumentContext, u64) {
    let owner = DocumentContext {
        window_id: "nc-test-window".into(),
        document_id: "nc-test-document".into(),
        epoch: 3,
    };
    let revision = 17;
    let mut world = World::new();
    world.init_resource::<ViewportUiAssets>();
    let camera = world.spawn(InterfaceCamera).id();
    let mut document = super::super::tests::job();
    document.setups[0].operations.clear();
    world.insert_resource(State {
        key: Some(Key {
            owner: owner.clone(),
            revision,
            selection: None,
        }),
        setup: Some(1),
        document: Some(Arc::new(document)),
        ..default()
    });
    reduce(
        &mut world,
        &owner,
        revision,
        0,
        Command::Open,
        &ControlInput::Click,
    )
    .unwrap();
    synchronize(&mut world, camera, 1360., 860.).unwrap();
    (world, camera, owner, revision)
}

fn command(
    world: &mut World,
    owner: &DocumentContext,
    revision: u64,
    command: Command,
    value: Option<&str>,
) -> Result<Value, String> {
    let serial = world.resource::<Editor>().serial;
    let input = value.map_or(ControlInput::Click, |value| {
        ControlInput::SetValue(value.into())
    });
    reduce(world, owner, revision, serial, command, &input)
}

fn source_entity(world: &World) -> Entity {
    world
        .resource::<Editor>()
        .widgets
        .entity("nc-source")
        .unwrap()
}

#[test]
fn oversized_source_blocks_rename_and_run_until_an_explicit_valid_replacement() {
    let (mut world, _, owner, revision) = fixture();
    let before = serde_json::to_value(world.resource::<State>().document.as_deref()).unwrap();
    command(&mut world, &owner, revision, Command::Source, Some(SOURCE)).unwrap();
    let oversized = "\u{e9}".repeat(limo_cad_cam::MAX_GCODE_BYTES / 2) + "x";
    assert_eq!(oversized.len(), limo_cad_cam::MAX_GCODE_BYTES + 1);
    let error = command(
        &mut world,
        &owner,
        revision,
        Command::Source,
        Some(&oversized),
    )
    .unwrap_err();
    assert!(error.contains("8388608 bytes"));
    assert_eq!(
        limits::error(&world, source_entity(&world)),
        Some(error.as_str()),
        "A failed direct replacement must fence any older native dirty buffer"
    );
    assert_eq!(world.resource::<Editor>().input.source, SOURCE);
    command(
        &mut world,
        &owner,
        revision,
        Command::FileName,
        Some("renamed.nc"),
    )
    .unwrap();
    command(&mut world, &owner, revision, Command::Dialect, Some("iso")).unwrap();
    assert_eq!(
        command(&mut world, &owner, revision, Command::Run, None).unwrap_err(),
        error
    );
    assert!(world.resource::<State>().nc_input.is_none());
    assert!(!world.resource::<State>().request_pending);
    assert!(caption(&world).unwrap().contains(&error));

    let replacement = SOURCE.replace("X2", "X4");
    command(
        &mut world,
        &owner,
        revision,
        Command::Source,
        Some(&replacement),
    )
    .unwrap();
    assert!(world.resource::<Editor>().source_error.is_none());
    assert!(limits::error(&world, source_entity(&world)).is_none());
    command(&mut world, &owner, revision, Command::Run, None).unwrap();
    let queued = world.resource::<State>().nc_input.as_ref().unwrap();
    assert_eq!(queued.source, replacement);
    assert_eq!(queued.file_name.as_deref(), Some("renamed.nc"));
    assert_eq!(queued.dialect, CamGcodeDialectDto::Iso);
    assert_eq!(
        serde_json::to_value(world.resource::<State>().document.as_deref()).unwrap(),
        before
    );
}

#[test]
fn native_limit_marker_blocks_run_and_cancel_reopen_retires_it_without_losing_accepted_source() {
    let (mut world, camera, owner, revision) = fixture();
    command(&mut world, &owner, revision, Command::Source, Some(SOURCE)).unwrap();
    let source = source_entity(&world);
    assert_eq!(
        world.get::<ByteLimit>(source).unwrap().maximum,
        limo_cad_cam::MAX_GCODE_BYTES
    );
    let message = "Source text exceeds 8388608 bytes; the edit was not inserted";
    world.get_mut::<ByteLimit>(source).unwrap().rejected = Some(message.into());
    synchronize(&mut world, camera, 1360., 860.).unwrap();
    assert!(caption(&world).unwrap().contains(message));
    assert_eq!(
        world.resource::<Editor>().limit_error.as_deref(),
        Some(message)
    );
    command(
        &mut world,
        &owner,
        revision,
        Command::FileName,
        Some("still-blocked.nc"),
    )
    .unwrap();
    assert_eq!(
        command(&mut world, &owner, revision, Command::Run, None).unwrap_err(),
        message
    );
    assert!(world.resource::<State>().nc_input.is_none());
    command(&mut world, &owner, revision, Command::Source, Some(SOURCE)).unwrap();
    assert!(limits::error(&world, source).is_none());
    assert!(world.resource::<Editor>().limit_error.is_none());
    world.get_mut::<ByteLimit>(source).unwrap().rejected = Some(message.into());
    let serial = world.resource::<Editor>().serial;
    command(&mut world, &owner, revision, Command::Close, None).unwrap();
    synchronize(&mut world, camera, 1360., 860.).unwrap();
    assert!(world.get_entity(source).is_err());
    command(&mut world, &owner, revision, Command::Open, None).unwrap();
    synchronize(&mut world, camera, 1360., 860.).unwrap();
    assert!(world.resource::<Editor>().serial > serial);
    assert_eq!(world.resource::<Editor>().input.source, SOURCE);
    assert!(limits::error(&world, source_entity(&world)).is_none());
    assert!(!caption(&world).unwrap().contains(message));
    command(&mut world, &owner, revision, Command::Run, None).unwrap();
    assert_eq!(
        world.resource::<State>().nc_input.as_ref().unwrap().source,
        SOURCE
    );
}

#[test]
fn retired_generation_revision_and_owner_cannot_replace_or_run_nc_source() {
    let (mut world, camera, owner, revision) = fixture();
    command(&mut world, &owner, revision, Command::Source, Some(SOURCE)).unwrap();
    let old_serial = world.resource::<Editor>().serial;
    command(&mut world, &owner, revision, Command::Close, None).unwrap();
    synchronize(&mut world, camera, 1360., 860.).unwrap();
    command(&mut world, &owner, revision, Command::Open, None).unwrap();
    for (command, input) in [
        (Command::Source, ControlInput::SetValue("stale".into())),
        (Command::Run, ControlInput::Click),
    ] {
        assert!(reduce(&mut world, &owner, revision, old_serial, command, &input).is_err());
    }
    let serial = world.resource::<Editor>().serial;
    assert!(reduce(
        &mut world,
        &owner,
        revision + 1,
        serial,
        Command::Run,
        &ControlInput::Click
    )
    .is_err());
    let successor = DocumentContext {
        epoch: owner.epoch + 1,
        ..owner.clone()
    };
    world.resource_mut::<State>().key.as_mut().unwrap().owner = successor.clone();
    assert!(reduce(
        &mut world,
        &owner,
        revision,
        serial,
        Command::Run,
        &ControlInput::Click
    )
    .is_err());
    assert!(reduce(
        &mut world,
        &successor,
        revision,
        serial,
        Command::Source,
        &ControlInput::SetValue("replacement".into())
    )
    .is_err());
    assert_eq!(world.resource::<Editor>().input.source, SOURCE);
    assert!(world.resource::<State>().nc_input.is_none());
    synchronize(&mut world, camera, 1360., 860.).unwrap();
    assert!(!modal(&world));
}

#[test]
fn failed_file_replacement_blocks_old_source_and_a_valid_file_repairs_both_rejections() {
    let (mut world, camera, owner, revision) = fixture();
    command(&mut world, &owner, revision, Command::Source, Some(SOURCE)).unwrap();
    let complete = |world: &mut World, result| {
        let (send, receive) = mpsc::channel();
        send.send(result).unwrap();
        let mut editor = world.resource_mut::<Editor>();
        editor.picker = Some(Picker {
            key: editor.key.clone().unwrap(),
            serial: editor.serial,
            receiver: Mutex::new(receive),
        });
    };
    complete(
        &mut world,
        Err("NC source must contain valid UTF-8 text".into()),
    );
    synchronize(&mut world, camera, 1360., 860.).unwrap();
    assert_eq!(
        limits::error(&world, source_entity(&world)),
        Some("NC source must contain valid UTF-8 text")
    );
    command(
        &mut world,
        &owner,
        revision,
        Command::FileName,
        Some("new-name.nc"),
    )
    .unwrap();
    assert!(command(&mut world, &owner, revision, Command::Run, None)
        .unwrap_err()
        .contains("UTF-8"));
    assert_eq!(world.resource::<Editor>().input.source, SOURCE);
    command(&mut world, &owner, revision, Command::Dialect, Some("haas")).unwrap();
    let source = source_entity(&world);
    world.get_mut::<ByteLimit>(source).unwrap().rejected =
        Some("Previous native input was too large".into());
    let replacement = SOURCE.replace("X2", "X5");
    complete(
        &mut world,
        Ok(Some(nc_input::Input {
            source: replacement.clone(),
            file_name: Some("loaded.nc".into()),
            dialect: CamGcodeDialectDto::Auto,
        })),
    );
    synchronize(&mut world, camera, 1360., 860.).unwrap();
    assert!(world.resource::<Editor>().source_error.is_none());
    assert!(limits::error(&world, source).is_none());
    command(&mut world, &owner, revision, Command::Run, None).unwrap();
    let input = world.resource::<State>().nc_input.as_ref().unwrap();
    assert_eq!(input.source, replacement);
    assert_eq!(input.file_name.as_deref(), Some("loaded.nc"));
    assert_eq!(
        input.dialect,
        CamGcodeDialectDto::Haas,
        "File picking preserves the explicit controller choice"
    );
}
