//! The renderer and real-kernel orientation tests use one directed polyline.
//! Keep the platform consumer from rebuilding a complementary positive arc.
#[test]
fn native_projection_draws_the_same_samples_used_for_snapping() {
    let source = include_str!("../../../desktop/src/native_viewport/platform.rs");
    let draw = source
        .split("fn draw_projected_edges<")
        .nth(1)
        .unwrap()
        .split("fn draw_base_curve_outside_profiles")
        .next()
        .unwrap();
    assert!(draw.contains("edge.points.windows(2)"));
    assert!(
        !draw.contains("atan2"),
        "endpoint-only angles lose reversed face orientation"
    );
    assert!(
        !draw.contains("draw_parametric_curve"),
        "use the same tessellation as browser snapping"
    );
}
