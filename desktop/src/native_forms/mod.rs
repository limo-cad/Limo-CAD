//! Typed, host-neutral feature editors over the existing engine request DTOs.
//! These are form state and validation, not another command or schema catalog.

mod feature;
pub(crate) mod joint;
mod measurement;
pub(crate) mod motion_study;

pub(crate) use feature::{
    ApplyTicket, FormModel, MoveMode, ProfileSource, SolidField, SolidFieldView, SolidForm,
    SolidFormKind, SolidFormPresentation,
};
pub(crate) use measurement::{DimensionKind, MeasurementInput, ParameterValue};
