//! Transient DXF semantics resolved from the existing annotation geometry.
//! Picture blocks borrow the same primitives used by SVG and native paper.
use super::*;
use crate::drawing_presentation::{geometry, text};
use limo_cad_core::UnitSystem;
use std::ops::Range;

pub(super) enum LinearMode {
    Aligned,
    Rotated,
}

enum Kind {
    Aligned,
    Rotated { angle: f64 },
    Angular { vertex: P },
    Radial { diameter: bool },
}

pub(super) struct Dimension {
    pub range: Range<usize>,
    first: P,
    second: P,
    definition: P,
    kind: Kind,
    factor: f64,
    value: f64,
    precision: u8,
    post: String,
    text: String,
    text_height: f64,
    arrow_size: f64,
}

fn supported(
    presentation: &DrawingDimensionPresentationDto,
    precision: u8,
    prefix: &str,
    suffix: &str,
) -> bool {
    !presentation.basic
        && presentation.dual_units.is_none()
        && presentation.tolerance.mode == DrawingDimensionToleranceMode::None
        && precision <= 8
        && !prefix.contains("<>")
        && !suffix.contains("<>")
        && !presentation.fit_class.contains("<>")
        && !prefix
            .chars()
            .chain(suffix.chars())
            .chain(presentation.fit_class.chars())
            .any(|c| c.is_control() || "\\{}%".contains(c))
}

fn label_template(value: String, presentation: &DrawingDimensionPresentationDto) -> String {
    text::finish(text::with_fit(value, presentation), presentation)
}

impl Dimension {
    pub(super) fn retained_bytes(&self) -> Result<usize, String> {
        std::mem::size_of::<Self>()
            .checked_add(self.post.capacity())
            .and_then(|n| n.checked_add(self.text.capacity()))
            .ok_or_else(|| "Dimension metadata storage overflow".into())
    }

    pub(super) fn radial(
        span: &geometry::Linear,
        diameter: bool,
        presentation: &DrawingDimensionPresentationDto,
        precision: u8,
        units: UnitSystem,
        affixes: (&str, &str),
        style: &DrawingSheetStyleDto,
    ) -> Option<Self> {
        let mut dimension = Self::resolve(
            span,
            LinearMode::Aligned,
            presentation,
            precision,
            units,
            affixes,
            style,
        )?;
        dimension.kind = Kind::Radial { diameter };
        dimension.definition = span.first;
        // The symbol belongs to the style, so editing keeps the live value.
        dimension.text = format!("{}<>", affixes.0);
        dimension.post = format!(
            "{}{}{}",
            if diameter { "\u{00d8}" } else { "R" },
            label_template(format!("<> {}", text::unit_label(units)), presentation),
            affixes.1
        );
        Some(dimension)
    }

    /// Complex presentation stays graphical until its regeneration semantics
    /// are implemented. Never replace it with an overridden, stale number.
    pub(super) fn resolve(
        geometry: &geometry::Linear,
        mode: LinearMode,
        presentation: &DrawingDimensionPresentationDto,
        precision: u8,
        units: UnitSystem,
        (prefix, suffix): (&str, &str),
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
            kind: match mode {
                LinearMode::Aligned => Kind::Aligned,
                LinearMode::Rotated => Kind::Rotated {
                    angle: (-span[1]).atan2(span[0]).to_degrees(),
                },
            },
            factor,
            value: geometry.value / unit_mm,
            precision,
            post: format!(
                "{prefix}{}{suffix}",
                label_template(format!("<> {}", text::unit_label(units)), presentation)
            ),
            text: "<>".into(),
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
            kind: Kind::Angular {
                vertex: geometry.vertex,
            },
            factor: 1.,
            value: geometry.value,
            precision,
            post: format!(
                "{prefix}{}{suffix}",
                label_template("<>".into(), presentation)
            ),
            text: "<>".into(),
            text_height: style.text_height_mm,
            arrow_size: style.arrow_size_mm,
        })
    }

    /// Even lossless decimal serialization cannot undo cancellation when paper
    /// coordinates are flipped into DXF's Y-up space. Keep the complete picture
    /// instead of emitting an editable dimension that regenerates another label.
    pub(super) fn preserves_measurement(&self, height: f64) -> bool {
        let point = |p: P| [p[0], height - p[1]];
        let delta = geometry::sub(point(self.second), point(self.first));
        let measured = match self.kind {
            Kind::Aligned | Kind::Radial { .. } => geometry::length(delta) * self.factor,
            Kind::Rotated { angle } => {
                let angle = angle.to_radians();
                (delta[0] * angle.cos() + delta[1] * angle.sin()).abs() * self.factor
            }
            Kind::Angular { vertex } => {
                let first = geometry::sub(point(self.first), point(vertex));
                let second = geometry::sub(point(self.second), point(vertex));
                geometry::cross(first, second)
                    .atan2(geometry::dot(first, second))
                    .to_degrees()
                    .rem_euclid(360.)
            }
        };
        let precision = self.precision as usize;
        measured.is_finite()
            && format!("{measured:.precision$}") == format!("{:.precision$}", self.value)
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
        if matches!(self.kind, Kind::Angular { .. }) {
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
            "100\nAcDbDimension\n2\n*D{index}\n3\nLIMO_DIM_{index}\n70\n{}\n71\n5\n1\n{}",
            match self.kind {
                Kind::Angular { .. } => 165,
                Kind::Aligned => 161,
                Kind::Rotated { .. } => 160,
                Kind::Radial { diameter } =>
                    if diameter {
                        163
                    } else {
                        164
                    },
            },
            dxf_text(&self.text)
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
        match self.kind {
            Kind::Aligned | Kind::Rotated { .. } => {
                out.push_str("100\nAcDbAlignedDimension\n");
                point(out, 13, self.first, paper.size[1]);
                point(out, 14, self.second, paper.size[1]);
                if let Kind::Rotated { angle } = self.kind {
                    writeln!(out, "50\n{angle}\n100\nAcDbRotatedDimension").unwrap();
                }
            }
            Kind::Angular { vertex } => {
                out.push_str("100\nAcDb3PointAngularDimension\n");
                point(out, 13, self.first, paper.size[1]);
                point(out, 14, self.second, paper.size[1]);
                point(out, 15, vertex, paper.size[1]);
            }
            Kind::Radial { diameter } => {
                out.push_str(if diameter {
                    "100\nAcDbDiametricDimension\n"
                } else {
                    "100\nAcDbRadialDimension\n"
                });
                point(out, 15, self.second, paper.size[1]);
                out.push_str("40\n0\n");
            }
        }
    }
}

fn point(out: &mut String, code: u8, p: P, height: f64) {
    writeln!(
        out,
        "{code}\n{}\n{}\n{}\n{}\n0",
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
    fn precision_fallback_keeps_all_picture_ranges_and_consistent_tables() {
        use crate::drawing_export::straight_tests::dxf_entities;

        for neighbors in [false, true] {
            let mut paper = Paper {
                dimensions: Vec::new(),
                size: [297., 210.],
                items: Vec::new(),
            };
            for (index, label) in ["before", "fallback", "after"].into_iter().enumerate() {
                if !neighbors && label != "fallback" {
                    continue;
                }
                let span = if label == "fallback" {
                    let width = 40.12345678;
                    let scale = 1e-6;
                    geometry::Linear {
                        first: [80., 70. - width * scale * 0.5],
                        second: [80., 70. + width * scale * 0.5],
                        start: [90., 70. - width * scale * 0.5],
                        end: [90., 70. + width * scale * 0.5],
                        value: width,
                    }
                } else {
                    geometry::Linear {
                        first: [80., 30.],
                        second: [160., 30.],
                        start: [80., 40.],
                        end: [160., 40.],
                        value: 40.,
                    }
                };
                let mut dimension = Dimension::resolve(
                    &span,
                    LinearMode::Aligned,
                    &DrawingDimensionPresentationDto::default(),
                    8,
                    UnitSystem::Mm,
                    ("", ""),
                    &DrawingSheetStyleDto::default(),
                )
                .unwrap();
                assert_eq!(
                    dimension.preserves_measurement(paper.size[1]),
                    label != "fallback"
                );
                let start = paper.items.len();
                paper.text([20., 20. + index as f64 * 10.], label, 3.5);
                dimension.range = start..paper.items.len();
                paper.dimensions.push(dimension);
            }
            let output = dxf(&paper, "Arial").unwrap();
            let dimensions = dxf_entities(&output, "DIMENSION");
            let styles = dxf_entities(&output, "DIMSTYLE");
            let blocks = dxf_entities(&output, "BLOCK_RECORD");
            let count = if neighbors { 2 } else { 0 };
            assert_eq!(dimensions.len(), count);
            assert_eq!(styles.len(), count);
            assert_eq!(blocks.len(), count + 2);
            let model_space = &blocks.iter().find(|b| b["2"] == "*Model_Space").unwrap()["5"];
            let texts = dxf_entities(&output, "TEXT");
            assert_eq!(texts.len(), if neighbors { 3 } else { 1 });
            let fallback = texts.iter().find(|t| t["1"] == "fallback").unwrap();
            assert_eq!(&fallback["330"], model_space);
            if neighbors {
                for label in ["before", "after"] {
                    let text = texts.iter().find(|t| t["1"] == label).unwrap();
                    assert_ne!(&text["330"], model_space);
                }
            }
        }
    }

    #[test]
    fn presentation_controls_never_become_dxf_format_instructions() {
        for fit in ["H7<>", "H7%", "H7\\", "H7{", "H7}", "H7\n"] {
            let p = DrawingDimensionPresentationDto {
                fit_class: fit.into(),
                ..Default::default()
            };
            assert!(!supported(&p, 2, "", ""));
        }
        let mut p = DrawingDimensionPresentationDto {
            reference: true,
            fit_class: " H7 ".into(),
            ..Default::default()
        };
        assert!(supported(&p, 2, "QA ", " exact"));
        p.basic = true;
        assert!(!supported(&p, 2, "", ""));
        p.basic = false;
        p.tolerance.mode = DrawingDimensionToleranceMode::Limits;
        assert!(!supported(&p, 2, "", ""));
    }

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
                    assert!(matches!(dimension.kind, Kind::Angular { vertex: v } if v == vertex));
                }
            }
        }
    }
}
