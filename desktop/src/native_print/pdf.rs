//! svg2pdf still uses usvg 0.45. Pass it resolved paths, so the shared current
//! parser owns text shaping and the PDF backend cannot substitute older fonts.

fn pdf_tree(tree: &resvg::usvg::Tree, dpi: f32) -> Result<svg2pdf::usvg::Tree, String> {
    let svg = tree.to_string(&resvg::usvg::WriteOptions {
        preserve_text: false,
        coordinates_precision: 9,
        transforms_precision: 9,
        ..Default::default()
    });
    let options = svg2pdf::usvg::Options {
        dpi,
        image_href_resolver: svg2pdf::usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..Default::default()
    };
    svg2pdf::usvg::Tree::from_str(&svg, &options).map_err(|error| error.to_string())
}

pub(super) fn to_pdf(tree: &resvg::usvg::Tree, dpi: f32) -> Result<Vec<u8>, String> {
    let tree = pdf_tree(tree, dpi)?;
    // Keep the same parser DPI: PDF points are 72-DPI; SVG units are 96-DPI.
    svg2pdf::to_pdf(
        &tree,
        svg2pdf::ConversionOptions {
            embed_text: false,
            ..Default::default()
        },
        svg2pdf::PageOptions { dpi },
    )
    .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn resolved_unicode_paths_keep_rotated_text_geometry_and_vector_pdf_size() {
        let source = r#"<svg xmlns="http://www.w3.org/2000/svg" width="297mm" height="210mm" viewBox="0 0 297 210"><path d="M10 10 L100 10" stroke="black" fill="none"/><text x="40" y="50" font-family="Arial" font-size="4" textLength="50" lengthAdjust="spacingAndGlyphs" transform="rotate(37 40 50)">Café 零件 ⌀ Ø Ω</text></svg>"#;
        let fonts = Arc::new(limo_cad_occt::drawing_export::load_outline_fonts(None));
        let source = limo_cad_occt::drawing_export::resolve_svg_text(source, &fonts).unwrap();
        let options = resvg::usvg::Options {
            dpi: 96.,
            fontdb: fonts,
            ..Default::default()
        };
        let current = resvg::usvg::Tree::from_str(&source, &options).unwrap();
        let converted = pdf_tree(&current, options.dpi).unwrap();
        assert!((converted.size().width() - current.size().width()).abs() < 0.001);
        assert!((converted.size().height() - current.size().height()).abs() < 0.001);
        let paths = converted.to_string(&svg2pdf::usvg::WriteOptions {
            preserve_text: true,
            coordinates_precision: 9,
            transforms_precision: 9,
            ..Default::default()
        });
        assert!(
            !paths.contains("<text"),
            "PDF must use already-shaped paths"
        );
        let reparsed = resvg::usvg::Tree::from_str(&paths, &Default::default()).unwrap();
        let raster = |tree: &resvg::usvg::Tree| {
            let mut image = resvg::tiny_skia::Pixmap::new(1123, 794).unwrap();
            resvg::render(
                tree,
                resvg::tiny_skia::Transform::identity(),
                &mut image.as_mut(),
            );
            image
        };
        let before = raster(&current);
        let after = raster(&reparsed);
        let changed = before
            .data()
            .iter()
            .zip(after.data())
            .filter(|(a, b)| a.abs_diff(**b) > 2)
            .count();
        assert!(
            changed < 100,
            "The parser bridge changed {changed} color channels"
        );
        assert!(before.data().iter().filter(|&&value| value != 0).count() > 1000);
        let bytes = to_pdf(&current, options.dpi).unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
        let pdf = String::from_utf8_lossy(&bytes);
        let bounds = pdf
            .split("/MediaBox [")
            .nth(1)
            .unwrap()
            .split(']')
            .next()
            .unwrap();
        let bounds: Vec<f64> = bounds
            .split_whitespace()
            .map(|value| value.parse().unwrap())
            .collect();
        assert_eq!(&bounds[..2], &[0., 0.]);
        assert!((bounds[2] - 297. * 72. / 25.4).abs() < 0.001);
        assert!((bounds[3] - 210. * 72. / 25.4).abs() < 0.001);
        assert!(
            !pdf.contains("/Subtype /Image"),
            "Drawing must remain vector output"
        );
    }
}
