//! Typed, host-neutral feature editors over the existing engine request DTOs.
//! These are form state and validation, not another command or schema catalog.

mod feature;
pub(crate) mod joint;
pub(crate) mod motion_study;
mod measurement;

pub(crate) use feature::{
    MoveMode, ApplyTicket, SolidField, SolidFieldView, SolidForm, SolidFormKind, FormModel, ProfileSource,
};
pub(crate) use measurement::{DimensionKind, MeasurementInput, ParameterValue};
