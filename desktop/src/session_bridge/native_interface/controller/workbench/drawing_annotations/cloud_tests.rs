use super::*;
use serde_json::json;

#[test]
fn shared_cloud_geometry_keeps_native_strokes_labels_selection_and_saved_vertices() {
    for points in [
        vec![[20., 20.], [55., 20.], [35., 50.]],
        vec![[20., 20.], [55., 20.], [55., 50.], [20., 50.]],
        vec![
            [20., 20.],
            [35., 18.],
            [55., 30.],
            [55., 30.],
            [52., 50.],
            [30., 55.],
            [14., 37.],
        ],
    ] {
        let (mut sheet, projections) = tests::fixture();
        sheet.annotations = vec![tests::annotation(
            json!({"kind":"revision_cloud","revision":"cω\nB","points":points}),
        )];
        let before = serde_json::to_string(&sheet).unwrap();
        let art = try_render(&sheet, &projections, UnitSystem::In).unwrap();
        let cloud = limo_cad_occt::drawing_presentation::cloud::Cloud::new(&points).unwrap();
        let expected: Vec<_> = cloud
            .arcs()
            .flat_map(|a| {
                a.points()
                    .windows(2)
                    .map(|p| {
                        [
                            p[0][0] as f32,
                            p[0][1] as f32,
                            p[1][0] as f32,
                            p[1][1] as f32,
                        ]
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        let actual: Vec<_> = art
            .segments
            .iter()
            .map(|s| {
                assert_eq!(s.width_mm, 0.45_f32);
                assert!(matches!(s.ink, Ink::Revision));
                [s.x1, s.y1, s.x2, s.y2]
            })
            .collect();
        assert_eq!(actual, expected);
        assert_eq!(art.labels[0].text, "REV cω");
        assert_eq!(art.labels[1].text, "B");
        assert!((art.labels[1].y - art.labels[0].y - 4.).abs() < 1e-5);
        let baseline = cloud.caption_baseline("REV cω\nB");
        assert!((f64::from(art.labels[0].y) - (baseline[1] - 3.2 * 0.4)).abs() < 1e-5);
        let top_ink = art
            .segments
            .iter()
            .flat_map(|s| [s.y1, s.y2])
            .fold(f32::INFINITY, f32::min)
            - 0.45 * 0.5;
        let last_bottom = f64::from(art.labels[1].y + art.labels[1].height_mm * 0.5);
        assert!(last_bottom + 1. <= f64::from(top_ink) + 1e-4);
        assert!(art
            .labels
            .iter()
            .all(|l| l.text_height_mm == 3.2_f32 && matches!(l.ink, Ink::Revision)));
        assert_eq!(art.marks.len(), 1);
        assert_eq!(serde_json::to_string(&sheet).unwrap(), before);
    }
}

#[test]
fn invalid_cloud_rejects_whole_native_art_instead_of_leaving_partial_content() {
    let (mut sheet, projections) = tests::fixture();
    sheet.annotations = vec![
        tests::annotation(
            json!({"kind":"note","text":"Preserved preceding record","position":[20.,20.]}),
        ),
        tests::annotation(
            json!({"kind":"revision_cloud","revision":"A","points":[[20.,20.],[40.,20.],[30.,40.]]}),
        ),
    ];
    if let DrawingAnnotationDto::RevisionCloud { points, .. } = &mut sheet.annotations[1] {
        points[1][0] = f64::NAN;
    }
    assert!(try_render(&sheet, &projections, UnitSystem::Mm).is_err());
}
