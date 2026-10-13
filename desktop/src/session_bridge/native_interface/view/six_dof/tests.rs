use super::*;
use crate::session_bridge::native_interface::controller::six_dof::device_axes;
use crate::six_dof_mouse::motion_from_report;
use bevy::prelude::Quat;

/// Stands in for a SpaceMouse. It feeds the same HID reports the release host
/// decoded and never opens a device, driver, or connection worker.
struct DevicePacket {
    report: Vec<u8>,
}

impl DevicePacket {
    fn report(id: u8, axes: &[i16]) -> Self {
        let mut report = vec![id];
        report.extend(axes.iter().flat_map(|value| value.to_le_bytes()));
        Self { report }
    }

    fn motion(&self) -> Motion {
        let packet = motion_from_report(&self.report).expect("device motion report");
        Motion {
            translation: packet.translation.map(device_axes).unwrap_or([0.; 3]),
            rotation: packet.rotation.map(device_axes).unwrap_or([0.; 3]),
        }
    }
}

fn camera() -> ViewportCamera {
    ViewportCamera {
        position: [0., 0., 10.],
        target: [0., 0., 0.],
        up: [0., 1., 0.],
        vertical_fov_degrees: 15.2,
    }
}
fn close(a: Vec3, b: Vec3) {
    assert!((a - b).length() < 1e-4, "{a:?} != {b:?}");
}

#[test]
fn native_object_motion_matches_release_pan_dolly_and_speed_bounds() {
    let before = camera();
    let motion = Motion {
        translation: [1., 0., 1.],
        ..Default::default()
    };
    let moved = move_camera(before, Vec3::ZERO, motion, 1. / 60., 1.5).unwrap();
    close(
        Vec3::from_array(moved.position),
        Vec3::new(-0.225, -0.225, 10.),
    );
    close(
        Vec3::from_array(moved.target),
        Vec3::new(-0.225, -0.225, 0.),
    );
    assert_eq!(moved.up, before.up);
    let dolly = Motion {
        translation: [0., 1., 0.],
        ..Default::default()
    };
    let moved = move_camera(before, Vec3::ZERO, dolly, 1. / 60., 1.5).unwrap();
    assert!((moved.position[2] - 10. * 0.0225_f32.exp()).abs() < 1e-4);
    assert_eq!(moved.target, before.target);
    assert_eq!(
        move_camera(before, Vec3::ZERO, motion, 10., 20.).unwrap(),
        move_camera(before, Vec3::ZERO, motion, 0.05, 3.).unwrap()
    );
    assert_eq!(
        move_camera(before, Vec3::ZERO, motion, 0., 0.).unwrap(),
        move_camera(before, Vec3::ZERO, motion, 0.001, 0.25).unwrap()
    );
}

#[test]
fn rotation_turns_the_entire_camera_rig_around_the_visible_solid_without_recentering() {
    let mut before = camera();
    before.target = [2., 0., 0.];
    let pivot = Vec3::new(3., 4., 5.);
    let motion = Motion {
        rotation: [0.5, -0.2, 1.],
        ..Default::default()
    };
    let moved = move_camera(before, pivot, motion, 1. / 60., 1.5).unwrap();
    let distance = |point: [f32; 3]| Vec3::from_array(point).distance(pivot);
    assert!((distance(moved.position) - distance(before.position)).abs() < 1e-4);
    assert!((distance(moved.target) - distance(before.target)).abs() < 1e-4);
    let rig = |camera: ViewportCamera| {
        Vec3::from_array(camera.position).distance(Vec3::from_array(camera.target))
    };
    assert!((rig(moved) - rig(before)).abs() < 1e-4);
    assert_ne!(moved.target, before.target);
    assert!((Vec3::from_array(moved.up).length() - 1.).abs() < 1e-5);
    assert!(move_camera(before, pivot, motion, f32::NAN, 1.).is_err());
    assert!(move_camera(before, Vec3::NAN, motion, 1. / 60., 1.).is_err());
}

#[test]
fn device_packet_matches_release_pan_zoom_and_rotate() {
    let before = camera();
    let pivot = Vec3::ZERO;
    let seconds = 1. / 60.;
    let speed = 1.5;

    let pan = DevicePacket::report(1, &[350, 0, 0]).motion();
    assert_eq!(pan.translation, [1., 0., 0.]);
    assert_eq!(pan.rotation, [0., 0., 0.]);
    let moved = move_camera(before, pivot, pan, seconds, speed).unwrap();
    close(Vec3::from_array(moved.position), Vec3::new(-0.225, 0., 10.));
    close(Vec3::from_array(moved.target), Vec3::new(-0.225, 0., 0.));
    assert_eq!(moved.up, before.up);

    let zoom = DevicePacket::report(1, &[0, -350, 0]).motion();
    assert_eq!(zoom.translation, [0., 1., 0.]);
    let moved = move_camera(before, pivot, zoom, seconds, speed).unwrap();
    close(
        Vec3::from_array(moved.position),
        Vec3::new(0., 0., 10. * 0.0225_f32.exp()),
    );
    assert_eq!(moved.target, before.target);
    assert_eq!(moved.up, before.up);

    let rotate = DevicePacket::report(2, &[0, 0, 350]).motion();
    assert_eq!(rotate.rotation, [0., 0., -1.]);
    let moved = move_camera(before, pivot, rotate, seconds, speed).unwrap();
    let angle = 1.65 * seconds * speed;
    let turned = Quat::from_axis_angle(Vec3::Y, angle) * Vec3::new(0., 0., 10.);
    close(Vec3::from_array(moved.position), turned);
    close(Vec3::from_array(moved.target), Vec3::ZERO);
    close(Vec3::from_array(moved.up), Vec3::Y);
    assert!((Vec3::from_array(moved.position).length() - 10.).abs() < 1e-4);
}
