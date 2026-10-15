use std::{env, fs, path::Path};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().collect();
    if args.len() != 3 { return Err("usage: render_svg input.svg output.png".into()); }
    if Path::new(&args[2]).exists() { return Err("refusing to overwrite prior image".into()); }
    let mut options = resvg::usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    let tree = resvg::usvg::Tree::from_data(&fs::read(&args[1])?, &options)?;
    let scale = 2000.0 / tree.size().width();
    let mut pixels = resvg::tiny_skia::Pixmap::new(2000, (tree.size().height() * scale).ceil() as u32).ok_or("invalid SVG dimensions")?;
    pixels.fill(resvg::tiny_skia::Color::WHITE);
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixels.as_mut());
    pixels.save_png(&args[2])?;
    Ok(())
}
