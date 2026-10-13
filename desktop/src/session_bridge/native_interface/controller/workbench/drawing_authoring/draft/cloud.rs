use super::*;

impl Draft {
    pub fn revision_cloud(&mut self, value: String) -> Result<(), String> {
        let DrawingAnnotationDto::RevisionCloud { revision, .. } = &mut self.edited else {
            return Err("Select a revision cloud".into());
        };
        *revision = value;
        Ok(())
    }
    /// Translate from the original vertices and clamp each point to
    /// the five-millimetre inset. Repeated moves are cumulative, not additive.
    pub fn move_revision_cloud(&mut self, delta: [f64; 2], sheet: [f64; 2]) -> Result<(), String> {
        if delta.iter().any(|v| !v.is_finite()) || sheet.iter().any(|v| !v.is_finite() || *v < 10.)
        {
            return Err("Invalid revision cloud paper position".into());
        }
        let DrawingAnnotationDto::RevisionCloud {
            points: original, ..
        } = &self.original
        else {
            return Err("Select a revision cloud".into());
        };
        let points: Vec<_> = original
            .iter()
            .map(|point| std::array::from_fn(|i| (point[i] + delta[i]).clamp(5., sheet[i] - 5.)))
            .collect();
        if points.iter().flatten().any(|v| !v.is_finite()) {
            return Err("Invalid revision cloud paper position".into());
        }
        let DrawingAnnotationDto::RevisionCloud { points: edited, .. } = &mut self.edited else {
            unreachable!()
        };
        *edited = points;
        Ok(())
    }
}
