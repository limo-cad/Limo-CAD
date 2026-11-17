//! The kernel as a WebAssembly module for the browser demo.
//!
//! `cargo xtask kernel-demo` builds this example for `wasm32-unknown-unknown`
//! and embeds it in `web.html`, giving one self-contained page that models
//! the sample parts live. The module imports nothing: the page writes the
//! dimensions into its memory, calls `limo_model` and `limo_mesh`, and reads
//! the display buffers back.

use std::cell::RefCell;

use limo_cad_kernel::demo::PARTS;
use limo_cad_kernel::tess::{edge_polyline, tessellate, TessOptions};
use limo_cad_kernel::{Body, SurfKind};

/// Which buffer `limo_data` and `limo_size` refer to.
const POSITIONS: u32 = 0;
const NORMALS: u32 = 1;
/// Per vertex: surface kind and face index.
const TAGS: u32 = 2;
const INDICES: u32 = 3;
/// Line segments along creases, then along tangent joins.
const SHARP: u32 = 4;
const SMOOTH: u32 = 5;
const TEXT: u32 = 6;

#[derive(Default)]
struct State {
    dims: [f64; 16],
    body: Option<Body>,
    positions: Vec<f32>,
    normals: Vec<f32>,
    tags: Vec<f32>,
    indices: Vec<u32>,
    sharp: Vec<f32>,
    smooth: Vec<f32>,
    text: String,
    stats: [f64; 17],
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn kind_index(kind: SurfKind) -> usize {
    match kind {
        SurfKind::Plane => 0,
        SurfKind::Cylinder => 1,
        SurfKind::Cone => 2,
        SurfKind::Sphere => 3,
        SurfKind::Torus => 4,
    }
}

/// Where the page writes the dimensions of the part to model.
#[no_mangle]
pub extern "C" fn limo_dims() -> *mut f64 {
    STATE.with(|s| s.borrow_mut().dims.as_mut_ptr())
}

/// Put the catalogue of sample parts into the text buffer, as JSON.
#[no_mangle]
pub extern "C" fn limo_catalogue() {
    let mut out = String::from("[");
    for (i, part) in PARTS.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            "{{\"name\":\"{}\",\"title\":\"{}\",\"about\":\"{}\",\"dimensions\":[",
            part.name, part.title, part.about
        ));
        for (j, d) in part.dimensions.iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            out.push_str(&format!(
                "{{\"label\":\"{}\",\"min\":{},\"max\":{},\"value\":{}}}",
                d.label, d.min, d.max, d.value
            ));
        }
        out.push_str("]}");
    }
    out.push(']');
    STATE.with(|s| s.borrow_mut().text = out);
}

/// Model part `part` from the dimensions. Returns 1 on success; on failure
/// the previous solid is kept and the text buffer holds the kernel's reason.
#[no_mangle]
pub extern "C" fn limo_model(part: u32) -> u32 {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let Some(spec) = PARTS.get(part as usize) else {
            s.text = "no such part".into();
            return 0;
        };
        let dims = s.dims;
        match (spec.build)(&dims[..spec.dimensions.len()]) {
            Ok(body) => {
                s.body = Some(body);
                s.text.clear();
                1
            }
            Err(reason) => {
                s.text = reason;
                0
            }
        }
    })
}

/// Mesh the current solid for display and return the triangle count.
#[no_mangle]
pub extern "C" fn limo_mesh(linear: f64, angular: f64) -> u32 {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let s = &mut *s;
        let Some(body) = &s.body else {
            return 0;
        };
        let mesh = tessellate(body, &TessOptions { linear, angular });
        s.positions.clear();
        s.normals.clear();
        for (p, n) in mesh.positions.iter().zip(&mesh.normals) {
            s.positions.extend([p.x as f32, p.y as f32, p.z as f32]);
            s.normals.extend([n.x as f32, n.y as f32, n.z as f32]);
        }
        s.tags.clear();
        s.tags.resize(2 * mesh.positions.len(), 0.0);
        let mut kinds = [0.0; 5];
        for (f, &[first, count]) in mesh.faces.iter().enumerate() {
            let kind = kind_index(body.f_surface(f as u32).kind);
            kinds[kind] += 1.0;
            for &i in &mesh.indices[first as usize..(first + count) as usize] {
                s.tags[2 * i as usize] = kind as f32;
                s.tags[2 * i as usize + 1] = f as f32;
            }
        }
        s.indices.clone_from(&mesh.indices);
        s.sharp.clear();
        s.smooth.clear();
        for e in 0..body.edge_count() as u32 {
            let lines = if body.e_sharp(e) {
                &mut s.sharp
            } else {
                &mut s.smooth
            };
            for w in edge_polyline(body, e, 0.25 * linear).windows(2) {
                for p in w {
                    lines.extend([p.x as f32, p.y as f32, p.z as f32]);
                }
            }
        }
        let bounds = body.bbox();
        s.stats = [
            body.face_count() as f64,
            body.edge_count() as f64,
            body.verts.len() as f64,
            mesh.triangle_count() as f64,
            mesh.volume(),
            mesh.area(),
            kinds[0],
            kinds[1],
            kinds[2],
            kinds[3],
            kinds[4],
            bounds.lo.x,
            bounds.lo.y,
            bounds.lo.z,
            bounds.hi.x,
            bounds.hi.y,
            bounds.hi.z,
        ];
        mesh.triangle_count() as u32
    })
}

/// Describe face `f` of the current solid in the text buffer.
#[no_mangle]
pub extern "C" fn limo_face(f: u32) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let text = match &s.body {
            Some(body) if (f as usize) < body.face_count() => {
                let surf = body.f_surface(f);
                match surf.kind {
                    SurfKind::Plane => "Plane".to_string(),
                    SurfKind::Cylinder => format!("Cylinder, radius {:.3} mm", surf.r),
                    SurfKind::Cone => format!("Cone, half-angle {:.2}\u{b0}", surf.s.to_degrees()),
                    SurfKind::Sphere => format!("Sphere, radius {:.3} mm", surf.r),
                    SurfKind::Torus => {
                        format!("Torus, radii {:.3} mm and {:.3} mm", surf.r, surf.s)
                    }
                }
            }
            _ => String::new(),
        };
        s.text = text;
    })
}

#[no_mangle]
pub extern "C" fn limo_data(which: u32) -> *const u8 {
    STATE.with(|s| {
        let s = s.borrow();
        match which {
            POSITIONS => s.positions.as_ptr().cast(),
            NORMALS => s.normals.as_ptr().cast(),
            TAGS => s.tags.as_ptr().cast(),
            INDICES => s.indices.as_ptr().cast(),
            SHARP => s.sharp.as_ptr().cast(),
            SMOOTH => s.smooth.as_ptr().cast(),
            TEXT => s.text.as_ptr(),
            _ => std::ptr::null(),
        }
    })
}

/// Number of elements (floats, indices or bytes) in a buffer.
#[no_mangle]
pub extern "C" fn limo_size(which: u32) -> u32 {
    STATE.with(|s| {
        let s = s.borrow();
        (match which {
            POSITIONS => s.positions.len(),
            NORMALS => s.normals.len(),
            TAGS => s.tags.len(),
            INDICES => s.indices.len(),
            SHARP => s.sharp.len(),
            SMOOTH => s.smooth.len(),
            TEXT => s.text.len(),
            _ => 0,
        }) as u32
    })
}

/// Faces, edges, vertices, triangles, volume, area, faces per surface kind
/// (plane, cylinder, cone, sphere, torus) and the bounds of the solid.
#[no_mangle]
pub extern "C" fn limo_stat(which: u32) -> f64 {
    STATE.with(|s| s.borrow().stats.get(which as usize).copied().unwrap_or(0.0))
}
