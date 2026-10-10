use super::*;
impl Draft {
    pub fn hole_note(&mut self, next: DrawingAnnotationDto) -> Result<(), String> {
        let (
            DrawingAnnotationDto::HoleNote {
                id,
                view_id,
                feature,
                source_feature_id,
                feature_name,
                ..
            },
            DrawingAnnotationDto::HoleNote {
                id: old_id,
                view_id: old_view,
                feature: old_feature,
                source_feature_id: old_source,
                feature_name: old_name,
                ..
            },
        ) = (&next, &self.edited)
        else {
            return Err("Select a hole note".into());
        };
        if (id, view_id, feature, source_feature_id, feature_name)
            != (old_id, old_view, old_feature, old_source, old_name)
        {
            return Err("Hole note association changed; reset the edit".into());
        }
        self.edited = next;
        Ok(())
    }
    pub fn move_hole(&mut self, delta: [f64; 2], sheet: [f64; 2]) -> Result<(), String> {
        if delta.iter().any(|v| !v.is_finite()) || sheet.iter().any(|v| !v.is_finite() || *v < 10.)
        {
            return Err("Invalid hole note paper position".into());
        }
        let DrawingAnnotationDto::HoleNote {
            position: original, ..
        } = &self.original
        else {
            return Err("Select a hole note".into());
        };
        let DrawingAnnotationDto::HoleNote { position, .. } = &mut self.edited else {
            unreachable!()
        };
        *position = std::array::from_fn(|i| (original[i] + delta[i]).clamp(5., sheet[i] - 5.));
        Ok(())
    }
}
