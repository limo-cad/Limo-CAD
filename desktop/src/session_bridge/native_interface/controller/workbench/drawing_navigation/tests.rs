use super::*;

fn owner() -> DocumentContext {
    DocumentContext {
        window_id: "main".into(),
        document_id: "drawing".into(),
        epoch: 7,
    }
}
fn pane() -> Pane {
    Pane {
        bounds: Rect {
            x: 100.,
            y: 80.,
            width: 640.,
            height: 480.,
        },
        padding: [32.; 4],
    }
}
fn state() -> Navigation {
    Navigation::new(owner(), 3, [300., 200.], pane()).unwrap()
}
fn close(a: [f64; 2], b: [f64; 2]) {
    for i in 0..2 {
        assert!((a[i] - b[i]).abs() < 1e-8, "{a:?} != {b:?}");
    }
}
fn wheel(dpi: f64) -> Wheel {
    Wheel {
        delta: [0., 30. * dpi],
        unit: WheelUnit::Pixel,
        window_scale: dpi,
        ctrl: true,
        alt: false,
        macos: true,
        now_ms: 1000.,
    }
}

#[test]
fn fit_uses_the_existing_pane_padding_and_keeps_sheet_millimetres_unchanged() {
    let mut nav = state();
    assert!((nav.zoom - 0.64).abs() < 1e-9);
    assert!(nav.fitted);
    close(nav.transform().origin, [132., 112.]);
    close(nav.transform().sheet_mm, [300., 200.]);
    nav.zoom_at(4., None);
    assert!(nav.begin_pan(&owner(), 3, [420., 320.]));
    nav.pan_to(&owner(), 3, [50., 40.]);
    nav.fit();
    assert!(nav.pan.is_none());
    close(nav.scroll, [0.; 2]);
    close(nav.transform().origin, [132., 112.]);
    assert_eq!(nav.transform().visible_paper(), Some([0., 0., 300., 200.]));
}

#[test]
fn ctrl_wheel_and_pinch_anchor_the_same_paper_point_at_both_dpi_scales() {
    let cursor = [420., 320.];
    let mut expected = None;
    for dpi in [1., 2.] {
        let mut nav = state();
        let paper = nav.transform().pick(cursor).unwrap();
        let fitted = nav.zoom;
        assert!(nav.wheel(&owner(), 3, cursor, wheel(dpi)));
        assert!((nav.zoom - fitted * 0.21f64.exp()).abs() < 1e-9);
        close(nav.transform().to_screen(paper), cursor);
        close(nav.transform().pick(cursor).unwrap(), paper);
        assert!(!nav.fitted);
        if let Some(transform) = expected {
            assert_eq!(nav.transform(), transform);
        }
        expected = Some(nav.transform());
        assert!(nav.pinch(&owner(), 3, cursor, 0.2));
        close(nav.transform().to_screen(paper), cursor);
        close(nav.transform().to_paper(cursor), paper);
    }
}

#[test]
fn middle_pan_crosses_pane_bounds_without_changing_paper_scale_and_rejects_retired_owners() {
    let mut nav = state();
    nav.zoom_at(2., None);
    let zoom = nav.zoom;
    let prior = nav.transform();
    assert!(nav.begin_pan(&owner(), 3, [420., 320.]));
    assert!(nav.pan_to(&owner(), 3, [350., 270.]));
    close(
        nav.transform().origin,
        [prior.origin[0] - 70., prior.origin[1] - 50.],
    );
    assert_eq!(nav.zoom, zoom);
    assert!(nav.pan_to(&owner(), 3, [-50., -50.]));
    assert!(nav.cancel());
    assert!(!nav.pan_to(&owner(), 3, [440., 330.]));
    let mut stale = owner();
    stale.epoch -= 1;
    let before = nav.transform();
    assert!(!nav.begin_pan(&stale, 3, [420., 320.]));
    assert!(!nav.wheel(&stale, 3, [420., 320.], wheel(1.)));
    assert!(!nav.pinch(&owner(), 999, [420., 320.], 0.2));
    assert_eq!(nav.transform(), before);
    assert!(nav.begin_pan(&owner(), 3, [420., 320.]));
    assert!(!nav.pan_to(&stale, 3, [450., 350.]));
    assert!(nav.pan.is_none());
}

#[test]
fn trackpad_pan_and_explicit_option_zoom_follow_react_without_dpi_dependent_speed() {
    let cursor = [420., 320.];
    let mut expected = None;
    for dpi in [1., 2.] {
        let mut nav = state();
        nav.zoom_at(2., None);
        let before = nav.scroll;
        let input = Wheel {
            delta: [-20. * dpi, -30. * dpi],
            ctrl: false,
            ..wheel(dpi)
        };
        nav.wheel(&owner(), 3, cursor, input);
        close(nav.scroll, [before[0] + 20., before[1] + 30.]);
        assert_eq!(nav.zoom, 2.);
        if let Some(transform) = expected {
            assert_eq!(nav.transform(), transform);
        }
        expected = Some(nav.transform());
        let zoom = nav.zoom;
        nav.wheel(
            &owner(),
            3,
            cursor,
            Wheel {
                ctrl: false,
                alt: true,
                ..wheel(dpi)
            },
        );
        assert!((nav.zoom - zoom * 0.06f64.exp()).abs() < 1e-9);
    }
    let mut classifier = WheelGesture::default();
    assert!(!classifier.pans(WheelUnit::Line, [0., 16.], 1000.));
    assert!(classifier.pans(WheelUnit::Pixel, [0., 1.5], 1010.));
    assert!(classifier.pans(WheelUnit::Pixel, [0., 120.], 1020.));
    assert!(!classifier.pans(WheelUnit::Pixel, [0., 120.], 1500.));
    assert!(!classifier.pans(WheelUnit::Pixel, [0., 120.], 1510.));
    assert!(classifier.pans(WheelUnit::Pixel, [0., 120.], 1520.));
}

#[test]
fn resize_only_refits_fitted_sheet_but_owner_sheet_and_format_changes_always_refit() {
    let mut nav = state();
    let mut smaller = pane();
    smaller.bounds.width = 500.;
    nav.observe(owner(), 3, [300., 200.], smaller).unwrap();
    assert!(nav.fitted);
    assert!((nav.zoom - 436. / 900.).abs() < 1e-9);
    nav.zoom_at(2., None);
    nav.begin_pan(&owner(), 3, [300., 320.]);
    nav.observe(owner(), 3, [300., 200.], pane()).unwrap();
    assert_eq!(nav.zoom, 2.);
    assert!(!nav.fitted);
    assert!(nav.pan.is_none());
    nav.observe(owner(), 4, [300., 200.], pane()).unwrap();
    assert!(nav.fitted);
    nav.zoom_at(3., None);
    nav.observe(owner(), 4, [200., 300.], pane()).unwrap();
    assert!(nav.fitted);
    assert!((nav.zoom - 416. / 900.).abs() < 1e-9);
    let mut next = owner();
    next.epoch += 1;
    nav.zoom_at(3., None);
    nav.observe(next, 4, [200., 300.], pane()).unwrap();
    assert!(nav.fitted);
}

#[test]
fn picking_and_visible_raster_region_share_the_clipped_paper_transform() {
    let mut nav = state();
    nav.zoom_at(5., None);
    let transform = nav.transform();
    let paper = transform.pick([420., 320.]).unwrap();
    close(transform.to_screen(paper), [420., 320.]);
    assert_eq!(transform.pick([99., 320.]), None);
    assert_eq!(transform.pick([740., 320.]), None);
    let region = transform.visible_paper().unwrap();
    assert!((region[2] * transform.scale - pane().bounds.width).abs() < 1e-9);
    assert!((region[3] * transform.scale - pane().bounds.height).abs() < 1e-9);
    close(transform.to_screen([region[0], region[1]]), [100., 80.]);
    nav.zoom_at(0.001, None);
    assert_eq!(nav.zoom, MIN_ZOOM);
    nav.zoom_at(100., None);
    assert_eq!(nav.zoom, MAX_ZOOM);
    let before = nav.transform();
    assert!(!nav.zoom_at(f64::NAN, None));
    assert!(!nav.wheel(
        &owner(),
        3,
        [420., 320.],
        Wheel {
            window_scale: 0.,
            ..wheel(1.)
        }
    ));
    assert_eq!(nav.transform(), before);
}
