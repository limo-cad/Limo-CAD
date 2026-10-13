use super::*;
use limo_cad_cam::{CamMachineAssignmentDto, CamPostConfigDto, PostDialect};
use std::sync::Arc;

const SOURCE: &str = "/native/machine/source";
const SECTION: &str = "/native/ui/setup_section";
const RETRACT: &str = "/native/machine/post/machine_retract_z";
const SUPA: &str = "/native/machine/post/siemens_828d/supa_retract_z";

fn draft(cam: &CamDocumentDto, private: machine::Snapshot) -> Draft {
    let mut draft = Draft::new(cam, Selection::Setup(3)).unwrap();
    machine::extend(&mut draft, cam, Arc::new(private)).unwrap();
    draft
}
fn edit(draft: &mut Draft, cam: &CamDocumentDto, path: &str, value: &str) {
    if let Some(options) = draft
        .fields
        .iter()
        .find(|field| field.path == path)
        .and_then(|field| field.options.as_ref())
    {
        assert_eq!(
            choose(
                options,
                form::text(draft, path).unwrap(),
                &ControlInput::SetValue(value.into())
            )
            .unwrap(),
            value
        );
    }
    set(draft, path, value);
    machine::changed(draft, cam.units, path).unwrap();
}
fn detailed_machine() -> CamMachineAssignmentDto {
    let post: CamPostConfigDto = serde_json::from_value(json!({
        "dialect":"siemens828d","program_number":7321,"sequence_numbers":false,
        "tool_call_mode":"name","machine_retract_z":0.1,
        "siemens_828d":{"atc_style":"carousel_chain","tool_change_positioning":"supa_z_then_xy",
            "supa_retract_z":-0.1,"station_x":0.1,"station_y":-0.3,
            "tool_length_offset":17,"optional_stop_on_tool_change":true,"preload_next_tool":true,
            "spindle_stop_subprogram":"SHOP_STOP"}
    }))
    .unwrap();
    let mut machine = CamMachineAssignmentDto::three_axis(post);
    machine.profile.id = "private-shop-machine".into();
    machine.profile.name = "Retained shop machine".into();
    machine.profile.revision = 17;
    machine.profile.controller.software_version = Some("shop-controller-4.2".into());
    machine.profile.axes[0].origin.x = f64::from_bits(0x3fb999999999999b);
    machine.profile.axes[0].limits = Some([-321.1234567890123, 123.9876543210987]);
    machine.tool_calls = serde_json::from_value(json!([
        {"tool_id":5,"call":{"kind":"name","name":"LEGACY_FACE"}},
        {"tool_id":99,"call":{"kind":"number","number":42}}
    ]))
    .unwrap();
    machine.validate().unwrap();
    machine
}

#[test]
fn native_cam_machine_unedited_and_unrelated_edits_preserve_complete_snapshot() {
    let mut cam = job();
    cam.units = CamUnits::Inches;
    cam.setups[0].machine = Some(detailed_machine());
    cam.validate_for_editing().unwrap();
    let mut draft = draft(&cam, machine::Snapshot::default());
    assert!(!draft.dirty());
    assert_eq!(draft.edited(&cam).unwrap(), cam);
    set(&mut draft, "/name", "Renamed setup only");
    let mut expected = cam.clone();
    expected.setups[0].name = "Renamed setup only".into();
    assert_eq!(draft.edited(&cam).unwrap(), expected);

    let mut draft = self::draft(&cam, machine::Snapshot::default());
    edit(
        &mut draft,
        &cam,
        "/native/machine/post/sequence_numbers",
        "true",
    );
    let next = draft.edited(&cam).unwrap();
    let mut expected = cam.clone();
    let machine = expected.setups[0].machine.as_mut().unwrap();
    machine.profile.post.sequence_numbers = true;
    machine.profile.revision += 1;
    assert_eq!(next, expected);
    let original = cam.setups[0].machine.as_ref().unwrap();
    let actual = next.setups[0].machine.as_ref().unwrap();
    let original_siemens = original.profile.post.siemens_828d.as_ref().unwrap();
    let actual_siemens = actual.profile.post.siemens_828d.as_ref().unwrap();
    assert_eq!(
        actual_siemens.supa_retract_z.to_bits(),
        original_siemens.supa_retract_z.to_bits()
    );
    assert_eq!(
        actual_siemens.station_x.unwrap().to_bits(),
        original_siemens.station_x.unwrap().to_bits()
    );
    assert_eq!(
        actual_siemens.station_y.unwrap().to_bits(),
        original_siemens.station_y.unwrap().to_bits()
    );
    assert_eq!(
        actual.profile.post.machine_retract_z.unwrap().to_bits(),
        original.profile.post.machine_retract_z.unwrap().to_bits()
    );
    assert_eq!(
        actual.profile.axes[0].origin.x.to_bits(),
        original.profile.axes[0].origin.x.to_bits()
    );
    assert_eq!(actual.tool_calls, original.tool_calls);
}

#[test]
fn native_cam_machine_starters_are_explicit_and_unknown_coordinates_stay_unknown() {
    let mut cam = job();
    cam.units = CamUnits::Inches;
    cam.setups[0].machine = None;
    let mut draft = draft(&cam, machine::Snapshot::default());
    assert_eq!(form::text(&draft, SOURCE).unwrap(), "generic");
    assert!(!draft.dirty());
    assert_eq!(draft.edited(&cam).unwrap(), cam);
    edit(&mut draft, &cam, SOURCE, "starter:fanuc");
    assert_eq!(form::text(&draft, RETRACT).unwrap(), "");
    let unknown = draft.edited(&cam).unwrap();
    let machine = unknown.setups[0].machine.as_ref().unwrap();
    assert_eq!(machine.profile.post.dialect, PostDialect::Fanuc);
    assert!(machine.profile.post.machine_retract_z.is_none());
    assert!(
        machine.ensure_post_matches(&machine.profile.post).is_err(),
        "A stored starter must not certify an unknown machine retract"
    );
    let identity = machine.profile.id.clone();
    assert!(!identity.is_empty());
    edit(&mut draft, &cam, RETRACT, "-0.25");
    let entered = draft.edited(&cam).unwrap();
    let machine = entered.setups[0].machine.as_ref().unwrap();
    assert_eq!(machine.profile.id, identity);
    assert_eq!(machine.profile.post.machine_retract_z, Some(-6.35));
    machine.ensure_post_matches(&machine.profile.post).unwrap();
    assert_eq!(entered.setups[0].operations, cam.setups[0].operations);
    assert_eq!(entered.height_expressions, cam.height_expressions);
    assert_eq!(entered.tools, cam.tools);
    edit(&mut draft, &cam, RETRACT, "0");
    assert_eq!(
        draft.edited(&cam).unwrap().setups[0]
            .machine
            .as_ref()
            .unwrap()
            .profile
            .post
            .machine_retract_z,
        Some(0.)
    );
    edit(&mut draft, &cam, RETRACT, "");
    assert!(draft.edited(&cam).unwrap().setups[0]
        .machine
        .as_ref()
        .unwrap()
        .profile
        .post
        .machine_retract_z
        .is_none());

    let mut remove = self::draft(&entered, machine::Snapshot::default());
    edit(&mut remove, &entered, SOURCE, "generic");
    let mut expected = entered.clone();
    expected.setups[0].machine = None;
    assert_eq!(remove.edited(&entered).unwrap(), expected);
}

#[test]
fn native_cam_siemens_starter_requires_explicit_supa_and_converts_coordinates_once() {
    let mut cam = job();
    cam.units = CamUnits::Inches;
    cam.setups[0].machine = None;
    let mut draft = draft(&cam, machine::Snapshot::default());
    edit(&mut draft, &cam, SOURCE, "starter:siemens828d");
    assert_eq!(form::text(&draft, SUPA).unwrap(), "");
    assert!(
        draft.edited(&cam).is_err(),
        "The starter's placeholder zero is not an entered SUPA coordinate"
    );
    edit(&mut draft, &cam, SUPA, "-0.5");
    let next = draft.edited(&cam).unwrap();
    let post = next.setups[0]
        .machine
        .as_ref()
        .unwrap()
        .profile
        .post
        .siemens_828d
        .as_ref()
        .unwrap();
    assert_eq!(post.supa_retract_z, -12.7);
    assert_eq!(post.station_x, None);
    assert_eq!(post.station_y, None);
    edit(&mut draft, &cam, SECTION, "machine");
    let station_x = "/native/machine/post/siemens_828d/station_x";
    let station_y = "/native/machine/post/siemens_828d/station_y";
    assert!(!machine::visible(&draft, station_x));
    edit(
        &mut draft,
        &cam,
        "/native/machine/post/siemens_828d/tool_change_positioning",
        "supa_z_then_xy",
    );
    assert!(machine::visible(&draft, station_x));
    assert!(machine::visible(&draft, station_y));
    edit(&mut draft, &cam, station_x, "0.125");
    edit(&mut draft, &cam, station_y, "0");
    edit(
        &mut draft,
        &cam,
        "/native/machine/post/siemens_828d/spindle_stop_subprogram",
        "SHOP_STOP",
    );
    let next = draft.edited(&cam).unwrap();
    let machine = next.setups[0].machine.as_ref().unwrap();
    let post = machine.profile.post.siemens_828d.as_ref().unwrap();
    assert_eq!(post.supa_retract_z, -12.7);
    assert_eq!(post.station_x, Some(3.175));
    assert_eq!(post.station_y, Some(0.));
    assert_eq!(machine.profile.schema_version, 2);
    assert_eq!(post.spindle_stop_subprogram.as_deref(), Some("SHOP_STOP"));
    machine.ensure_post_matches(&machine.profile.post).unwrap();
}

#[test]
fn native_cam_private_machine_selection_is_pinned_across_catalog_refresh() {
    let mut cam = job();
    cam.setups[0].machine = None;
    let old = detailed_machine();
    let mut draft = draft(
        &cam,
        machine::Snapshot {
            profiles: vec![("shop.limo-post.json".into(), old.clone())],
            notice: String::new(),
        },
    );
    edit(&mut draft, &cam, SOURCE, "private:shop.limo-post.json");
    edit(
        &mut draft,
        &cam,
        "/native/machine/post/program_number",
        "8456",
    );
    let mut replacement = old.clone();
    replacement.profile.id = "different-catalog-machine".into();
    replacement.profile.name = "Catalog replacement".into();
    replacement.profile.revision = 99;
    replacement
        .profile
        .post
        .siemens_828d
        .as_mut()
        .unwrap()
        .supa_retract_z = -300.;
    machine::refresh_library(
        &mut draft,
        &cam,
        Arc::new(machine::Snapshot {
            profiles: vec![("shop.limo-post.json".into(), replacement)],
            notice: String::new(),
        }),
    );
    machine::refresh_library(&mut draft, &cam, Arc::new(machine::Snapshot::default()));
    assert_eq!(
        form::text(&draft, SOURCE).unwrap(),
        "private:shop.limo-post.json"
    );
    assert_eq!(
        form::text(&draft, "/native/machine/post/program_number").unwrap(),
        "8456"
    );
    let mut expected = old;
    expected.profile.post.program_number = Some(8456);
    expected.profile.revision += 1;
    let next = draft.edited(&cam).unwrap();
    assert_eq!(next.setups[0].machine.as_ref(), Some(&expected));
    assert_eq!(next.tools, cam.tools);
    assert_eq!(next.setups[0].operations, cam.setups[0].operations);
}

#[test]
fn native_cam_machine_invalid_fields_leave_the_shared_document_unchanged() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let mut cam = job();
    cam.setups[0].machine = Some(detailed_machine());
    parse_engine_envelope(
        fixture
            .engine
            .engine_call("cam_set_document", &serde_json::to_string(&cam).unwrap()),
    )
    .unwrap();
    let before = export(&fixture);
    for (path, value) in [
        ("/native/machine/name", " "),
        ("/native/machine/post/program_number", "-1"),
        ("/native/machine/post/program_number", "4294967296"),
        ("/native/machine/post/program_number", "1.5"),
        (RETRACT, "NaN"),
        (SUPA, "inf"),
        ("/native/machine/post/siemens_828d/tool_length_offset", "0"),
        (
            "/native/machine/post/siemens_828d/spindle_stop_subprogram",
            "M5\nG0Z0",
        ),
        ("/native/machine/post/sequence_numbers", "perhaps"),
        (SOURCE, "private:missing.json"),
    ] {
        let mut draft = draft(&cam, machine::Snapshot::default());
        set(&mut draft, path, value);
        assert!(
            draft.edited(&cam).is_err(),
            "Accepted invalid machine field {path} = {value:?}"
        );
        assert_eq!(fixture.engine.cam_document_snapshot(), cam);
        assert_eq!(export(&fixture), before);
    }
}

#[test]
fn native_cam_machine_section_navigation_is_clean_and_retained() {
    let cam = job();
    let mut draft = draft(&cam, machine::Snapshot::default());
    for section in ["machine", "setup", "machine"] {
        edit(&mut draft, &cam, SECTION, section);
        assert!(!draft.dirty());
        assert_eq!(draft.edited(&cam).unwrap(), cam);
        assert!(machine::visible(&draft, SECTION));
        assert_eq!(machine::visible(&draft, "/name"), section == "setup");
        assert_eq!(machine::visible(&draft, SOURCE), section == "machine");
    }
    let mut reopened = self::draft(&cam, machine::Snapshot::default());
    machine::retain_section(Some(&draft), &mut reopened);
    assert_eq!(form::text(&reopened, SECTION).unwrap(), "machine");
    assert!(!reopened.dirty());
    assert_eq!(reopened.edited(&cam).unwrap(), cam);
}
