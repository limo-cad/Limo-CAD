//! Portable Unicode drawing text uses actual shaped font outlines in standard
//! DXF solid hatches. A hidden TEXT retains the source without double painting.
use super::{dxf_text, xml, P};
use std::{
    fmt::Write,
    sync::{Arc, OnceLock},
};
use unicode_segmentation::UnicodeSegmentation;
use usvg::tiny_skia_path::{Path, PathBuilder, PathSegment};

const TOLERANCE_MM: f64 = 0.005;
const MAX_VERTICES: usize = 200_000;
const MAX_DXF_BYTES: usize = 32 * 1024 * 1024;

fn generic_fallback(face: &ttf_parser::Face<'_>) -> bool {
    // OpenType head.flags bit 14 marks symbolic placeholders for Unicode
    // ranges, not actual character coverage (e.g. macOS LastResort).
    face.raw_face()
        .table(ttf_parser::Tag::from_bytes(b"head"))
        .and_then(|head| head.get(16..18))
        .is_some_and(|flags| u16::from_be_bytes([flags[0], flags[1]]) & (1 << 14) != 0)
}

fn fonts() -> Arc<usvg::fontdb::Database> {
    static FONTS: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    FONTS
        .get_or_init(|| Arc::new(load_outline_fonts(None)))
        .clone()
}

/// Installed monochrome faces shared by portable DXF and native PDF printing.
/// The native host can supply its embedded fallback without shipping system fonts.
pub fn load_outline_fonts(fallback: Option<&[u8]>) -> usvg::fontdb::Database {
    let mut fonts = usvg::fontdb::Database::new();
    fonts.load_system_fonts();
    if let Some(fallback) = fallback {
        fonts.load_font_data(fallback.to_vec());
    }
    let unsupported_faces: Vec<_> = fonts
        .faces()
        .filter(|face| {
            fonts
                .with_face_data(face.id, |bytes, index| {
                    ttf_parser::Face::parse(bytes, index).is_ok_and(|font| {
                        generic_fallback(&font)
                            || [b"COLR", b"CBDT", b"sbix", b"SVG "].iter().any(|tag| {
                                font.raw_face()
                                    .table(ttf_parser::Tag::from_bytes(tag))
                                    .is_some()
                            })
                    })
                })
                .unwrap_or(false)
        })
        .map(|face| face.id)
        .collect();
    for id in unsupported_faces {
        fonts.remove_face(id);
    }
    if fonts
        .query(&usvg::fontdb::Query {
            families: &[usvg::fontdb::Family::Serif],
            ..Default::default()
        })
        .is_none()
    {
        let family = fonts
            .faces()
            .find(|face| face.style == usvg::fontdb::Style::Normal)
            .and_then(|face| face.families.first())
            .map(|family| family.0.clone());
        if let Some(family) = family {
            fonts.set_serif_family(family);
        }
    }
    fonts
}

/// Resolve the flat text emitted by the shared drawing exporter before usvg
/// shapes a print page. Its glyph-count fallback can silently omit mixed-script
/// labels; explicit grapheme font runs use the same policy as DXF output.
/// Geometry, placement, physical paper size and saved drawing intent stay intact.
pub fn resolve_svg_text(svg: &str, fonts: &usvg::fontdb::Database) -> Result<String, String> {
    if svg.len() > MAX_DXF_BYTES {
        return Err("Drawing SVG exceeds the 32 MiB text resolution budget".into());
    }
    let document = usvg::roxmltree::Document::parse_with_options(
        svg,
        usvg::roxmltree::ParsingOptions {
            nodes_limit: 1_000_000,
            ..Default::default()
        },
    )
    .map_err(|error| format!("Drawing text resolution failed: {error}"))?;
    let mut output = String::with_capacity(svg.len());
    let mut cursor = 0;
    for node in document.descendants().filter(|node| {
        node.is_element()
            && node.tag_name().name() == "text"
            && node.tag_name().namespace() == Some("http://www.w3.org/2000/svg")
    }) {
        if !node.children().all(|child| child.is_text()) {
            return Err("Print preparation requires shared flat drawing text".into());
        }
        let value: String = node.children().filter_map(|child| child.text()).collect();
        if value.is_empty() {
            continue;
        }
        let range = node.range();
        let element = &svg[range.clone()];
        let start = range.start + element.find('>').ok_or("Drawing text has no opening tag")? + 1;
        let end = range.start
            + element
                .rfind("</")
                .ok_or("Drawing text has no closing tag")?;
        let family = super::font::family(node.attribute("font-family").unwrap_or("Fira Mono"))?;
        let spans = font_spans(&value.replace('\u{fe0e}', ""), &family, fonts)?;
        output.push_str(&svg[cursor..start]);
        output.push_str(&spans);
        cursor = end;
        if output.len() + svg.len() - cursor > MAX_DXF_BYTES {
            return Err("Resolved drawing SVG exceeds the 32 MiB text budget".into());
        }
    }
    output.push_str(&svg[cursor..]);
    Ok(output)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn write_text(
    out: &mut String,
    sheet_height: f64,
    point: P,
    value: &str,
    height: f64,
    centered: bool,
    rotation_deg: f64,
    fitted_width: Option<f64>,
    layer: &str,
    family: &str,
) -> Result<(), String> {
    let outlines = if value.is_ascii() {
        Vec::new()
    } else {
        shape(value, height, centered, fitted_width, family, fonts())?
    };
    writeln!(
        out,
        "0\nTEXT\n8\n{layer}\n7\nSTANDARD\n10\n{:.5}\n20\n{:.5}\n40\n{height}\n1\n{}",
        point[0],
        sheet_height - point[1],
        dxf_text(value)
    )
    .unwrap();
    let angle = rotation_deg.to_radians();
    if let Some(width) = fitted_width {
        writeln!(
            out,
            "72\n5\n73\n0\n11\n{:.5}\n21\n{:.5}",
            point[0] + width * angle.cos(),
            sheet_height - point[1] - width * angle.sin()
        )
        .unwrap();
    } else if centered {
        writeln!(
            out,
            "72\n1\n11\n{:.5}\n21\n{:.5}",
            point[0],
            sheet_height - point[1]
        )
        .unwrap();
    }
    if rotation_deg != 0. {
        writeln!(out, "50\n{:.5}", -rotation_deg).unwrap();
    }
    if !outlines.is_empty() {
        out.push_str("60\n1\n");
    }
    for loops in outlines {
        writeln!(out, "0\nHATCH\n100\nAcDbEntity\n8\n{layer}\n100\nAcDbHatch\n10\n0\n20\n0\n30\n0\n210\n0\n220\n0\n230\n1\n2\nSOLID\n70\n1\n71\n0\n91\n{}", loops.len()).unwrap();
        for contour in loops {
            writeln!(out, "92\n2\n72\n0\n73\n1\n93\n{}", contour.len()).unwrap();
            for [x, y] in contour {
                writeln!(
                    out,
                    "10\n{:.5}\n20\n{:.5}",
                    point[0] + x * angle.cos() - y * angle.sin(),
                    sheet_height - point[1] - x * angle.sin() - y * angle.cos()
                )
                .unwrap();
            }
            out.push_str("97\n0\n");
        }
        out.push_str("75\n0\n76\n1\n98\n0\n");
        if out.len() > MAX_DXF_BYTES {
            return Err("Drawing DXF exceeds the 32 MiB text outline budget".into());
        }
    }
    Ok(())
}

type Glyph = Vec<Vec<P>>;
fn shape(
    value: &str,
    height: f64,
    centered: bool,
    fitted_width: Option<f64>,
    family: &str,
    fonts: Arc<usvg::fontdb::Database>,
) -> Result<Vec<Glyph>, String> {
    let anchor = if centered {
        " text-anchor=\"middle\""
    } else {
        ""
    };
    let fit = fitted_width.map_or_else(String::new, |width| {
        format!(" textLength=\"{width}\" lengthAdjust=\"spacingAndGlyphs\"")
    });
    let value_for_shaping = value.replace('\u{fe0e}', "");
    let spans = font_spans(&value_for_shaping, family, &fonts)?;
    let svg = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100\" height=\"100\"><text xml:space=\"preserve\" x=\"0\" y=\"0\" font-family=\"{}\" font-size=\"{height}\"{anchor}{fit}>{spans}</text></svg>", xml(family));
    let options = usvg::Options {
        fontdb: fonts,
        ..Default::default()
    };
    let tree = usvg::Tree::from_str(&svg, &options)
        .map_err(|error| format!("Drawing text shaping failed: {error}"))?;
    let text = find_text(tree.root())
        .ok_or("Drawing DXF needs an installed outline font covering this label")?;
    let mut result = Vec::new();
    let mut vertices = 0;
    for glyph in text
        .layouted()
        .iter()
        .flat_map(|span| &span.positioned_glyphs)
    {
        if glyph
            .text
            .chars()
            .all(|ch| ch.is_whitespace() || ch == '\u{fe0e}' || ch == '\u{fe0f}')
        {
            continue;
        }
        if glyph.id.0 == 0 {
            return Err(format!(
                "Drawing DXF needs an installed font covering {:?}",
                glyph.text
            ));
        }
        let path = tree
            .fontdb()
            .with_face_data(glyph.font, |bytes, index| {
                let face = ttf_parser::Face::parse(bytes, index).ok()?;
                let mut builder = Outline(PathBuilder::new());
                face.outline_glyph(ttf_parser::GlyphId(glyph.id.0), &mut builder)?;
                builder.0.finish()
            })
            .flatten()
            .ok_or_else(|| {
                format!(
                    "Drawing DXF requires outline glyphs for {:?}; choose an outline font",
                    glyph.text
                )
            })?;
        let transform = text.abs_transform().pre_concat(glyph.outline_transform());
        let path = path
            .transform(transform)
            .ok_or("Drawing text has a non-finite outline transform")?;
        result.push(flatten(&path, &mut vertices)?);
    }
    if result.is_empty() && value.chars().any(|ch| !ch.is_whitespace()) {
        return Err("Drawing DXF could not resolve this label to outline glyphs".into());
    }
    Ok(result)
}

fn find_text(group: &usvg::Group) -> Option<&usvg::Text> {
    group.children().iter().find_map(|node| match node {
        usvg::Node::Text(text) => Some(text.as_ref()),
        usvg::Node::Group(group) => find_text(group),
        _ => None,
    })
}

fn font_spans(value: &str, family: &str, fonts: &usvg::fontdb::Database) -> Result<String, String> {
    let primary = fonts
        .query(&usvg::fontdb::Query {
            families: &[
                usvg::fontdb::Family::Name(family),
                usvg::fontdb::Family::Serif,
            ],
            ..Default::default()
        })
        .ok_or("Drawing output needs an installed outline font")?;
    let covers = |id, cluster: &str| {
        fonts
            .with_face_data(id, |bytes, index| {
                ttf_parser::Face::parse(bytes, index).is_ok_and(|face| {
                    !generic_fallback(&face)
                        && cluster.chars().all(|ch| {
                            ch.is_whitespace()
                                || matches!(ch, '\u{200c}' | '\u{200d}' | '\u{fe0e}' | '\u{fe0f}')
                                || face.glyph_index(ch).is_some()
                        })
                })
            })
            .unwrap_or(false)
    };
    let mut resolved = std::collections::HashMap::new();
    let mut runs: Vec<(usvg::fontdb::ID, String)> = Vec::new();
    for cluster in value.graphemes(true) {
        let id = if let Some(id) = resolved.get(cluster) {
            *id
        } else {
            let id = if covers(primary, cluster) {
                primary
            } else {
                fonts
                    .faces()
                    .filter(|face| {
                        face.style == usvg::fontdb::Style::Normal
                            && face.weight == usvg::fontdb::Weight::NORMAL
                    })
                    .find(|face| covers(face.id, cluster))
                    .map(|face| face.id)
                    .ok_or_else(|| {
                        format!("Drawing output needs an installed font covering {cluster:?}")
                    })?
            };
            resolved.insert(cluster.to_owned(), id);
            id
        };
        if let Some((_, text)) = runs.last_mut().filter(|(previous, _)| *previous == id) {
            text.push_str(cluster);
        } else {
            runs.push((id, cluster.to_owned()));
        }
    }
    let mut spans = String::new();
    for (id, text) in runs {
        let family = &fonts
            .face(id)
            .ok_or("Drawing font disappeared while shaping")?
            .families[0]
            .0;
        write!(
            spans,
            "<tspan font-family=\"{}\">{}</tspan>",
            xml(family),
            xml(&text)
        )
        .unwrap();
    }
    Ok(spans)
}

struct Outline(PathBuilder);
impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to(x, y);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.0.quad_to(x1, y1, x, y);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.0.cubic_to(x1, y1, x2, y2, x, y);
    }
    fn close(&mut self) {
        self.0.close();
    }
}

fn push(contour: &mut Vec<P>, point: P, vertices: &mut usize) -> Result<(), String> {
    *vertices += 1;
    if *vertices > MAX_VERTICES {
        return Err("Drawing label exceeds the 200,000 outline vertex budget".into());
    }
    if !point.iter().all(|v| v.is_finite()) {
        return Err("Drawing font contains non-finite outline coordinates".into());
    }
    contour.push(point);
    Ok(())
}
fn midpoint(a: P, b: P) -> P {
    [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5]
}
fn distance_to_segment(p: P, a: P, b: P) -> f64 {
    let v = [b[0] - a[0], b[1] - a[1]];
    let d = v[0] * v[0] + v[1] * v[1];
    let t = if d == 0. {
        0.
    } else {
        (((p[0] - a[0]) * v[0] + (p[1] - a[1]) * v[1]) / d).clamp(0., 1.)
    };
    (p[0] - a[0] - t * v[0]).hypot(p[1] - a[1] - t * v[1])
}
fn cubic(
    points: [P; 4],
    depth: u8,
    contour: &mut Vec<P>,
    vertices: &mut usize,
) -> Result<(), String> {
    let [a, b, c, d] = points;
    if distance_to_segment(b, a, d).max(distance_to_segment(c, a, d)) <= TOLERANCE_MM {
        return push(contour, d, vertices);
    }
    if depth == 12 {
        return Err("Drawing font curve exceeds the bounded outline subdivision depth".into());
    }
    let ab = midpoint(a, b);
    let bc = midpoint(b, c);
    let cd = midpoint(c, d);
    let abc = midpoint(ab, bc);
    let bcd = midpoint(bc, cd);
    let center = midpoint(abc, bcd);
    cubic([a, ab, abc, center], depth + 1, contour, vertices)?;
    cubic([center, bcd, cd, d], depth + 1, contour, vertices)
}
fn flatten(path: &Path, vertices: &mut usize) -> Result<Glyph, String> {
    let mut loops = Vec::new();
    let mut contour = Vec::new();
    let mut current = [0., 0.];
    let point = |p: usvg::tiny_skia_path::Point| [f64::from(p.x), f64::from(p.y)];
    for segment in path.segments() {
        match segment {
            PathSegment::MoveTo(p) => {
                current = point(p);
                contour.clear();
                push(&mut contour, current, vertices)?;
            }
            PathSegment::LineTo(p) => {
                current = point(p);
                push(&mut contour, current, vertices)?;
            }
            PathSegment::QuadTo(control, end) => {
                let b = point(control);
                let d = point(end);
                let mix = |a: P, b: P| [(a[0] + 2. * b[0]) / 3., (a[1] + 2. * b[1]) / 3.];
                cubic(
                    [current, mix(current, b), mix(d, b), d],
                    0,
                    &mut contour,
                    vertices,
                )?;
                current = d;
            }
            PathSegment::CubicTo(b, c, d) => {
                let d = point(d);
                cubic([current, point(b), point(c), d], 0, &mut contour, vertices)?;
                current = d;
            }
            PathSegment::Close => {
                if contour.first() == contour.last() {
                    contour.pop();
                }
                if contour.len() >= 3 {
                    loops.push(std::mem::take(&mut contour));
                }
            }
        }
    }
    if loops.is_empty() {
        return Err("Drawing font has no closed outline contours".into());
    }
    Ok(loops)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unavailable_font_rejects_without_fabricating_a_glyph() {
        assert!(shape(
            "\u{10ffff}",
            3.,
            false,
            None,
            "missing",
            Arc::new(usvg::fontdb::Database::new())
        )
        .is_err());
        assert!(shape("\u{10ffff}", 3., false, None, "Arial", fonts()).is_err());
        assert!(shape("A\u{10ffff}", 3., false, None, "Arial", fonts()).is_err());
    }
    #[test]
    fn curves_keep_closed_holes_and_respect_paper_tolerance_and_budget() {
        let mut builder = PathBuilder::new();
        builder.move_to(0., 0.);
        builder.cubic_to(0., 10., 10., 10., 10., 0.);
        builder.close();
        builder.move_to(4., 1.);
        builder.line_to(6., 1.);
        builder.line_to(5., 2.);
        builder.close();
        let path = builder.finish().unwrap();
        let mut vertices = 0;
        let loops = flatten(&path, &mut vertices).unwrap();
        assert_eq!(loops.len(), 2);
        assert!(loops[0].len() > 16);
        assert_eq!(loops[1].len(), 3);
        assert!(loops[0]
            .iter()
            .any(|p| (p[0] - 5.).hypot(p[1] - 7.5) < TOLERANCE_MM));
        assert!(flatten(&path, &mut MAX_VERTICES.clone()).is_err());
    }
    #[test]
    fn mixed_rotated_fitted_text_has_one_hidden_source_and_real_filled_outlines() {
        let mut out = String::new();
        write_text(
            &mut out,
            100.,
            [20., 30.],
            "O\u{2300} A",
            4.,
            false,
            90.,
            Some(20.),
            "ANNOTATION",
            "Arial",
        )
        .unwrap();
        assert_eq!(out.matches("0\nTEXT\n").count(), 1);
        assert!(out.contains("60\n1\n0\nHATCH\n"));
        assert!(out.contains("11\n20.00000\n21\n50.00000"));
        assert!(out.contains("91\n2\n"), "O preserves its hole");
        assert!(!out.contains("NOBS_EMBEDDED_FONT"));
        assert!(out.matches("0\nHATCH\n").count() >= 3);
    }
    #[test]
    fn mixed_cjk_latin_and_technical_labels_keep_every_cluster() {
        let value = "Café 零件 ⌀ Ø Ω Ⓜ\u{fe0e}";
        let glyphs = shape(value, 3., false, None, "Arial", fonts()).unwrap();
        assert_eq!(glyphs.len(), 10);
    }

    #[test]
    fn print_font_resolution_preserves_geometry_and_every_escaped_cluster() {
        let source = r#"<svg xmlns="http://www.w3.org/2000/svg" width="297mm" height="210mm" viewBox="0 0 297 210"><path d="M10 10 L20 20"/><text x="40" y="50" font-family="Arial" font-size="3" textLength="30" lengthAdjust="spacingAndGlyphs" transform="rotate(90 40 50)">Café 零件 &amp; &lt;Ø&gt;</text><text x="1" y="2" font-family="Arial">Second label</text></svg>"#;
        let fonts = fonts();
        let resolved = resolve_svg_text(source, &fonts).unwrap();
        let original = usvg::roxmltree::Document::parse(source).unwrap();
        let rewritten = usvg::roxmltree::Document::parse(&resolved).unwrap();
        assert!(resolved.contains(r#"<path d="M10 10 L20 20"/>"#));
        for attribute in ["width", "height", "viewBox"] {
            assert_eq!(
                original.root_element().attribute(attribute),
                rewritten.root_element().attribute(attribute)
            );
        }
        let labels = |document: &usvg::roxmltree::Document<'_>| {
            document
                .descendants()
                .filter(|node| node.is_element() && node.tag_name().name() == "text")
                .count()
        };
        assert_eq!(labels(&original), 2);
        assert_eq!(labels(&rewritten), 2);
        for (before, after) in original
            .descendants()
            .filter(|node| node.is_element() && node.tag_name().name() == "text")
            .zip(
                rewritten
                    .descendants()
                    .filter(|node| node.is_element() && node.tag_name().name() == "text"),
            )
        {
            for attribute in before.attributes() {
                assert_eq!(after.attribute(attribute.name()), Some(attribute.value()));
            }
            let text: String = after
                .descendants()
                .filter(|node| node.is_text())
                .filter_map(|node| node.text())
                .collect();
            assert_eq!(text, before.text().unwrap());
            let spans: Vec<_> = after.children().filter(|node| node.is_element()).collect();
            assert!(!spans.is_empty());
            for span in spans {
                assert_eq!(span.tag_name().name(), "tspan");
                let family = span.attribute("font-family").unwrap();
                let face = fonts
                    .query(&usvg::fontdb::Query {
                        families: &[usvg::fontdb::Family::Name(family)],
                        ..Default::default()
                    })
                    .unwrap();
                assert!(fonts
                    .with_face_data(face, |bytes, index| {
                        let font = ttf_parser::Face::parse(bytes, index).unwrap();
                        span.text()
                            .unwrap()
                            .chars()
                            .all(|ch| ch.is_whitespace() || font.glyph_index(ch).is_some())
                    })
                    .unwrap());
            }
        }
        assert!(resolved.contains("&amp; &lt;Ø&gt;"));
    }

    #[test]
    fn print_font_resolution_rejects_missing_glyphs_and_nonshared_text() {
        let unsupported = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg"><text font-family="Arial">A{}</text></svg>"#,
            '\u{10ffff}'
        );
        assert!(resolve_svg_text(&unsupported, &fonts()).is_err());
        let nested = r#"<svg xmlns="http://www.w3.org/2000/svg"><text><tspan>Unreviewed markup</tspan></text></svg>"#;
        assert!(resolve_svg_text(nested, &fonts()).is_err());
    }
}
