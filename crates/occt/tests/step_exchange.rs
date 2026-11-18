#![cfg(feature = "native-occt")]
//! STEP between the two kernels: a body of the Limo kernel is written,
//! read by Open CASCADE, written by Open CASCADE and read back. What
//! comes back has to be the solid that went out.

use limo_cad_core::{BodyId, FeatureId};
use limo_cad_kernel::demo::showcase;
use limo_cad_kernel::make::{self, PSeg};
use limo_cad_kernel::props::mass;
use limo_cad_kernel::step::{read, write};
use limo_cad_kernel::{boolean, v2, v3, Body, Op, V3};
use limo_cad_occt::OcctKernel;
use limo_cad_solid::{KernelImportStepJobDto, KernelJobDto, RecomputePlanDto, StepExportRequest};

fn base64(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for k in 0..4 {
            if k <= chunk.len() {
                out.push(DIGITS[((n >> (18 - 6 * k)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// What Open CASCADE makes of a body of the Limo kernel, as STEP again.
fn through_the_reference(name: &str, body: &Body) -> String {
    let text = write(&[(name.to_string(), body)], "exchange test");
    let mut kernel = OcctKernel::new().unwrap();
    assert!(
        !kernel.uses_limo_kernel(),
        "run this test without LIMO_CAD_KERNEL"
    );
    let scene = kernel
        .recompute(&RecomputePlanDto {
            transaction_id: 1,
            errors: vec![],
            jobs: vec![KernelJobDto::ImportStep(KernelImportStepJobDto {
                feature_id: FeatureId(1),
                result_body_id: BodyId(1),
                data_base64: base64(text.as_bytes()),
            })],
        })
        .unwrap();
    assert!(scene.errors.is_empty(), "{name}: {:?}", scene.errors);
    // What the reference shows is the solid that was written, to the
    // tolerance of its display mesh.
    let mut shown = 0.0;
    for found in &scene.bodies {
        let point = |i: u32| {
            let k = 3 * i as usize;
            [
                f64::from(found.positions[k]),
                f64::from(found.positions[k + 1]),
                f64::from(found.positions[k + 2]),
            ]
        };
        for t in found.indices.as_chunks::<3>().0 {
            let (a, b, c) = (point(t[0]), point(t[1]), point(t[2]));
            shown += (a[0] * (b[1] * c[2] - b[2] * c[1])
                + a[1] * (b[2] * c[0] - b[0] * c[2])
                + a[2] * (b[0] * c[1] - b[1] * c[0]))
                / 6.0;
        }
    }
    let exact = mass(body).volume;
    assert!(
        (shown - exact).abs() < 4.0e-2 * exact,
        "{name}: the reference shows {shown} for {exact}"
    );
    let back = kernel.export_step(&StepExportRequest::default()).unwrap();
    let back = String::from_utf8(back).unwrap();
    // `LIMO_CAD_STEP_KEEP=<dir>` keeps both files for a look.
    if let Some(dir) = std::env::var_os("LIMO_CAD_STEP_KEEP") {
        let dir = std::path::PathBuf::from(dir);
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join(format!("{name}-out.step")), &text);
        let _ = std::fs::write(dir.join(format!("{name}-back.step")), &back);
    }
    back
}

#[track_caller]
fn assert_exchanges(name: &str, body: &Body) {
    let back = read(&through_the_reference(name, body)).unwrap_or_else(|e| panic!("{name}: {e}"));
    let before = mass(body);
    let (mut volume, mut area) = (0.0, 0.0);
    for solid in &back {
        let m = mass(solid);
        volume += m.volume;
        area += m.area;
    }
    let close = |a: f64, b: f64| (a - b).abs() <= 1.0e-7 * a.abs().max(1.0);
    assert!(
        close(before.volume, volume) && close(before.area, area),
        "{name}: volume {} and area {} came back as {volume} and {area}",
        before.volume,
        before.area
    );
}

#[test]
fn solids_survive_the_reference_kernel() {
    assert_exchanges(
        "block",
        &make::block(v3(1.0, 2.0, 3.0), v3(11.0, 22.0, 33.0)).unwrap(),
    );
    assert_exchanges(
        "rod",
        &make::cylinder(v3(5.0, -3.0, 2.0), v3(1.0, 2.0, 2.0), 4.0, 10.0).unwrap(),
    );
    assert_exchanges(
        "cone",
        &make::lathe(V3::ZERO, V3::Z, &[(0.0, 0.0), (6.0, 9.0), (0.0, 9.0)]).unwrap(),
    );
    assert_exchanges(
        "cup",
        &make::lathe(
            v3(3.0, 4.0, 5.0),
            v3(0.0, 1.0, 1.0),
            &[
                (2.0, 0.0),
                (8.0, 0.0),
                (11.0, 9.0),
                (9.0, 9.0),
                (6.5, 1.5),
                (2.0, 1.5),
            ],
        )
        .unwrap(),
    );
    let half = [
        PSeg::Arc {
            a: v2(0.0, -7.5),
            b: v2(0.0, 7.5),
            c: v2(0.0, 0.0),
            ccw: true,
        },
        PSeg::Line {
            a: v2(0.0, 7.5),
            b: v2(0.0, -7.5),
        },
    ];
    let ball = make::revolve(
        &[half.to_vec()],
        V3::ZERO,
        V3::Z,
        V3::X,
        std::f64::consts::TAU,
    )
    .unwrap();
    assert_exchanges("ball", &ball);
    let tube = PSeg::Circle {
        c: v2(20.0, 0.0),
        r: 6.0,
    };
    let ring = make::revolve(&[vec![tube]], V3::ZERO, V3::Z, V3::X, std::f64::consts::TAU).unwrap();
    assert_exchanges("ring", &ring);
    // A plate with a bore: the bore has no seam on the way out and one on
    // the way back.
    let plate = make::block(V3::ZERO, v3(40.0, 30.0, 8.0)).unwrap();
    let bore = make::cylinder(v3(15.0, 12.0, -1.0), V3::Z, 5.0, 10.0).unwrap();
    assert_exchanges("drilled plate", &boolean(&plate, &bore, Op::Cut).unwrap());
}

#[test]
fn sample_parts_survive_the_reference_kernel() {
    use limo_cad_kernel::CurveKind;
    for (name, body) in showcase().unwrap() {
        let free = (0..body.edge_count() as u32).any(|e| {
            !matches!(
                body.e_curve_ref(e).kind,
                CurveKind::Line | CurveKind::Circle | CurveKind::Ellipse
            )
        });
        if free {
            // The reference reads the splines; its own file has them too,
            // and those are not read back here.
            let _ = through_the_reference(name, &body);
        } else {
            assert_exchanges(name, &body);
        }
    }
}

/// Volume and area of a body of the Limo kernel as Open CASCADE measures
/// them on the solid it reads from the STEP file: its own exact integrals,
/// not a mesh.
fn weighed_by_the_reference(name: &str, body: &Body) -> (f64, f64) {
    use limo_cad_occt::measurement::ResolvedMeasurementSelection;
    let text = write(&[(name.to_string(), body)], "exchange test");
    let mut kernel = OcctKernel::new().unwrap();
    let scene = kernel
        .recompute(&RecomputePlanDto {
            transaction_id: 1,
            errors: vec![],
            jobs: vec![KernelJobDto::ImportStep(KernelImportStepJobDto {
                feature_id: FeatureId(1),
                result_body_id: BodyId(1),
                data_base64: base64(text.as_bytes()),
            })],
        })
        .unwrap();
    assert!(scene.errors.is_empty(), "{name}: {:?}", scene.errors);
    assert_eq!(scene.bodies.len(), 1, "{name}");
    let found = kernel
        .detached_measurement_snapshot(&[BodyId(1)])
        .unwrap()
        .inspect(&[ResolvedMeasurementSelection {
            body_id: BodyId(1),
            kind: 0,
            index: 0,
            translation: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
        }])
        .unwrap();
    let whole = &found.selections[0];
    (whole.volume.unwrap(), whole.area.unwrap())
}

/// Threads go out as spline surfaces that follow the screw surfaces to a
/// few millionths of a millimetre. The reference kernel reads each as one
/// closed solid and weighs it with its own integrals.
///
/// Solids of planes, cylinders and tori come back to the last digit. On a
/// spline face the reference has to work out for itself where the edges
/// run, and its area is a few parts in a million off. Its volume is as
/// good while a face makes a turn or two; over the seven and a half turns
/// of the long thread it is two parts in ten thousand low, with the same
/// surfaces, the same area and nothing else changed.
#[test]
fn threads_weigh_the_same_in_the_reference_kernel() {
    use limo_cad_kernel::thread::{clipped, cutter, iso_external, rounded, Run};
    use limo_cad_solid::{iso_metric_grade6_envelope, ThreadFit};
    let run = |pitch: f64, depth: f64| Run {
        start: V3::ZERO,
        along: V3::Z,
        zero: V3::X,
        pitch,
        depth,
        left_hand: false,
    };
    // M8 x 1.25 - 6g along ten millimetres of a pin.
    let limits = iso_metric_grade6_envelope(8.0, 1.25, ThreadFit::External).unwrap();
    let pin = make::cylinder(V3::ZERO, V3::Z, 0.5 * limits.modeled_major, 14.0).unwrap();
    let profile = iso_external(
        limits.modeled_major,
        limits.modeled_pitch,
        limits.modeled_minor,
        1.25,
    )
    .unwrap();
    let groove = cutter(&profile, &run(1.25, 10.0)).unwrap();
    let groove = clipped(&groove, &run(1.25, 10.0), 0.5 * limits.modeled_major, true).unwrap();
    let metric = boolean(&pin, &groove, Op::Cut).unwrap();
    // A rounded 30 degree thread, 24 across with a pitch of 4, over a turn
    // and a half and over seven and a half.
    let rounded_rod = |length: f64| {
        let rod = make::cylinder(V3::ZERO, V3::Z, 12.0, length).unwrap();
        let profile = rounded(24.0, 20.0, 4.0, 0.3, 0.0, false).unwrap();
        let groove = cutter(&profile, &run(4.0, length)).unwrap();
        let groove = clipped(&groove, &run(4.0, length), 12.0, true).unwrap();
        boolean(&rod, &groove, Op::Cut).unwrap()
    };
    let (short, long) = (rounded_rod(6.0), rounded_rod(30.0));
    let ring = make::revolve(
        &[vec![PSeg::Circle {
            c: v2(12.0, 0.0),
            r: 3.0,
        }]],
        V3::ZERO,
        V3::Z,
        V3::X,
        std::f64::consts::TAU,
    )
    .unwrap();
    for (name, body, volume_near, area_near) in [
        ("ring", &ring, 1.0e-12, 1.0e-12),
        ("plain pin", &pin, 1.0e-12, 1.0e-12),
        ("metric thread", &metric, 3.0e-5, 3.0e-5),
        ("short rounded thread", &short, 3.0e-5, 3.0e-5),
        ("long rounded thread", &long, 1.0e-3, 3.0e-5),
    ] {
        let exact = mass(body);
        let (volume, area) = weighed_by_the_reference(name, body);
        assert!(
            (volume - exact.volume).abs() < volume_near * exact.volume,
            "{name}: volume {volume} for {}",
            exact.volume
        );
        assert!(
            (area - exact.area).abs() < area_near * exact.area,
            "{name}: area {area} for {}",
            exact.area
        );
    }
}

/// A thread goes out as spline surfaces; the reference kernel reads it
/// as the same solid.
#[test]
fn a_threaded_rod_reaches_the_reference_kernel() {
    let pitch = 2.0;
    let rod = make::cylinder(V3::ZERO, V3::Z, 5.0, 9.0).unwrap();
    let points = [v2(4.0, -0.3), v2(5.2, -0.9), v2(5.2, 0.9), v2(4.0, 0.3)];
    let tooth: Vec<PSeg> = (0..4)
        .map(|i| PSeg::Line {
            a: points[i],
            b: points[(i + 1) % 4],
        })
        .collect();
    let groove = make::screw(
        &tooth,
        V3::ZERO,
        V3::Z,
        V3::X,
        pitch / std::f64::consts::TAU,
        -std::f64::consts::TAU,
        std::f64::consts::TAU * (9.0 / pitch + 1.0),
    )
    .unwrap();
    let threaded = boolean(&rod, &groove, Op::Cut).unwrap();
    let _ = through_the_reference("threaded rod", &threaded);
}

/// Free-form solids go out as the splines they are made of: the same
/// degrees, knots, weights and control points. The reference kernel reads
/// each as one closed solid and weighs it as this kernel does.
#[test]
fn free_form_solids_weigh_the_same_in_the_reference_kernel() {
    use limo_cad_kernel::loft::skin;
    use limo_cad_kernel::make::Seg;
    use std::f64::consts::PI;
    let square = |half: f64, z: f64, turn: f64| -> Vec<Seg> {
        let p: Vec<V3> = (0..4)
            .map(|i| {
                let a = turn + PI / 4.0 + PI / 2.0 * i as f64;
                let r = half * 2f64.sqrt();
                v3(r * a.cos(), r * a.sin(), z)
            })
            .collect();
        (0..4)
            .map(|i| Seg::Line {
                a: p[i],
                b: p[(i + 1) % 4],
            })
            .collect()
    };
    let circle = |r: f64, z: f64| {
        vec![Seg::Circle {
            c: v3(0.0, 0.0, z),
            p: v3(r, 0.0, z),
            n: V3::Z,
        }]
    };
    let prism = skin(&[square(10.0, 0.0, 0.0), square(6.0, 25.0, 0.5)], true).unwrap();
    let funnel = skin(&[square(10.0, 0.0, 0.0), circle(8.0, 30.0)], true).unwrap();
    let vase = skin(
        &[circle(8.0, 0.0), circle(12.0, 15.0), circle(6.0, 30.0)],
        false,
    )
    .unwrap();
    let rod = make::cylinder(v3(-25.0, -3.0, 2.0), v3(1.0, 0.2, 0.4).unit(), 2.5, 70.0).unwrap();
    let drilled = boolean(&vase, &rod, Op::Cut).unwrap();
    let image = vase.transformed(&limo_cad_kernel::Xform::mirror(
        v3(3.0, 0.0, 0.0),
        v3(1.0, 0.1, 0.2).unit(),
    ));
    for (name, body) in [
        ("twisted prism", &prism),
        ("square to round", &funnel),
        ("vase", &vase),
        ("drilled vase", &drilled),
        ("vase in a mirror", &image),
    ] {
        let exact = mass(body);
        let (volume, area) = weighed_by_the_reference(name, body);
        assert!(
            (volume - exact.volume).abs() < 1.0e-9 * exact.volume,
            "{name}: volume {volume} for {}",
            exact.volume
        );
        assert!(
            (area - exact.area).abs() < 1.0e-8 * exact.area,
            "{name}: area {area} for {}",
            exact.area
        );
    }
}
