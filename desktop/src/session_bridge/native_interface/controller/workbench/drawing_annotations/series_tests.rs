use super::*;
use serde_json::json;

fn fixture() -> (
    DrawingSheetDto,
    BTreeMap<u64, (DrawingViewDto, DrawingProjectionDto)>,
) {
    let (mut sheet, mut projections) = super::tests::fixture();
    let (view, p) = projections.get_mut(&1).unwrap();
    view.scale = 1.;
    view.position = [75., 40.];
    p.bounds = [0., 0., 50., 20.];
    p.anchors[2].point = [50., 0.];
    p.anchors[2].model_point = [50., 0., 0.];
    p.anchors[1].point = [20., 0.];
    p.anchors[1].model_point = [20., 0., 0.];
    sheet.views = vec![view.clone()];
    (sheet, projections)
}
fn anchor(edge: u64, endpoint: &str) -> serde_json::Value {
    json!({"body_id":1,"edge_id":edge,"edge_key":format!("e{edge}"),"endpoint":endpoint,"fallback_point":[999.,999.,999.]})
}
fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.0001
}

#[test]
fn continued_matches_release_react_and_every_series_label_has_a_separate_stable_hit_part() {
    for layout in ["chain", "baseline", "continued"] {
        for spacing in [7., 11.] {
            let (mut sheet, p) = fixture();
            sheet.annotations = vec![super::tests::annotation(json!({"kind":"chain_dimension",
                "anchors":[anchor(1,"start"),anchor(1,"end"),anchor(2,"start")],
                "mode":"horizontal","layout":layout,"offset":12.,"spacing":spacing}))];
            let art = try_render(&sheet, &p, UnitSystem::Mm).unwrap();
            let ys: Vec<_> = art
                .segments
                .iter()
                .filter(|s| s.arrow)
                .map(|s| s.y1 as f64)
                .collect();
            let second = if layout == "baseline" {
                62. + spacing
            } else {
                62.
            };
            assert_eq!(ys.len(), 4);
            assert!(ys[..2].iter().all(|y| close(*y, 62.)));
            assert!(
                ys[2..].iter().all(|y| close(*y, second)),
                "{layout}: {ys:?}"
            );
            assert_eq!(art.labels.len(), 2);
            assert_eq!(art.marks.len(), 2);
            for (part, mark) in art.marks.iter().enumerate() {
                assert_eq!((mark.id, mark.part), (1, part));
                assert_eq!(
                    mark.center,
                    [art.labels[part].x as f64, art.labels[part].y as f64]
                );
                assert_eq!(
                    mark.size,
                    [
                        art.labels[part].width_mm as f64,
                        art.labels[part].height_mm as f64
                    ]
                );
                assert_eq!(mark.linear_points, Some([[50., 50.], [70., 50.]]));
            }
            assert_ne!(art.marks[0].center, art.marks[1].center);
            let mut missing = p.clone();
            missing.get_mut(&1).unwrap().1.anchors.remove(2);
            let broken = try_render(&sheet, &missing, UnitSystem::Mm).unwrap();
            assert_eq!(broken.labels.len(), 1);
            assert_eq!(broken.labels[0].text, "!");
            assert!(broken.marks.is_empty());
        }
    }
}

#[test]
fn ordinate_matches_release_leader_tail_arrow_and_start_anchored_baseline() {
    for axis in ["x", "y", "both"] {
        for offset in [12_f64, -12.] {
            let (mut sheet, p) = fixture();
            sheet.annotations = vec![super::tests::annotation(
                json!({"kind":"ordinate_dimension",
                "origin":anchor(1,"start"),"target":anchor(1,"end"),"axis":axis,"offset":offset}),
            )];
            let art = try_render(&sheet, &p, UnitSystem::Mm).unwrap();
            let elbow = 50. + offset;
            let text_y = elbow + offset.signum() * 2.;
            assert!(
                art.segments.iter().any(|s| !s.arrow
                    && close(s.x1 as f64, 70.)
                    && close(s.x2 as f64, 70.)
                    && close(s.y1 as f64, elbow)
                    && close(s.y2 as f64, text_y)),
                "Ordinate leader must reach text"
            );
            assert!(art.segments.iter().any(|s| s.arrow
                && close(s.x1 as f64, 70.)
                && close(s.y1 as f64, 50.)
                && (s.y2 as f64 - 50.).signum() == offset.signum()));
            let label = &art.labels[0];
            assert_eq!(label.align, LabelAlign::Start);
            assert!(close((label.x - label.width_mm * 0.5) as f64, 70.));
            assert!(close(
                label.y as f64,
                text_y - 0.7 - sheet.style.text_height_mm * 0.4
            ));
            assert_eq!(label.text.contains('X'), axis != "y");
            assert_eq!(label.text.contains('Y'), axis != "x");
            assert!(!label.text.contains("X ") && !label.text.contains("Y "));
            assert!(art
                .fills
                .iter()
                .any(|f| f.round && close(f.width as f64, 2.4)));
            assert_eq!(art.marks[0].ordinate_points, Some([[50., 50.], [70., 50.]]));
            assert!(art.marks[0].linear_points.is_none());
        }
    }
}
