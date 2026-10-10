use super::*;

impl Draft {
    pub fn series(
        &mut self,
        new_layout: DrawingChainDimensionLayout,
        new_mode: DrawingLinearDimensionMode,
        new_offset: f64,
        new_spacing: f64,
        (new_precision, new_prefix, new_suffix, new_presentation): (
            u8,
            String,
            String,
            DrawingDimensionPresentationDto,
        ),
    ) -> Result<(), String> {
        let DrawingAnnotationDto::ChainDimension {
            layout,
            mode,
            offset,
            spacing,
            precision,
            prefix,
            suffix,
            presentation,
            ..
        } = &mut self.edited
        else {
            return Err("Select a dimension series".into());
        };
        *layout = new_layout;
        *mode = new_mode;
        *offset = new_offset;
        *spacing = new_spacing;
        *precision = new_precision;
        *prefix = new_prefix;
        *suffix = new_suffix;
        *presentation = new_presentation;
        Ok(())
    }
    pub fn ordinate(
        &mut self,
        new_axis: DrawingOrdinateAxis,
        new_offset: f64,
        new_precision: u8,
        new_presentation: DrawingDimensionPresentationDto,
    ) -> Result<(), String> {
        let DrawingAnnotationDto::OrdinateDimension {
            axis,
            offset,
            precision,
            presentation,
            ..
        } = &mut self.edited
        else {
            return Err("Select an ordinate dimension".into());
        };
        *axis = new_axis;
        *offset = new_offset;
        *precision = new_precision;
        *presentation = new_presentation;
        Ok(())
    }
    pub fn move_ordinate(&mut self, delta: [f64; 2]) -> Result<(), String> {
        let DrawingAnnotationDto::OrdinateDimension { offset: start, .. } = &self.original else {
            return Err("Select an ordinate dimension".into());
        };
        let next = start
            + if delta[0].abs() > delta[1].abs() {
                delta[0]
            } else {
                delta[1]
            };
        if delta.iter().any(|n| !n.is_finite()) || !next.is_finite() {
            return Err("Dimension offset must be finite".into());
        }
        let DrawingAnnotationDto::OrdinateDimension { offset, .. } = &mut self.edited else {
            unreachable!()
        };
        *offset = next;
        Ok(())
    }
}
