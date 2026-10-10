//! Disposable annotation edits over the existing complete shared drawing DTO.
//! The controller must fence application with the captured document receipt.
use limo_cad_sketch::*;
mod center;
mod chamfer;
mod cloud;
mod curved;
mod hole;
mod series;
mod straight;
mod technical;

#[derive(Clone, Copy)]
pub(super) struct Selection {
    pub sheet_id: u64,
    pub annotation_id: u64,
}
pub(super) struct Draft {
    selection: Selection,
    original: DrawingAnnotationDto,
    edited: DrawingAnnotationDto,
}
fn record(
    document: &DrawingDocumentDto,
    selection: Selection,
) -> Result<&DrawingAnnotationDto, String> {
    document
        .sheets
        .iter()
        .find(|s| s.id == selection.sheet_id)
        .and_then(|s| {
            s.annotations
                .iter()
                .find(|a| a.id() == selection.annotation_id)
        })
        .ok_or("Drawing annotation was removed".into())
}
impl Draft {
    pub fn selection(&self) -> Selection {
        self.selection
    }
    pub fn annotation(&self) -> &DrawingAnnotationDto {
        &self.edited
    }
    pub fn new(document: &DrawingDocumentDto, selection: Selection) -> Result<Self, String> {
        let original = record(document, selection)?.clone();
        Ok(Self {
            selection,
            edited: original.clone(),
            original,
        })
    }
    pub fn dirty(&self) -> bool {
        self.edited != self.original
    }
    pub fn note(&mut self, value: String) -> Result<(), String> {
        let DrawingAnnotationDto::Note { text, .. } = &mut self.edited else {
            return Err("Select a note".into());
        };
        *text = value;
        Ok(())
    }
    pub fn linear(
        &mut self,
        new_mode: DrawingLinearDimensionMode,
        new_offset: f64,
        new_precision: u8,
        new_prefix: String,
        new_suffix: String,
        new_presentation: DrawingDimensionPresentationDto,
    ) -> Result<(), String> {
        let DrawingAnnotationDto::LinearDimension {
            mode,
            offset,
            precision,
            prefix,
            suffix,
            presentation,
            ..
        } = &mut self.edited
        else {
            return Err("Select a linear dimension".into());
        };
        *mode = new_mode;
        *offset = new_offset;
        *precision = new_precision;
        *prefix = new_prefix;
        *suffix = new_suffix;
        *presentation = new_presentation;
        Ok(())
    }
    pub fn move_note(&mut self, point: [f64; 2], sheet_mm: [f64; 2]) -> Result<(), String> {
        if point.iter().any(|v| !v.is_finite())
            || sheet_mm.iter().any(|v| !v.is_finite() || *v < 10.)
        {
            return Err("Invalid note paper position".into());
        }
        let DrawingAnnotationDto::Note { position, .. } = &mut self.edited else {
            return Err("Select a note".into());
        };
        *position = std::array::from_fn(|i| point[i].clamp(5., sheet_mm[i] - 5.));
        Ok(())
    }
    pub fn set_note_position(&mut self, point: [f64; 2]) -> Result<(), String> {
        if point.iter().any(|v| !v.is_finite()) {
            return Err("Invalid note paper position".into());
        }
        let DrawingAnnotationDto::Note { position, .. } = &mut self.edited else {
            return Err("Select a note".into());
        };
        *position = point;
        Ok(())
    }
    /// Delta is cumulative paper millimetres from press, never a pixel delta.
    /// Only release commits; pointer moves update this disposable preview.
    pub fn move_linear(
        &mut self,
        first: [f64; 2],
        second: [f64; 2],
        delta: [f64; 2],
    ) -> Result<(), String> {
        if first
            .iter()
            .chain(&second)
            .chain(&delta)
            .any(|v| !v.is_finite())
        {
            return Err("Invalid dimension paper position".into());
        }
        let (DrawingAnnotationDto::LinearDimension { offset: start, .. }
        | DrawingAnnotationDto::ChainDimension { offset: start, .. }) = &self.original
        else {
            return Err("Select a linear dimension".into());
        };
        let (DrawingAnnotationDto::LinearDimension { mode, offset, .. }
        | DrawingAnnotationDto::ChainDimension { mode, offset, .. }) = &mut self.edited
        else {
            unreachable!()
        };
        let increment = match mode {
            DrawingLinearDimensionMode::Horizontal => delta[1],
            DrawingLinearDimensionMode::Vertical => delta[0],
            DrawingLinearDimensionMode::Aligned => {
                let span = [second[0] - first[0], second[1] - first[1]];
                let length = span[0].hypot(span[1]);
                if length < 1e-8 {
                    0.
                } else {
                    (-delta[0] * span[1] + delta[1] * span[0]) / length
                }
            }
        };
        let next = start + increment;
        if !next.is_finite() {
            return Err("Dimension offset must be finite".into());
        }
        *offset = next;
        Ok(())
    }
    #[cfg(test)]
    pub fn apply(&self, document: &DrawingDocumentDto) -> Result<DrawingDocumentDto, String> {
        self.commit(document, false)
    }
    #[cfg(test)]
    pub fn delete(&self, document: &DrawingDocumentDto) -> Result<DrawingDocumentDto, String> {
        self.commit(document, true)
    }
    pub fn verify(&self, document: &DrawingDocumentDto) -> Result<(), String> {
        if record(document, self.selection)? != &self.original {
            return Err("Drawing annotation changed; reset before applying".into());
        }
        Ok(())
    }
    #[cfg(test)]
    fn commit(
        &self,
        document: &DrawingDocumentDto,
        delete: bool,
    ) -> Result<DrawingDocumentDto, String> {
        self.verify(document)?;
        let mut next = document.clone();
        let sheet = next
            .sheets
            .iter_mut()
            .find(|s| s.id == self.selection.sheet_id)
            .unwrap();
        let index = sheet
            .annotations
            .iter()
            .position(|a| a.id() == self.selection.annotation_id)
            .unwrap();
        if delete {
            sheet.annotations.remove(index);
        } else {
            sheet.annotations[index] = self.edited.clone();
        }
        if (delete || self.dirty()) && sheet.release.status == DrawingReleaseStatus::Released {
            sheet.release.status = DrawingReleaseStatus::Draft;
        }
        next.validate()?;
        Ok(next)
    }
}
