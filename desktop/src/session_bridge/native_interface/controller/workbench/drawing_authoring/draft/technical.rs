use super::*;
impl Draft {
    pub fn technical(&mut self, annotation: DrawingAnnotationDto) -> Result<(), String> {
        if annotation.id() != self.original.id()
            || std::mem::discriminant(&annotation) != std::mem::discriminant(&self.original)
        {
            return Err("Annotation identity changed; reset the inspector".into());
        }
        self.edited = annotation;
        Ok(())
    }
    pub fn move_technical(&mut self, delta: [f64; 2], size: [f64; 2]) -> Result<(), String> {
        if delta.iter().any(|n| !n.is_finite()) || size.iter().any(|n| !n.is_finite() || *n < 10.) {
            return Err("Invalid annotation paper position".into());
        }
        let point =
            |p: [f64; 2]| std::array::from_fn(|i| (p[i] + delta[i]).clamp(5., size[i] - 5.));
        self.edited = self.original.clone();
        match &mut self.edited {
            DrawingAnnotationDto::JoggedRadiusDimension { position, jog, .. } => {
                *position = point(*position);
                *jog = point(*jog);
            }
            DrawingAnnotationDto::DatumFeature { position, .. }
            | DrawingAnnotationDto::GdtFrame { position, .. }
            | DrawingAnnotationDto::SurfaceTexture { position, .. }
            | DrawingAnnotationDto::EdgeRequirement { position, .. }
            | DrawingAnnotationDto::WeldSymbol { position, .. }
            | DrawingAnnotationDto::ItemBalloon { position, .. } => *position = point(*position),
            _ => return Err("Select a movable symbol".into()),
        }
        Ok(())
    }
    pub fn move_arc_length(
        &mut self,
        center: [f64; 2],
        text: [f64; 2],
        delta: [f64; 2],
    ) -> Result<(), String> {
        if center
            .iter()
            .chain(&text)
            .chain(&delta)
            .any(|n| !n.is_finite())
        {
            return Err("Invalid arc length position".into());
        }
        let DrawingAnnotationDto::ArcLengthDimension { offset: start, .. } = &self.original else {
            return Err("Select an arc length dimension".into());
        };
        let DrawingAnnotationDto::ArcLengthDimension { offset, .. } = &mut self.edited else {
            unreachable!()
        };
        let radius = (text[0] - center[0]).hypot(text[1] - center[1]);
        *offset = (start + (text[0] + delta[0] - center[0]).hypot(text[1] + delta[1] - center[1])
            - radius)
            .max(0.1);
        Ok(())
    }
}
