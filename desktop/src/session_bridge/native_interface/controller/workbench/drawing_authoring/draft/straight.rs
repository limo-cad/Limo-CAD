use super::*;

impl Draft {
    pub fn straight(
        &mut self,
        point: [f64; 2],
        new_precision: u8,
        new_prefix: String,
        new_suffix: String,
        new_presentation: DrawingDimensionPresentationDto,
    ) -> Result<(), String> {
        if point.iter().any(|v| !v.is_finite() || v.abs() > 1e6) {
            return Err("Invalid dimension paper position".into());
        }
        let (DrawingAnnotationDto::LineDimension {
            position,
            precision,
            prefix,
            suffix,
            presentation,
            ..
        }
        | DrawingAnnotationDto::PointLineDimension {
            position,
            precision,
            prefix,
            suffix,
            presentation,
            ..
        }) = &mut self.edited
        else {
            return Err("Select an edge dimension".into());
        };
        *position = point;
        *precision = new_precision;
        *prefix = new_prefix;
        *suffix = new_suffix;
        *presentation = new_presentation;
        Ok(())
    }
    pub fn move_straight(&mut self, delta: [f64; 2], sheet_mm: [f64; 2]) -> Result<(), String> {
        if delta.iter().any(|v| !v.is_finite())
            || sheet_mm.iter().any(|v| !v.is_finite() || *v < 10.)
        {
            return Err("Invalid dimension paper position".into());
        }
        let (DrawingAnnotationDto::LineDimension {
            position: original, ..
        }
        | DrawingAnnotationDto::PointLineDimension {
            position: original, ..
        }) = &self.original
        else {
            return Err("Select an edge dimension".into());
        };
        let (DrawingAnnotationDto::LineDimension { position, .. }
        | DrawingAnnotationDto::PointLineDimension { position, .. }) = &mut self.edited
        else {
            unreachable!()
        };
        *position = std::array::from_fn(|i| (original[i] + delta[i]).clamp(5., sheet_mm[i] - 5.));
        Ok(())
    }
}
