//! Transient DXF semantics resolved from the existing annotation geometry.
//! Picture blocks borrow the same primitives used by SVG and native paper.
use super::*;
use crate::drawing_presentation::{geometry, text};
use limo_cad_core::UnitSystem;
use std::ops::Range;

pub(super) struct Dimension {
    pub range: Range<usize>,
    first: P,
    second: P,
    definition: P,
    angle: f64,
    vertex: Option<P>,
    factor: f64,
    precision: u8,
    post: String,
    text_height: f64,
    arrow_size: f64,
}

fn supported(
    presentation: &DrawingDimensionPresentationDto,
    precision: u8,
    prefix: &str,
    suffix: &str,
) -> bool {
    presentation == &DrawingDimensionPresentationDto::default()
        && precision <= 8
        && !prefix.contains("<>")
        && !suffix.contains("<>")
        && !prefix
            .chars()
            .chain(suffix.chars())
            .any(|c| c.is_control() || "\\{}%".contains(c))
}

impl Dimension {
    /// Complex presentation stays graphical until its regeneration semantics
    /// are implemented. Never replace it with an overridden, stale number.
    pub(super) fn resolve(
        geometry: &geometry::Linear,
        presentation: &DrawingDimensionPresentationDto,
        precision: u8,
        units: UnitSystem,
        prefix: &str,
        suffix: &str,
        style: &DrawingSheetStyleDto,
    ) -> Option<Self> {
        if !supported(presentation, precision, prefix, suffix) {
            return None;
        }
        let span = geometry::sub(geometry.end, geometry.start);
        let paper_length = geometry::length(span);
        let unit_mm = match units {
            UnitSystem::Mm => 1.,
            UnitSystem::Cm => 10.,
            UnitSystem::In => 25.4,
        };
        let factor = geometry.value / paper_length / unit_mm;
        if !factor.is_finite() || factor <= 0. {
            return None;
        }
        Some(Self {
            range: 0..0,
            first: geometry.first,
            second: geometry.second,
            definition: geometry.end,
            angle: (-span[1]).atan2(span[0]).to_degrees(),
            vertex: None,
            factor,
            precision,
            post: format!("{prefix}<> {}{suffix}", text::unit_label(units)),
            text_height: style.text_height_mm,
            arrow_size: style.arrow_size_mm,
        })
    }

    pub(super) fn angular(
        geometry: &geometry::Angular,
        presentation: &DrawingDimensionPresentationDto,
        precision: u8,
        prefix: &str,
        suffix: &str,
        style: &DrawingSheetStyleDto,
    ) -> Option<Self> {
        if !supported(presentation, precision, prefix, suffix)
            || !(0. < geometry.value && geometry.value <= 180.)
        {
            return None;
        }
        // Paper Y points down. DXF measures counterclockwise in Y-up space.
        let (first, second) = if geometry::cross(
            geometry::sub(geometry.first, geometry.vertex),
            geometry::sub(geometry.second, geometry.vertex),
        ) > 0.
        {
            (geometry.second, geometry.first)
        } else {
            (geometry.first, geometry.second)
        };
        Some(Self {
            range: 0..0,
            first,
            second,
            definition: *geometry.points.get(geometry.points.len() / 2)?,
            angle: 0.,
            vertex: Some(geometry.vertex),
            factor: 1.,
            precision,
            post: format!("{prefix}<>{suffix}"),
            text_height: style.text_height_mm,
            arrow_size: style.arrow_size_mm,
        })
    }

    pub(super) fn style(
        &self,
        out: &mut String,
        index: usize,
        table: usize,
        handle: usize,
        font: usize,
    ) {
        writeln!(out, "0\nDIMSTYLE\n105\n{handle:X}\n330\n{table:X}\n100\nAcDbSymbolTableRecord\n100\nAcDbDimStyleTableRecord\n2\nLIMO_DIM_{index}\n70\n0\n3\n{}\n40\n1\n41\n{}\n140\n{}\n144\n{}\n271\n{}\n277\n2\n278\n46\n78\n0\n77\n1\n340\n{font:X}", dxf_text(&self.post), self.arrow_size, self.text_height, self.factor, self.precision).unwrap();
        if self.vertex.is_some() {
            writeln!(out, "179\n{}\n275\n0\n79\n0", self.precision).unwrap();
        }
    }

    pub(super) fn entity(
        &self,
        out: &mut String,
        index: usize,
        paper: &Paper,
        handles: &mut dxf_handles::Handles,
        owner: usize,
    ) {
        handles.entity(out, "DIMENSION", owner, "DIMENSION");
        writeln!(
            out,
            "100\nAcDbDimension\n2\n*D{index}\n3\nLIMO_DIM_{index}\n70\n{}\n71\n5\n1\n<>",
            if self.vertex.is_some() { 165 } else { 160 }
        )
        .unwrap();
        point(out, 10, self.definition, paper.size[1]);
        let text = paper.items[self.range.clone()]
            .iter()
            .find_map(|item| {
                if let Primitive::Text {
                    point,
                    height,
                    rotation_deg,
                    ..
                } = item
                {
                    let angle = rotation_deg.to_radians();
                    Some([
                        point[0] + height * 0.4 * angle.sin(),
                        point[1] - height * 0.4 * angle.cos(),
                    ])
                } else {
                    None
                }
            })
            .unwrap_or(self.definition);
        point(out, 11, text, paper.size[1]);
        out.push_str(if self.vertex.is_some() {
            "100\nAcDb3PointAngularDimension\n"
        } else {
            "100\nAcDbAlignedDimension\n"
        });
        point(out, 13, self.first, paper.size[1]);
        point(out, 14, self.second, paper.size[1]);
        if let Some(vertex) = self.vertex {
            point(out, 15, vertex, paper.size[1]);
        } else {
            writeln!(out, "50\n{}\n100\nAcDbRotatedDimension", self.angle).unwrap();
        }
    }
}

fn point(out: &mut String, code: u8, p: P, height: f64) {
    writeln!(
        out,
        "{code}\n{:.10}\n{}\n{:.10}\n{}\n0",
        p[0],
        code + 10,
        height - p[1],
        code + 20
    )
    .unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angular_definitions_preserve_minor_sector_across_winding_and_angle_seam() {
        for rotation in [-179_f64, -91., 0., 89., 179.] {
            for degrees in [0.1_f64, 15., 89.9, 90., 135., 179.9, 180.] {
                for winding in [-1., 1.] {
                    let vertex = [25., 32.];
                    let arm = |angle: f64| {
                        geometry::add(
                            vertex,
                            [
                                20. * angle.to_radians().cos(),
                                20. * angle.to_radians().sin(),
                            ],
                        )
                    };
                    let resolved = geometry::angular(
                        vertex,
                        arm(rotation),
                        arm(rotation + winding * degrees),
                        15.,
                    )
                    .unwrap();
                    let dimension = Dimension::angular(
                        &resolved,
                        &DrawingDimensionPresentationDto::default(),
                        3,
                        "",
                        "",
                        &DrawingSheetStyleDto::default(),
                    )
                    .unwrap();
                    let angle = |p: P| (vertex[1] - p[1]).atan2(p[0] - vertex[0]);
                    let measured = (angle(dimension.second) - angle(dimension.first))
                        .to_degrees()
                        .rem_euclid(360.);
                    assert!(
                        (measured - degrees).abs() < 1e-7,
                        "rotation={rotation} angle={degrees} winding={winding}: {measured}"
                    );
                    assert_eq!(dimension.vertex, Some(vertex));
                }
            }
        }
    }
}
