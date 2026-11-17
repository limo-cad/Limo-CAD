//! Remove topology that carries no geometry: edges between faces on the same
//! surface, and vertices in the middle of a single curve.
//!
//! Everything here is combinatorial. Deleting an edge only re-links the
//! half-edge successors around its ends; no point is ever evaluated or
//! compared, so the pass cannot introduce geometric error.

use crate::math::*;
use crate::topo::{Body, NONE};

fn find(parent: &mut [u32], mut i: u32) -> u32 {
    while parent[i as usize] != i {
        parent[i as usize] = parent[parent[i as usize] as usize];
        i = parent[i as usize];
    }
    i
}

/// Whether `t` is where a closed curve begins.
fn starts_there(curve: &crate::geom::Curve, t: f64) -> bool {
    curve.periodic() && {
        let turn = t.rem_euclid(TAU);
        turn.min(TAU - turn) < 1.0e-9
    }
}

pub fn unify(body: &Body) -> Body {
    let nf = body.face_count();
    let ne = body.edge_count();
    let nh = ne * 2;

    // Faces on one surface, with one orientation, joined across an edge.
    let mut parent: Vec<u32> = (0..nf as u32).collect();
    for e in 0..ne as u32 {
        let (f1, f2) = (body.h_face(2 * e), body.h_face(2 * e + 1));
        if f1 != f2
            && body.f_surf[f1 as usize] == body.f_surf[f2 as usize]
            && body.f_sense[f1 as usize] == body.f_sense[f2 as usize]
        {
            let (r1, r2) = (find(&mut parent, f1), find(&mut parent, f2));
            if r1 != r2 {
                parent[r2.max(r1) as usize] = r1.min(r2);
            }
        }
    }
    let class: Vec<u32> = (0..nf as u32).map(|f| find(&mut parent, f)).collect();
    let mut dead: Vec<bool> = (0..ne as u32)
        .map(|e| class[body.h_face(2 * e) as usize] == class[body.h_face(2 * e + 1) as usize])
        .collect();

    // Successor of each surviving half-edge once the dead edges are gone.
    let mut next: Vec<u32> = vec![NONE; nh];
    for h in 0..nh as u32 {
        if dead[(h >> 1) as usize] {
            continue;
        }
        let mut n = body.h_next[h as usize];
        let mut guard = 0;
        while dead[(n >> 1) as usize] && guard <= nh {
            n = body.h_next[(n ^ 1) as usize];
            guard += 1;
        }
        next[h as usize] = n;
    }

    // Edge geometry, mutable so that chains can be merged.
    let mut e_t = body.e_t.clone();
    let mut e_v = body.e_v.clone();
    // Per vertex: (edge, end) incidences of surviving edges.
    let mut around: Vec<Vec<(u32, u8)>> = vec![Vec::new(); body.verts.len()];
    for e in 0..ne as u32 {
        if dead[e as usize] {
            continue;
        }
        let [v0, v1] = e_v[e as usize];
        around[v0 as usize].push((e, 0));
        around[v1 as usize].push((e, 1));
    }
    // A vertex with two different edges on one curve is redundant.
    let mut dropped: Vec<bool> = vec![false; nh];
    let mut queue: Vec<u32> = (0..body.verts.len() as u32).collect();
    while let Some(v) = queue.pop() {
        let inc = &around[v as usize];
        if inc.len() != 2 || inc[0].0 == inc[1].0 {
            continue;
        }
        let ((ea, ia), (eb, ib)) = (inc[0], inc[1]);
        let (ca, cb) = (body.e_curve_ref(ea), body.e_curve_ref(eb));
        let Some((sign, offset)) = ca.same(cb) else {
            continue;
        };
        let [b0, b1] = e_t[eb as usize];
        let (mut m0, mut m1) = (sign * b0 + offset, sign * b1 + offset);
        if m0 > m1 {
            std::mem::swap(&mut m0, &mut m1);
        }
        let [a0, a1] = e_t[ea as usize];
        // Where a closed curve starts stays a vertex. A rim that is cut
        // again later keeps that one point where it was, so whatever was
        // tied to it — a dimension, a mate — still finds it.
        if starts_there(ca, e_t[ea as usize][ia as usize])
            || starts_there(cb, e_t[eb as usize][ib as usize])
        {
            continue;
        }
        // Slide the other span by whole periods until it meets this one.
        let (attach, at) = if ia == 1 { (a1, m0) } else { (a0, m1) };
        let turn = if ca.periodic() {
            ((attach - at) / TAU).round() * TAU
        } else {
            0.0
        };
        let (m0, m1) = (m0 + turn, m1 + turn);
        let speed = ca.deriv(attach).len().max(1.0e-12);
        let gap = if ia == 1 { m0 - a1 } else { a0 - m1 };
        if gap.abs() * speed > 100.0 * TOL {
            continue;
        }
        let merged = if ia == 1 { [a0, m1] } else { [m0, a1] };
        if ca.periodic() && merged[1] - merged[0] > TAU + 1.0e-9 {
            continue;
        }
        let far = e_v[eb as usize][(1 - ib) as usize];
        e_t[ea as usize] = merged;
        e_v[ea as usize][ia as usize] = far;
        dead[eb as usize] = true;
        dropped[(2 * eb) as usize] = true;
        dropped[(2 * eb + 1) as usize] = true;
        around[v as usize].clear();
        for entry in &mut around[far as usize] {
            if entry.0 == eb && entry.1 == 1 - ib {
                *entry = (ea, ia);
            }
        }
        queue.push(far);
    }
    // A merged-away half-edge hands its successor to whatever precedes it.
    let resolve = |mut n: u32| {
        let mut guard = 0;
        while dropped[n as usize] && guard <= nh {
            n = next[n as usize];
            guard += 1;
        }
        n
    };

    // Rebuild compactly.
    let mut out = Body::new();
    let mut surf_map: Vec<u32> = vec![NONE; body.surfs.len()];
    let mut vert_map: Vec<u32> = vec![NONE; body.verts.len()];
    let mut edge_map: Vec<u32> = vec![NONE; ne];
    let mut seen: Vec<bool> = vec![false; nh];
    // Loops per face class, in a stable order.
    let mut loops_of: Vec<Vec<Vec<u32>>> = vec![Vec::new(); nf];
    for start in 0..nh as u32 {
        if seen[start as usize] || dead[(start >> 1) as usize] {
            continue;
        }
        let mut lp = Vec::new();
        let mut h = start;
        let mut guard = 0;
        loop {
            seen[h as usize] = true;
            lp.push(h);
            h = resolve(next[h as usize]);
            guard += 1;
            if h == start || guard > nh {
                break;
            }
        }
        let face = class[body.h_face(start) as usize];
        loops_of[face as usize].push(lp);
    }
    for f in 0..nf as u32 {
        if class[f as usize] != f {
            continue;
        }
        let s = body.f_surf[f as usize] as usize;
        if surf_map[s] == NONE {
            surf_map[s] = out.add_surf(body.surfs[s]);
        }
        let mut loops: Vec<Vec<u32>> = Vec::with_capacity(loops_of[f as usize].len());
        for lp in &loops_of[f as usize] {
            let mut mapped = Vec::with_capacity(lp.len());
            for &h in lp {
                let e = (h >> 1) as usize;
                if edge_map[e] == NONE {
                    let mut vs = e_v[e];
                    for v in &mut vs {
                        if vert_map[*v as usize] == NONE {
                            vert_map[*v as usize] = out.add_vert(body.verts[*v as usize]);
                        }
                        *v = vert_map[*v as usize];
                    }
                    let c = out.add_curve(*body.e_curve_ref(e as u32));
                    edge_map[e] = out.add_edge(c, e_t[e][0], e_t[e][1], vs[0], vs[1]);
                }
                mapped.push(2 * edge_map[e] + (h & 1));
            }
            loops.push(mapped);
        }
        // Faces that became one keep the tag of the first of them: in the
        // result of a Boolean that is the one from the body worked on,
        // rather than the tool's.
        out.add_tagged_face(
            surf_map[s],
            body.f_sense[f as usize],
            &loops,
            body.f_tag[f as usize],
        );
    }
    out
}
