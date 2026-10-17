//! Transient DXF semantics resolved from the existing annotation geometry.
//! Picture blocks borrow the same primitives used by SVG and native paper.
use super::*;
use crate::drawing_presentation::{geometry, text};
use limo_cad_core::UnitSystem;
use std::ops::Range;

pub(super) struct Linear {
    pub range: Range<usize>,
    first: P,
    second: P,
    definition: P,
    angle: f64,
    factor: f64,
    precision: u8,
    post: String,
    text_height: f64,
    arrow_size: f64,
}

impl Linear {
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
        if presentation != &DrawingDimensionPresentationDto::default()
            || precision > 8
            || prefix.contains("<>")
            || suffix.contains("<>")
            || prefix.chars().chain(suffix.chars()).any(char::is_control)
            // DIMPOST is interpreted as CAD text, unlike literal SVG text.
            || prefix.chars().chain(suffix.chars()).any(|c| "\\{}%".contains(c))
        {
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
            factor,
            precision,
            post: format!("{prefix}<> {}{suffix}", text::unit_label(units)),
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
            "100\nAcDbDimension\n2\n*D{index}\n3\nLIMO_DIM_{index}\n70\n160\n71\n5\n1\n<>"
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
        out.push_str("100\nAcDbAlignedDimension\n");
        point(out, 13, self.first, paper.size[1]);
        point(out, 14, self.second, paper.size[1]);
        writeln!(out, "50\n{}\n100\nAcDbRotatedDimension", self.angle).unwrap();
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
