use super::*;

impl Draft {
    pub fn radial(
        &mut self,
        next_mode: DrawingRadialDimensionMode,
        angle: f64,
        next_offset: f64,
        (next_precision, next_prefix, next_suffix, next_presentation): (
            u8,
            String,
            String,
            DrawingDimensionPresentationDto,
        ),
    ) -> Result<(), String> {
        let DrawingAnnotationDto::RadialDimension {
            feature,
            mode,
            leader_angle_deg,
            offset,
            precision,
            prefix,
            suffix,
            presentation,
            ..
        } = &mut self.edited
        else {
            return Err("Select a radial dimension".into());
        };
        if next_mode == DrawingRadialDimensionMode::Diameter
            && *mode != next_mode
            && !feature.closed
        {
            return Err("Diameter requires a closed circular edge".into());
        }
        if !angle.is_finite() || !next_offset.is_finite() || next_offset <= 0. {
            return Err("Leader angle must be finite and offset must be positive".into());
        }
        *mode = next_mode;
        *leader_angle_deg = angle;
        *offset = next_offset;
        *precision = next_precision;
        *prefix = next_prefix;
        *suffix = next_suffix;
        *presentation = next_presentation;
        Ok(())
    }
    pub fn angular(
        &mut self,
        next_radius: f64,
        next_precision: u8,
        next_prefix: String,
        next_suffix: String,
        next_presentation: DrawingDimensionPresentationDto,
    ) -> Result<(), String> {
        if !next_radius.is_finite() || next_radius <= 0. || next_radius > 1e6 {
            return Err("Arc radius must be positive and at most 1000000 paper mm".into());
        }
        let DrawingAnnotationDto::AngularDimension {
            radius,
            precision,
            prefix,
            suffix,
            presentation,
            ..
        } = &mut self.edited
        else {
            return Err("Select an angular dimension".into());
        };
        *radius = next_radius;
        *precision = next_precision;
        *prefix = next_prefix;
        *suffix = next_suffix;
        *presentation = next_presentation;
        Ok(())
    }
    pub fn move_radial(
        &mut self,
        center: [f64; 2],
        paper_radius: f64,
        shoulder: [f64; 2],
        delta: [f64; 2],
    ) -> Result<(), String> {
        let vector = [
            shoulder[0] + delta[0] - center[0],
            shoulder[1] + delta[1] - center[1],
        ];
        let distance = vector[0].hypot(vector[1]);
        if !distance.is_finite() || !paper_radius.is_finite() || paper_radius <= 0. {
            return Err("Invalid radial drag geometry".into());
        }
        let DrawingAnnotationDto::RadialDimension {
            leader_angle_deg,
            offset,
            ..
        } = &mut self.edited
        else {
            return Err("Select a radial dimension".into());
        };
        *leader_angle_deg = vector[1].atan2(vector[0]).to_degrees();
        *offset = distance.max(paper_radius + 2.) - paper_radius;
        Ok(())
    }
    pub fn move_angular(
        &mut self,
        vertex: [f64; 2],
        text: [f64; 2],
        delta: [f64; 2],
    ) -> Result<(), String> {
        let distance = (text[0] + delta[0] - vertex[0]).hypot(text[1] + delta[1] - vertex[1]);
        if !distance.is_finite() {
            return Err("Invalid angular drag geometry".into());
        }
        let DrawingAnnotationDto::AngularDimension { radius, .. } = &mut self.edited else {
            return Err("Select an angular dimension".into());
        };
        *radius = (distance - 4.).max(2.);
        Ok(())
    }
}
