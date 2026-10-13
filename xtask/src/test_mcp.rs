use anyhow::{bail, Result};

/// Product scenarios use the shared Rust transport and commented script files.
pub fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    let suite = args.next().ok_or_else(|| {
        anyhow::anyhow!(
            "Select a Rust native scenario; cargo xtask --help lists the supported commands"
        )
    })?;
    if suite == "native-platform" {
        return crate::native_platform_test::run(args);
    }
    if suite == "switching-measurement" {
        return crate::native_switching_test::run(args);
    }
    if suite == "native-exchange" {
        return crate::native_exchange_test::run(args);
    }
    if suite == "native-lifecycle" {
        return crate::native_lifecycle_test::run(args);
    }
    if suite == "native-sketch" {
        return crate::native_sketch_test::run(args);
    }
    if suite == "native-build" {
        return crate::native_build_test::run(args);
    }
    if suite == "native-support" {
        return crate::native_support_test::run(args);
    }
    if suite == "native-refine" {
        return crate::native_refine_test::run(args);
    }
    if suite == "native-body" {
        return crate::native_body_test::run(args);
    }
    if suite == "native-pattern" {
        return crate::native_body_test::run_patterns(args);
    }
    if suite == "native-assembly" {
        return crate::native_assembly_test::run(args);
    }
    if suite == "native-joint" {
        return crate::native_joint_test::run(args);
    }
    if suite == "native-inspect" {
        return crate::native_inspect_test::run(args);
    }
    if suite == "native-studies" {
        return crate::native_studies_test::run(args);
    }
    if suite == "native-mechanism" {
        return crate::native_mechanism_test::run(args);
    }
    if suite == "native-mechanism-platform" {
        return crate::native_drawing_navigation_test::run_mechanism_owned(args);
    }
    if suite == "native-move" {
        return crate::native_move_test::run(args);
    }
    if suite == "native-hole" {
        return crate::native_hole_test::run(args);
    }
    if suite == "native-thread" {
        return crate::native_thread_test::run(args);
    }
    if suite == "native-print-layout" {
        return crate::native_print_layout_test::run(args);
    }
    if suite == "native-print-intent" {
        return crate::native_print_intent_test::run(args);
    }
    if suite == "native-print-height" {
        return crate::native_print_height_test::run(args);
    }
    if suite == "native-print-modifier" {
        return crate::native_print_modifier_test::run(args);
    }
    if suite == "native-bambu-project" {
        return crate::native_bambu_project_test::run(args);
    }
    if suite == "native-bambu-repeated" {
        return crate::native_bambu_project_test::run_repeated(args);
    }
    if suite == "native-view" {
        return crate::native_view_test::run(args);
    }
    if suite == "native-planes" {
        return crate::native_planes_test::run(args);
    }
    if suite == "native-drawing" {
        return crate::native_drawing_test::run(args);
    }
    if suite == "native-drawing-annotations" {
        return crate::native_drawing_annotations_test::run(args);
    }
    if suite == "native-drawing-sections" {
        return crate::native_drawing_section_test::run(args);
    }
    if suite == "native-drawing-hole" {
        return crate::native_drawing_hole_test::run(args);
    }
    if suite == "native-profile-export" {
        return crate::native_profile_export_test::run(args);
    }
    if suite == "native-drawing-authoring" {
        return crate::native_drawing_annotations_test::run_authoring(args);
    }
    if suite == "native-drawing-editor" {
        return crate::native_drawing_editor_test::run(args);
    }
    if suite == "native-drawing-navigation" {
        return crate::native_drawing_annotations_test::run_navigation(args);
    }
    if suite == "native-drawing-platform" {
        return crate::native_drawing_navigation_test::run_owned(args);
    }
    if suite == "native-body-appearance" {
        return crate::native_body_appearance_test::run(args);
    }
    if suite == "native-cam" {
        return crate::native_cam_test::run(args);
    }
    if suite == "native-cam-platform" {
        return crate::native_drawing_navigation_test::run_cam_owned(args);
    }
    if suite == "native-chamfer-platform" {
        return crate::native_drawing_navigation_test::run_chamfer_owned(args);
    }
    if suite == "native-cloud-platform" {
        return crate::native_drawing_navigation_test::run_cloud_owned(args);
    }
    if suite == "native-drawing-output-platform" {
        return crate::native_drawing_navigation_test::run_output_owned(args);
    }
    if suite == "native-centers-platform" {
        return crate::native_drawing_navigation_test::run_centers_owned(args);
    }
    if suite == "native-drawing-hole-platform" {
        return crate::native_drawing_navigation_test::run_hole_owned(args);
    }
    if suite == "native-cam-geometry-platform" {
        return crate::native_drawing_navigation_test::run_cam_geometry_owned(args);
    }
    if suite == "native-cam-geometry" {
        return crate::native_cam_geometry_test::run(args);
    }
    if suite == "native-cam-nc" {
        return crate::native_cam_nc_test::run(args);
    }
    if suite == "native-preferences" {
        return crate::native_preferences_test::run(args);
    }
    if suite == "native-lessons" {
        return crate::native_lessons_test::run(args);
    }
    if suite == "native-scripts-platform" {
        return crate::native_drawing_navigation_test::run_scripts_owned(args);
    }
    if suite == "playback" {
        return crate::playback_test::run(&args.collect::<Vec<_>>()).map_err(anyhow::Error::msg);
    }
    if suite == "scripts-workspace" {
        return crate::playback_test::run_workspace(&args.collect::<Vec<_>>())
            .map_err(anyhow::Error::msg);
    }
    if suite == "garden-bench" {
        return crate::replay::run(
            ["--recipe".to_owned(), "garden-bench".to_owned()]
                .into_iter()
                .chain(args),
        );
    }
    match suite.as_str() {
        "contracts" => bail!("The React browser contracts were retired. Run cargo test --locked -p limo-cad-interface for the shared catalog; use native-interface tests in desktop for Bevy controls."),
        "live" | "controls" | "exit" | "bench" | "drawing" => crate::mcp_scenarios::run(&suite, args),
        _ => bail!("Unknown MCP suite '{suite}'; select a Rust native scenario"),
    }
}
