//! Limo CAD's own boundary-representation kernel.
//!
//! The kernel is data-oriented rather than object-oriented:
//!
//! * **Geometry is tagged plain data.** A surface or curve is a kind tag, a
//!   frame and two scalars ([`geom`]). Evaluation is a `match`, not a virtual
//!   call, and every surface has a closed-form signed distance.
//! * **Topology is flat arrays.** A [`topo::Body`] is a few `Vec`s indexed by
//!   `u32` with implicit half-edge pairing. Cloning a body is a `memcpy`.
//! * **Operations are functions from bodies to bodies.** Nothing is mutated
//!   in place, so history replay can keep and reuse any intermediate result.
//! * **Exact where it matters.** Faces lie on planes, cylinders, cones,
//!   spheres and tori; edges are lines, circles and ellipses. Tessellation is
//!   derived output, never modelling input.
//!
//! The crate has no native dependencies and builds for WebAssembly.

pub mod blend;
pub mod boolean;
pub mod geom;
pub mod io;
pub mod make;
pub mod math;
pub mod props;
pub mod roots;
#[cfg(feature = "dto")]
pub mod session;
pub mod simplify;
pub mod ssi;
pub mod tess;
pub mod topo;
pub mod trim;

pub use boolean::{boolean, Op};
pub use geom::{Curve, CurveKind, SurfKind, Surface};
pub use math::{v2, v3, Aabb, Frame, Xform, V2, V3};
#[cfg(feature = "dto")]
pub use session::Kernel;
pub use topo::Body;
