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
pub mod delaunay;
pub mod demo;
pub mod geom;
pub mod io;
pub mod make;
pub mod math;
pub mod meet;
pub mod name;
pub mod props;
pub mod roots;
#[cfg(feature = "dto")]
pub mod session;
pub mod simplify;
pub mod ssi;
pub mod tess;
pub mod topo;
pub mod trim;

/// Opt-in diagnostics, switched on by environment variables that are read
/// once per process.
pub(crate) mod diag {
    use std::sync::OnceLock;

    fn flag(cell: &'static OnceLock<bool>, name: &str) -> bool {
        *cell.get_or_init(|| std::env::var_os(name).is_some())
    }

    /// `LIMO_CAD_KERNEL_TRACE`: explain modelling and meshing decisions on
    /// standard error.
    pub fn trace() -> bool {
        static CELL: OnceLock<bool> = OnceLock::new();
        flag(&CELL, "LIMO_CAD_KERNEL_TRACE")
    }

    /// `LIMO_CAD_KERNEL_PROFILE`: print the stage timings of every Boolean.
    pub fn profile() -> bool {
        static CELL: OnceLock<bool> = OnceLock::new();
        flag(&CELL, "LIMO_CAD_KERNEL_PROFILE")
    }
}

/// Hash maps for small integer keys. The standard hasher guards against
/// hostile keys at a cost that dominated vertex lookups; these keys are the
/// kernel's own indices. A fixed hasher also makes every run identical.
pub(crate) mod fast {
    use std::collections::{HashMap, HashSet};
    use std::hash::{BuildHasherDefault, Hasher};

    #[derive(Clone, Copy, Default)]
    pub struct IntHasher(u64);

    impl Hasher for IntHasher {
        fn finish(&self) -> u64 {
            let h = self.0;
            (h ^ (h >> 29)).wrapping_mul(0xbf58_476d_1ce4_e5b9)
        }
        fn write(&mut self, bytes: &[u8]) {
            for chunk in bytes.chunks(8) {
                let mut word = [0u8; 8];
                word[..chunk.len()].copy_from_slice(chunk);
                self.write_u64(u64::from_le_bytes(word));
            }
        }
        fn write_u64(&mut self, v: u64) {
            self.0 = (self.0.rotate_left(5) ^ v).wrapping_mul(0x517c_c1b7_2722_0a95);
        }
        fn write_u32(&mut self, v: u32) {
            self.write_u64(v as u64);
        }
        fn write_i64(&mut self, v: i64) {
            self.write_u64(v as u64);
        }
        fn write_usize(&mut self, v: usize) {
            self.write_u64(v as u64);
        }
    }

    pub type Map<K, V> = HashMap<K, V, BuildHasherDefault<IntHasher>>;
    pub type Set<K> = HashSet<K, BuildHasherDefault<IntHasher>>;
}

pub use boolean::{boolean, Op};
pub use geom::{Curve, CurveKind, SurfKind, Surface};
pub use math::{v2, v3, Aabb, Frame, Xform, V2, V3};
#[cfg(feature = "dto")]
pub use session::Kernel;
pub use topo::Body;
