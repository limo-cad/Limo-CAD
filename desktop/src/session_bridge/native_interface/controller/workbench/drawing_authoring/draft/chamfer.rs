use super::*;
impl Draft {
    pub fn chamfer(
        &mut self,
        point: [f64; 2],
        setback: f64,
        angle: f64,
        text: String,
    ) -> Result<(), String> {
        if point.iter().any(|v| !v.is_finite())
            || !setback.is_finite()
            || setback <= 0.
            || !angle.is_finite()
            || angle <= 0.
            || angle >= 180.
            || text.chars().count() > 256
        {
            return Err("Enter a positive chamfer setback, angle between 0 and 180, finite paper position, and prefix of at most 256 characters".into());
        }
        let DrawingAnnotationDto::ChamferNote {
            position,
            length,
            angle_deg,
            prefix,
            ..
        } = &mut self.edited
        else {
            return Err("Select a chamfer note".into());
        };
        *position = point;
        *length = setback;
        *angle_deg = angle;
        *prefix = text;
        Ok(())
    }
    pub fn move_chamfer(&mut self, delta: [f64; 2], sheet: [f64; 2]) -> Result<(), String> {
        if delta.iter().any(|v| !v.is_finite()) || sheet.iter().any(|v| !v.is_finite() || *v < 10.)
        {
            return Err("Invalid chamfer note paper position".into());
        }
        let DrawingAnnotationDto::ChamferNote {
            position: original, ..
        } = &self.original
        else {
            return Err("Select a chamfer note".into());
        };
        let DrawingAnnotationDto::ChamferNote { position, .. } = &mut self.edited else {
            unreachable!()
        };
        *position = std::array::from_fn(|i| (original[i] + delta[i]).clamp(5., sheet[i] - 5.));
        Ok(())
    }
}
