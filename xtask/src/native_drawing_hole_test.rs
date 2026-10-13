//! Bounded live native HoleNote proof on an explicitly supplied blank session.
use crate::native_fixture::start;
use anyhow::Result;

pub(super) fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let args: Vec<String> = args.collect();
    let physical = args.iter().any(|arg| arg == "--desktop-input");
    let mut fixture = start(args.into_iter(), "native-drawing-hole")?;
    let result = crate::native_drawing_authoring_test::exercise_hole(
        &mut fixture.client,
        &fixture.out,
        &fixture.server,
        physical,
    )?;
    std::fs::write(&fixture.report, serde_json::to_vec_pretty(&result)?)?;
    println!(
        "Hole note state/export checks passed; review original captures in {}",
        fixture.out.display()
    );
    Ok(())
}
