//! Shared Rust/OCCT document host for native desktop and hosted geometry.
//!
//! Execution and the host API require `native-occt` and an OCCT SDK. With no
//! features this crate is empty, keeping ordinary engine workspace builds
//! independent of the native SDK. It is a service-side component, not the
//! browser's WebAssembly kernel.

#[cfg(feature = "native-occt")]
mod host;
#[cfg(feature = "native-occt")]
pub use host::{
    DrawingProjectionBasis, NativeEngineHost, NativeViewportDocument, NativeViewportFrame,
    NativeViewportSnapshot, ResolvedDrawingProjection, BOOTSTRAP_SESSION_ID,
};
