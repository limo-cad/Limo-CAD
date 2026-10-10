use super::*;
impl Draft {
    pub fn center_extension(&mut self, next: f64) -> Result<(), String> {
        if !next.is_finite() || !(0. ..=1e6).contains(&next) {
            return Err("Center extension must be from 0 to 1000000 paper mm".into());
        }
        match &mut self.edited {
            DrawingAnnotationDto::CenterMark { extension, .. }
            | DrawingAnnotationDto::CenterLine { extension, .. }
            | DrawingAnnotationDto::CenterLineBetweenEdges { extension, .. }
            | DrawingAnnotationDto::AutomaticSymmetryAxis { extension, .. }
            | DrawingAnnotationDto::BoltCircleCenterLine { extension, .. } => *extension = next,
            _ => return Err("Select a center mark or centerline between circles".into()),
        }
        Ok(())
    }
}
