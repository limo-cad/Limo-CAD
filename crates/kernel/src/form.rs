//! Free-form curves and surfaces.
//!
//! Everything else in this kernel is a few numbers: a kind, a frame, two or
//! three scalars. A spline is as many control points as it happens to need,
//! and a `Surface` or `Curve` that owned them could no longer be copied
//! about as plain data. So the points live here instead. A definition is
//! written once, never changed and never freed, and what the plain data
//! carries is a reference to it: one word, as cheap to copy as a radius.
//! Asking twice for the same definition gives the same reference, so two
//! references are equal exactly when they are the same spline.
//!
//! The splines are non-uniform rational B-splines of any degree up to
//! [`MAX_DEGREE`], clamped at both ends. Three things fit them to the rest
//! of the kernel:
//!
//! * One that closes on itself smoothly has its parameter rescaled to run
//!   from `-π` to `π`. It then behaves as every other angle here does: it
//!   comes round again, with the period everything already expects.
//! * Past the ends of an open direction a spline carries on straight, along
//!   its end tangent. A point just off the edge of a patch still has a
//!   nearest point and a distance, and a search that steps over the edge
//!   comes back instead of falling off.
//! * Each keeps a coarse net of its own points. The nearest of those is
//!   where the search for the nearest point of the spline starts, which is
//!   what makes "where on this surface is this point" a question with one
//!   answer rather than whatever a blind start converges to.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Mutex, OnceLock};

use crate::math::*;

/// The highest degree a spline may have.
pub const MAX_DEGREE: usize = 15;
const ORDER: usize = MAX_DEGREE + 1;

/// A control point with its weight: `[w·x, w·y, w·z, w]`.
pub type Hom = [f64; 4];

#[inline]
fn add(sum: &mut Hom, h: &Hom, k: f64) {
    sum[0] += h[0] * k;
    sum[1] += h[1] * k;
    sum[2] += h[2] * k;
    sum[3] += h[3] * k;
}

#[inline]
fn xyz(h: &Hom) -> V3 {
    v3(h[0], h[1], h[2])
}

fn lift(p: V3, w: f64) -> Hom {
    [p.x * w, p.y * w, p.z * w, w]
}

/// The knot span that holds `t`: the last `k` with `knots[k] <= t`, kept
/// to the spans that carry control points of their own.
fn span(knots: &[f64], degree: usize, t: f64) -> usize {
    let last = knots.len() - degree - 2;
    if t >= knots[last + 1] {
        // The far end belongs to the last span that is not empty.
        let mut k = last;
        while k > degree && knots[k] >= knots[k + 1] {
            k -= 1;
        }
        return k;
    }
    let (mut lo, mut hi) = (degree, last + 1);
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if t < knots[mid] {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    lo
}

/// The `q + 1` B-spline functions of degree `q` that are not zero on span
/// `k`, at `t`: `out[j]` is the one that starts at knot `k - q + j`.
fn values(knots: &[f64], q: usize, k: usize, t: f64, out: &mut [f64; ORDER]) {
    let mut left = [0.0; ORDER];
    let mut right = [0.0; ORDER];
    out[0] = 1.0;
    for j in 1..=q {
        left[j] = t - knots[k + 1 - j];
        right[j] = knots[k + j] - t;
        let mut saved = 0.0;
        for r in 0..j {
            let temp = out[r] / (right[r + 1] + left[j - r]);
            out[r] = saved + right[r + 1] * temp;
            saved = left[j - r] * temp;
        }
        out[j] = saved;
    }
}

/// The functions of one direction at one parameter: the span they belong
/// to, their values and as many derivatives as were asked for.
struct Basis {
    k: usize,
    n: [[f64; ORDER]; 3],
}

/// Values and the first `order` derivatives (two at most) of the functions
/// of degree `p` at `t`. A derivative of a function of degree `p` is a
/// difference of two of degree `p - 1`, each over the width of its support.
fn basis(knots: &[f64], p: usize, t: f64, order: usize) -> Basis {
    let k = span(knots, p, t);
    let mut out = Basis {
        k,
        n: [[0.0; ORDER]; 3],
    };
    values(knots, p, k, t, &mut out.n[0]);
    if order == 0 || p == 0 {
        return out;
    }
    let mut lower = [0.0; ORDER];
    values(knots, p - 1, k, t, &mut lower);
    // The function of degree `p - 1` that starts at knot `k - p + 1 + l` is
    // `lower[l]`; over its support, it is what the derivative is made of.
    let over = |from: usize, l: usize, width: usize, f: &[f64; ORDER]| {
        f[l] / (knots[from + l + width] - knots[from + l])
    };
    for j in 0..=p {
        let mut d = 0.0;
        if j >= 1 {
            d += over(k + 1 - p, j - 1, p, &lower);
        }
        if j < p {
            d -= over(k + 1 - p, j, p, &lower);
        }
        out.n[1][j] = p as f64 * d;
    }
    if order == 1 || p == 1 {
        return out;
    }
    let mut lowest = [0.0; ORDER];
    values(knots, p - 2, k, t, &mut lowest);
    // First derivatives of the functions of degree `p - 1`.
    let mut slope = [0.0; ORDER];
    for (l, s) in slope.iter_mut().enumerate().take(p) {
        let mut d = 0.0;
        if l >= 1 {
            d += over(k + 2 - p, l - 1, p - 1, &lowest);
        }
        if l + 1 < p {
            d -= over(k + 2 - p, l, p - 1, &lowest);
        }
        *s = (p - 1) as f64 * d;
    }
    for j in 0..=p {
        let mut d = 0.0;
        if j >= 1 {
            d += over(k + 1 - p, j - 1, p, &slope);
        }
        if j < p {
            d -= over(k + 1 - p, j, p, &slope);
        }
        out.n[2][j] = p as f64 * d;
    }
    out
}

/// The functions of degree `degree` that are not zero at `t`: the index of
/// the first of them, and their values.
pub fn functions(knots: &[f64], degree: usize, t: f64) -> (usize, Vec<f64>) {
    let b = basis(knots, degree, t, 0);
    (b.k - degree, b.n[0][..=degree].to_vec())
}

/// Knots for a curve of `degree` through points at the parameters `at`,
/// which must increase: clamped, and inside each the mean of `degree`
/// parameters in a row, so that every point has a function of its own.
pub fn knots_through(at: &[f64], degree: usize) -> Vec<f64> {
    let n = at.len();
    let mut knots = vec![at[0]; degree + 1];
    for j in 1..n - degree {
        knots.push(at[j..j + degree].iter().sum::<f64>() / degree as f64);
    }
    knots.extend(std::iter::repeat_n(at[n - 1], degree + 1));
    knots
}

/// The control points of the curve of `degree` on `knots` that is at
/// `through[r]` when its parameter is `at[r]`. Weighted points are found
/// component by component, weight included.
pub fn through(
    knots: &[f64],
    degree: usize,
    at: &[f64],
    through: &[Hom],
) -> Result<Vec<Hom>, String> {
    let n = at.len();
    let mut rows = vec![vec![0.0; n]; n];
    for (r, &t) in at.iter().enumerate() {
        let (first, values) = functions(knots, degree, t);
        for (l, v) in values.iter().enumerate() {
            rows[r][first + l] = *v;
        }
    }
    let mut rhs: Vec<Hom> = through.to_vec();
    // Elimination with the largest pivot of each column.
    for c in 0..n {
        let pivot = (c..n)
            .max_by(|&a, &b| rows[a][c].abs().total_cmp(&rows[b][c].abs()))
            .unwrap();
        if rows[pivot][c].abs() < 1.0e-12 {
            return Err("the sections of a loft cannot be joined smoothly".into());
        }
        rows.swap(c, pivot);
        rhs.swap(c, pivot);
        for r in c + 1..n {
            let k = rows[r][c] / rows[c][c];
            if k != 0.0 {
                let (above, below) = rows.split_at_mut(r);
                for (x, pivot) in below[0][c..].iter_mut().zip(&above[c][c..]) {
                    *x -= k * pivot;
                }
                let pivot = rhs[c];
                for (x, p) in rhs[r].iter_mut().zip(&pivot) {
                    *x -= k * p;
                }
            }
        }
    }
    for c in (0..n).rev() {
        let mut solved = rhs[c];
        for (j, later) in rhs.iter().enumerate().skip(c + 1) {
            for (x, l) in solved.iter_mut().zip(later) {
                *x -= rows[c][j] * l;
            }
        }
        rhs[c] = solved.map(|x| x / rows[c][c]);
    }
    Ok(rhs)
}

/// What a knot vector has to be: long enough, never decreasing, clamped at
/// both ends and with no knot repeated more often than the degree allows.
fn check_knots(knots: &[f64], degree: usize, count: usize) -> Result<(), String> {
    if degree == 0 || degree > MAX_DEGREE {
        return Err(format!("a spline of degree {degree} is not supported"));
    }
    if count <= degree {
        return Err("a spline needs more control points than its degree".into());
    }
    if knots.len() != count + degree + 1 {
        return Err("a spline's knots do not match its control points".into());
    }
    if knots.iter().any(|k| !k.is_finite()) || knots.windows(2).any(|w| w[1] < w[0]) {
        return Err("a spline's knots must not decrease".into());
    }
    let (first, last) = (knots[0], knots[knots.len() - 1]);
    if knots[degree] != first || knots[count] != last || last <= first {
        return Err("a spline's knots must be clamped at both ends".into());
    }
    for k in degree + 1..count {
        if knots[k + degree] <= knots[k] {
            return Err("a spline's knot is repeated more often than its degree".into());
        }
    }
    Ok(())
}

/// Move a knot vector so that it runs from `lo` to `hi`.
fn rescale(knots: &mut [f64], lo: f64, hi: f64) {
    let (a, b) = (knots[0], knots[knots.len() - 1]);
    let last = knots.len() - 1;
    for (i, k) in knots.iter_mut().enumerate() {
        // The clamped ends must come out exactly equal to each other.
        *k = if *k == a {
            lo
        } else if *k == b || i == last {
            hi
        } else {
            lo + (*k - a) / (b - a) * (hi - lo)
        };
    }
}

/// Insert the knot `t` once more. The curve on `row` does not change; it
/// gains a control point.
fn insert_knot(knots: &mut Vec<f64>, degree: usize, row: &mut Vec<Hom>, t: f64) {
    // The last knot not beyond `t`.
    let k = knots.iter().rposition(|&k| k <= t).unwrap_or(0);
    let mut out = Vec::with_capacity(row.len() + 1);
    for i in 0..=row.len() {
        if i + degree <= k {
            out.push(row[i]);
        } else if i > k || knots[i] >= t {
            // Beyond the new knot, or at a copy of it that was there before.
            out.push(row[i - 1]);
        } else {
            let a = (t - knots[i]) / (knots[i + degree] - knots[i]);
            let mut h = [0.0; 4];
            add(&mut h, &row[i - 1], 1.0 - a);
            add(&mut h, &row[i], a);
            out.push(h);
        }
    }
    *row = out;
    knots.insert(k + 1, t);
}

/// Clamp a knot vector that is not: insert knots at both ends of the part
/// the curve is defined on until each is there `degree` times, and drop
/// what lies outside. The rows of a surface are clamped one by one, each
/// against the same knots.
pub fn clamp(knots: &[f64], degree: usize, rows: &mut [Vec<Hom>]) -> Result<Vec<f64>, String> {
    let count = rows.first().map_or(0, Vec::len);
    if degree == 0 || count <= degree || knots.len() != count + degree + 1 {
        return Err("a spline's knots do not match its control points".into());
    }
    if knots.windows(2).any(|w| w[1] < w[0]) {
        return Err("a spline's knots must not decrease".into());
    }
    let (start, end) = (knots[degree], knots[count]);
    if end <= start {
        return Err("a spline's knots must increase".into());
    }
    let mut result = Vec::new();
    for row in rows.iter_mut() {
        let mut ks = knots.to_vec();
        for t in [start, end] {
            let have = ks.iter().filter(|&&k| k == t).count();
            for _ in have..degree {
                insert_knot(&mut ks, degree, row, t);
            }
        }
        // What is left is the knots from the first `start` to the last
        // `end`, with one more of each, and the points that go with them.
        let first = ks.iter().position(|&k| k == start).unwrap();
        let last = ks.iter().rposition(|&k| k == end).unwrap();
        if first == 0 || last < degree || last - degree < first - 1 {
            return Err("a spline's knots are repeated too often to clamp".into());
        }
        *row = row[first - 1..=last - degree].to_vec();
        result = vec![start];
        result.extend_from_slice(&ks[first..=last]);
        result.push(end);
    }
    Ok(result)
}

// ---------------------------------------------------------------- curves

/// A spline curve. See the module for what the parameter does at and
/// beyond its ends.
#[derive(Debug)]
pub struct SplineCurve {
    pub degree: usize,
    pub knots: Vec<f64>,
    pub hom: Vec<Hom>,
    pub rational: bool,
    /// It closes smoothly: the parameter is an angle from `-π` to `π`.
    pub closed: bool,
    pub lo: f64,
    pub hi: f64,
    pub bbox: Aabb,
    /// The largest second derivative and the least speed found along it.
    pub bend: f64,
    pub slowest: f64,
    /// Points along it, evenly spaced in the parameter from `lo` to `hi`.
    seeds: Vec<V3>,
}

/// A reference to a spline curve: equal exactly when it is the same one.
#[derive(Clone, Copy)]
pub struct CurveForm(&'static SplineCurve);

impl CurveForm {
    /// The definition itself, for as long as the program runs.
    #[inline]
    pub fn get(self) -> &'static SplineCurve {
        self.0
    }
}

impl PartialEq for CurveForm {
    fn eq(&self, o: &CurveForm) -> bool {
        std::ptr::eq(self.0, o.0)
    }
}

impl std::ops::Deref for CurveForm {
    type Target = SplineCurve;
    fn deref(&self) -> &SplineCurve {
        self.0
    }
}

impl std::fmt::Debug for CurveForm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "spline curve of degree {} on {} points",
            self.degree,
            self.hom.len()
        )
    }
}

fn hash_numbers(parts: &[&[f64]], sizes: &[usize]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    sizes.hash(&mut h);
    for part in parts {
        for x in *part {
            // Negative zero is the same number as zero.
            (if *x == 0.0 { 0.0f64 } else { *x }).to_bits().hash(&mut h);
        }
    }
    h.finish()
}

fn flat(hom: &[Hom]) -> &[f64] {
    hom.as_flattened()
}

impl SplineCurve {
    /// The curve of `degree` on `points`, each with its weight if `weights`
    /// is not empty. `knots` are written out in full, repeats included.
    pub fn on(
        degree: usize,
        knots: &[f64],
        points: &[V3],
        weights: &[f64],
    ) -> Result<CurveForm, String> {
        if !weights.is_empty() && weights.len() != points.len() {
            return Err("a spline needs one weight for each control point".into());
        }
        if weights.iter().any(|&w| !(w > 0.0 && w.is_finite())) {
            return Err("a spline's weights must be positive".into());
        }
        let hom: Vec<Hom> = points
            .iter()
            .enumerate()
            .map(|(i, &p)| lift(p, weights.get(i).copied().unwrap_or(1.0)))
            .collect();
        SplineCurve::from_hom(degree, knots.to_vec(), hom)
    }

    /// The same from weighted control points.
    pub fn from_hom(
        degree: usize,
        mut knots: Vec<f64>,
        hom: Vec<Hom>,
    ) -> Result<CurveForm, String> {
        check_knots(&knots, degree, hom.len())?;
        if hom.iter().flatten().any(|x| !x.is_finite()) {
            return Err("a spline's control points must be finite".into());
        }
        let rational = hom
            .iter()
            .any(|h| (h[3] - hom[0][3]).abs() > 1.0e-14 * hom[0][3]);
        let mut bbox = Aabb::EMPTY;
        for h in &hom {
            bbox.add(xyz(h) / h[3]);
        }
        let size = bbox.diagonal().max(1.0e-12);
        let mut curve = SplineCurve {
            degree,
            lo: knots[0],
            hi: knots[knots.len() - 1],
            knots: Vec::new(),
            hom,
            rational,
            closed: false,
            bbox,
            bend: 0.0,
            slowest: 0.0,
            seeds: Vec::new(),
        };
        // Closed: the ends are one point, left and arrived at the same way.
        curve.knots = knots.clone();
        let (start, end) = (curve.inside(curve.lo, 1), curve.inside(curve.hi, 1));
        if start[0].dist(end[0]) < 1.0e-11 * size
            && start[1].len() > 0.0
            && start[1].unit().dist(end[1].unit()) < 1.0e-7
        {
            curve.closed = true;
            rescale(&mut knots, -PI, PI);
            curve.knots = knots;
            (curve.lo, curve.hi) = (-PI, PI);
        }
        let spans = curve.knots.windows(2).filter(|w| w[1] > w[0]).count();
        let steps = (if degree == 1 { 2 } else { 6 } * spans).clamp(8, 768);
        let (mut bend, mut slowest) = (0.0f64, f64::INFINITY);
        for i in 0..=steps {
            let t = curve.lo + (curve.hi - curve.lo) * i as f64 / steps as f64;
            let d = curve.inside(t, 2);
            curve.seeds.push(d[0]);
            bend = bend.max(d[2].len());
            slowest = slowest.min(d[1].len());
        }
        (curve.bend, curve.slowest) = (bend, slowest);
        Ok(CurveForm(intern_curve(curve)))
    }

    /// Control points without their weights.
    pub fn points(&self) -> Vec<V3> {
        self.hom.iter().map(|h| xyz(h) / h[3]).collect()
    }

    pub fn weights(&self) -> Vec<f64> {
        self.hom.iter().map(|h| h[3]).collect()
    }

    /// Point and two derivatives at a parameter within the knots.
    fn inside(&self, t: f64, order: usize) -> [V3; 3] {
        let b = basis(&self.knots, self.degree, t, order);
        let first = b.k - self.degree;
        let mut sum = [[0.0; 4]; 3];
        for (j, h) in self.hom[first..=b.k].iter().enumerate() {
            for (d, s) in sum.iter_mut().enumerate().take(order + 1) {
                add(s, h, b.n[d][j]);
            }
        }
        let w = sum[0][3];
        let c = xyz(&sum[0]) / w;
        let c1 = (xyz(&sum[1]) - c * sum[1][3]) / w;
        let c2 = (xyz(&sum[2]) - c1 * (2.0 * sum[1][3]) - c * sum[2][3]) / w;
        [c, c1, c2]
    }

    /// Point, first and second derivative at `t`, as many as `order` asks
    /// for. Round a closed curve `t` is an angle; past the ends of an open
    /// one the curve carries on along its end tangent.
    pub fn derivs(&self, t: f64, order: usize) -> [V3; 3] {
        if self.closed {
            return self.inside(wrap_from(t, -PI).min(PI), order);
        }
        if t >= self.lo && t <= self.hi {
            return self.inside(t, order);
        }
        let end = t.clamp(self.lo, self.hi);
        let d = self.inside(end, 1);
        [d[0] + d[1] * (t - end), d[1], V3::ZERO]
    }

    #[inline]
    pub fn point(&self, t: f64) -> V3 {
        self.derivs(t, 0)[0]
    }

    /// Parameter of the point of the curve nearest to `p`.
    pub fn nearest(&self, p: V3) -> f64 {
        let steps = self.seeds.len() - 1;
        let mut best = (f64::INFINITY, 0);
        for (i, s) in self.seeds.iter().enumerate() {
            let d = (*s - p).len2();
            if d < best.0 {
                best = (d, i);
            }
        }
        let width = (self.hi - self.lo) / steps as f64;
        let mut t = self.lo + width * best.1 as f64;
        let mut away = best.0;
        for _ in 0..32 {
            let [c, c1, c2] = self.derivs(t, 2);
            let d = c - p;
            let slope = d.dot(c1);
            let speed2 = c1.dot(c1);
            if speed2 < 1.0e-300 {
                break;
            }
            let curved = speed2 + d.dot(c2);
            let mut step = -slope
                / if curved > 0.25 * speed2 {
                    curved
                } else {
                    speed2
                };
            step = step.clamp(-2.0 * width, 2.0 * width);
            // Never further away than before.
            let mut moved = false;
            for _ in 0..8 {
                let to = self.point(t + step);
                let d2 = (to - p).len2();
                if d2 <= away * (1.0 + 1.0e-14) + 1.0e-300 {
                    t += step;
                    away = d2;
                    moved = true;
                    break;
                }
                step *= 0.5;
            }
            if !moved || step.abs() * speed2.sqrt() < 1.0e-15 * (1.0 + p.len()) {
                break;
            }
        }
        if self.closed {
            wrap_from(t, -PI)
        } else {
            t
        }
    }
}

// -------------------------------------------------------------- surfaces

/// How many seed intervals each way a block of the net covers.
const BLOCK: usize = 4;

/// A spline surface. Control point `i` along `u` and `j` along `v` is
/// `hom[i + count[0] * j]`.
#[derive(Debug)]
pub struct SplineSurface {
    pub degree: [usize; 2],
    pub knots: [Vec<f64>; 2],
    pub count: [usize; 2],
    pub hom: Vec<Hom>,
    pub rational: bool,
    /// Per direction: it closes smoothly, and its parameter is an angle.
    pub closed: [bool; 2],
    pub lo: V2,
    pub hi: V2,
    pub bbox: Aabb,
    /// The tightest radius found that the surface bends with along either
    /// parameter; infinite if it is flat both ways.
    pub radius: f64,
    /// Seed intervals each way, and the points at their corners.
    grid: [usize; 2],
    seeds: Vec<V3>,
    /// A box round the seeds of each block of intervals.
    blocks: Vec<Aabb>,
}

/// A reference to a spline surface: equal exactly when it is the same one.
#[derive(Clone, Copy)]
pub struct SurfForm(&'static SplineSurface);

impl SurfForm {
    /// The definition itself, for as long as the program runs.
    #[inline]
    pub fn get(self) -> &'static SplineSurface {
        self.0
    }
}

impl PartialEq for SurfForm {
    fn eq(&self, o: &SurfForm) -> bool {
        std::ptr::eq(self.0, o.0)
    }
}

impl std::ops::Deref for SurfForm {
    type Target = SplineSurface;
    fn deref(&self) -> &SplineSurface {
        self.0
    }
}

impl std::fmt::Debug for SurfForm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "spline surface of degree {}x{} on {}x{} points",
            self.degree[0], self.degree[1], self.count[0], self.count[1]
        )
    }
}

/// How much a surface bends over a patch of its parameters: the largest
/// second derivatives each way and across, and the least first ones.
#[derive(Clone, Copy, Debug)]
pub struct Bend {
    pub uu: f64,
    pub uv: f64,
    pub vv: f64,
    pub slowest: V2,
    pub fastest: V2,
}

impl SplineSurface {
    /// The surface of `degree` on a net of `count[0]` by `count[1]` control
    /// points, `points[i + count[0] * j]`, each with its weight if `weights`
    /// is not empty. Knots are written out in full, repeats included.
    pub fn on(
        degree: [usize; 2],
        knots: [&[f64]; 2],
        count: [usize; 2],
        points: &[V3],
        weights: &[f64],
    ) -> Result<SurfForm, String> {
        if points.len() != count[0] * count[1] {
            return Err("a spline surface's control points do not make a net".into());
        }
        if !weights.is_empty() && weights.len() != points.len() {
            return Err("a spline needs one weight for each control point".into());
        }
        if weights.iter().any(|&w| !(w > 0.0 && w.is_finite())) {
            return Err("a spline's weights must be positive".into());
        }
        let hom: Vec<Hom> = points
            .iter()
            .enumerate()
            .map(|(i, &p)| lift(p, weights.get(i).copied().unwrap_or(1.0)))
            .collect();
        SplineSurface::from_hom(degree, [knots[0].to_vec(), knots[1].to_vec()], count, hom)
    }

    /// The same from weighted control points.
    pub fn from_hom(
        degree: [usize; 2],
        mut knots: [Vec<f64>; 2],
        count: [usize; 2],
        hom: Vec<Hom>,
    ) -> Result<SurfForm, String> {
        check_knots(&knots[0], degree[0], count[0])?;
        check_knots(&knots[1], degree[1], count[1])?;
        if hom.len() != count[0] * count[1] {
            return Err("a spline surface's control points do not make a net".into());
        }
        if hom.iter().flatten().any(|x| !x.is_finite()) {
            return Err("a spline's control points must be finite".into());
        }
        let rational = hom
            .iter()
            .any(|h| (h[3] - hom[0][3]).abs() > 1.0e-14 * hom[0][3]);
        let mut bbox = Aabb::EMPTY;
        for h in &hom {
            bbox.add(xyz(h) / h[3]);
        }
        let size = bbox.diagonal().max(1.0e-12);
        let mut s = SplineSurface {
            degree,
            lo: v2(knots[0][0], knots[1][0]),
            hi: v2(knots[0][knots[0].len() - 1], knots[1][knots[1].len() - 1]),
            knots: knots.clone(),
            count,
            hom,
            rational,
            closed: [false; 2],
            bbox,
            radius: f64::INFINITY,
            grid: [0; 2],
            seeds: Vec::new(),
            blocks: Vec::new(),
        };
        // Closed one way: the two ends are one curve, left and arrived at
        // in the same direction all along it.
        for dir in 0..2 {
            let other = 1 - dir;
            let closes = (0..=8).all(|i| {
                let along = if other == 0 { s.lo.x } else { s.lo.y }
                    + if other == 0 {
                        s.hi.x - s.lo.x
                    } else {
                        s.hi.y - s.lo.y
                    } * i as f64
                        / 8.0;
                let at = |end: f64| {
                    let (u, v) = if dir == 0 { (end, along) } else { (along, end) };
                    let d = s.inside(u, v, 1);
                    (d[0], d[1 + dir])
                };
                let (a, b) = if dir == 0 {
                    (s.lo.x, s.hi.x)
                } else {
                    (s.lo.y, s.hi.y)
                };
                let ((p0, t0), (p1, t1)) = (at(a), at(b));
                p0.dist(p1) < 1.0e-11 * size && t0.len() > 0.0 && t0.unit().dist(t1.unit()) < 1.0e-7
            });
            if closes {
                s.closed[dir] = true;
                rescale(&mut knots[dir], -PI, PI);
            }
        }
        s.lo = v2(knots[0][0], knots[1][0]);
        s.hi = v2(knots[0][knots[0].len() - 1], knots[1][knots[1].len() - 1]);
        s.knots = knots;
        // The net of seeds: fine enough that the nearest of them is on the
        // same hill as the nearest point of the surface.
        for (dir, cells) in s.grid.iter_mut().enumerate() {
            let spans = s.knots[dir].windows(2).filter(|w| w[1] > w[0]).count();
            *cells = (if degree[dir] == 1 { 1 } else { 3 } * spans).clamp(4, 96);
        }
        let [gu, gv] = s.grid;
        for j in 0..=gv {
            for i in 0..=gu {
                let u = s.lo.x + (s.hi.x - s.lo.x) * i as f64 / gu as f64;
                let v = s.lo.y + (s.hi.y - s.lo.y) * j as f64 / gv as f64;
                let d = s.inside(u, v, 2);
                s.seeds.push(d[0]);
                // A curve of speed `c'` and second derivative `c''` bends
                // no tighter than `|c'|² / |c''|`.
                for (first, second) in [(d[1], d[3]), (d[2], d[5])] {
                    if second.len() > 1.0e-12 * first.len2() {
                        s.radius = s.radius.min(first.len2() / second.len());
                    }
                }
            }
        }
        if s.radius.is_nan() || s.radius <= 0.0 {
            return Err("a spline surface is folded to a point".into());
        }
        let (bu, bv) = (gu.div_ceil(BLOCK), gv.div_ceil(BLOCK));
        for cj in 0..bv {
            for ci in 0..bu {
                let mut b = Aabb::EMPTY;
                for j in cj * BLOCK..=((cj + 1) * BLOCK).min(gv) {
                    for i in ci * BLOCK..=((ci + 1) * BLOCK).min(gu) {
                        b.add(s.seeds[i + (gu + 1) * j]);
                    }
                }
                s.blocks.push(b);
            }
        }
        Ok(SurfForm(intern_surface(s)))
    }

    /// Control points without their weights.
    pub fn points(&self) -> Vec<V3> {
        self.hom.iter().map(|h| xyz(h) / h[3]).collect()
    }

    pub fn weights(&self) -> Vec<f64> {
        self.hom.iter().map(|h| h[3]).collect()
    }

    /// Point and derivatives at parameters within the knots:
    /// `[S, Su, Sv, Suu, Suv, Svv]`, as far as `order` asks.
    fn inside(&self, u: f64, v: f64, order: usize) -> [V3; 6] {
        let bu = basis(&self.knots[0], self.degree[0], u, order);
        let bv = basis(&self.knots[1], self.degree[1], v, order);
        let (p, q) = (self.degree[0], self.degree[1]);
        let (i0, j0) = (bu.k - p, bv.k - q);
        let mut sum = [[0.0; 4]; 6];
        for j in 0..=q {
            let start = (j0 + j) * self.count[0] + i0;
            let mut row = [[0.0; 4]; 3];
            for (i, h) in self.hom[start..=start + p].iter().enumerate() {
                for (d, r) in row.iter_mut().enumerate().take(order + 1) {
                    add(r, h, bu.n[d][i]);
                }
            }
            add(&mut sum[0], &row[0], bv.n[0][j]);
            if order >= 1 {
                add(&mut sum[1], &row[1], bv.n[0][j]);
                add(&mut sum[2], &row[0], bv.n[1][j]);
            }
            if order >= 2 {
                add(&mut sum[3], &row[2], bv.n[0][j]);
                add(&mut sum[4], &row[1], bv.n[1][j]);
                add(&mut sum[5], &row[0], bv.n[2][j]);
            }
        }
        let w = sum[0][3];
        let s = xyz(&sum[0]) / w;
        if order == 0 {
            return [s, V3::ZERO, V3::ZERO, V3::ZERO, V3::ZERO, V3::ZERO];
        }
        let su = (xyz(&sum[1]) - s * sum[1][3]) / w;
        let sv = (xyz(&sum[2]) - s * sum[2][3]) / w;
        if order == 1 {
            return [s, su, sv, V3::ZERO, V3::ZERO, V3::ZERO];
        }
        let suu = (xyz(&sum[3]) - su * (2.0 * sum[1][3]) - s * sum[3][3]) / w;
        let suv = (xyz(&sum[4]) - sv * sum[1][3] - su * sum[2][3] - s * sum[4][3]) / w;
        let svv = (xyz(&sum[5]) - sv * (2.0 * sum[2][3]) - s * sum[5][3]) / w;
        [s, su, sv, suu, suv, svv]
    }

    /// A parameter brought within the knots, and how far it was outside.
    #[inline]
    fn within(&self, dir: usize, t: f64) -> (f64, f64) {
        let (lo, hi) = if dir == 0 {
            (self.lo.x, self.hi.x)
        } else {
            (self.lo.y, self.hi.y)
        };
        if self.closed[dir] {
            (wrap_from(t, -PI).min(PI), 0.0)
        } else {
            let c = t.clamp(lo, hi);
            (c, t - c)
        }
    }

    /// Point and derivatives at `(u, v)`: `[S, Su, Sv, Suu, Suv, Svv]`, as
    /// far as `order` asks. A closed direction comes round; an open one
    /// carries on straight past its ends.
    pub fn derivs(&self, u: f64, v: f64, order: usize) -> [V3; 6] {
        let ((uc, du), (vc, dv)) = (self.within(0, u), self.within(1, v));
        if du == 0.0 && dv == 0.0 {
            return self.inside(uc, vc, order);
        }
        let [s, su, sv, suu, suv, svv] = self.inside(uc, vc, 2);
        [
            s + su * du + sv * dv + suv * (du * dv),
            su + suv * dv,
            sv + suv * du,
            if du != 0.0 { V3::ZERO } else { suu },
            suv,
            if dv != 0.0 { V3::ZERO } else { svv },
        ]
    }

    #[inline]
    pub fn point(&self, u: f64, v: f64) -> V3 {
        self.derivs(u, v, 0)[0]
    }

    /// The seed nearest to `p`, as parameters.
    fn seed(&self, p: V3) -> V2 {
        let [gu, gv] = self.grid;
        let bu = gu.div_ceil(BLOCK);
        let mut best = (f64::INFINITY, 0usize);
        let scan = |block: usize, best: &mut (f64, usize)| {
            let (ci, cj) = (block % bu, block / bu);
            for j in cj * BLOCK..=((cj + 1) * BLOCK).min(gv) {
                for i in ci * BLOCK..=((ci + 1) * BLOCK).min(gu) {
                    let at = i + (gu + 1) * j;
                    let d = (self.seeds[at] - p).len2();
                    if d < best.0 {
                        *best = (d, at);
                    }
                }
            }
        };
        // The block whose box is nearest first: after it, few others can
        // still hold anything nearer.
        let mut first = (f64::INFINITY, 0);
        for (b, bx) in self.blocks.iter().enumerate() {
            let d = bx.distance(p);
            if d < first.0 {
                first = (d, b);
            }
        }
        scan(first.1, &mut best);
        for (b, bx) in self.blocks.iter().enumerate() {
            if b != first.1 {
                let d = bx.distance(p);
                if d * d < best.0 {
                    scan(b, &mut best);
                }
            }
        }
        let (i, j) = (best.1 % (gu + 1), best.1 / (gu + 1));
        v2(
            self.lo.x + (self.hi.x - self.lo.x) * i as f64 / gu as f64,
            self.lo.y + (self.hi.y - self.lo.y) * j as f64 / gv as f64,
        )
    }

    /// Parameters of the point of the surface nearest to `p`. In a closed
    /// direction the answer is an angle in `[-π, π)`; in an open one it can
    /// lie past the end, on the straight continuation.
    pub fn nearest(&self, p: V3) -> V2 {
        self.nearest_from(p, self.seed(p))
    }

    /// The same, started from parameters already known to be close.
    pub fn nearest_from(&self, p: V3, start: V2) -> V2 {
        let reach = v2(
            (self.hi.x - self.lo.x) / self.grid[0] as f64,
            (self.hi.y - self.lo.y) / self.grid[1] as f64,
        );
        let mut q = start;
        let mut away = (self.point(q.x, q.y) - p).len2();
        for _ in 0..40 {
            let [s, su, sv, suu, suv, svv] = self.derivs(q.x, q.y, 2);
            let d = s - p;
            let g = v2(su.dot(d), sv.dot(d));
            let (a, b, c) = (su.dot(su), su.dot(sv), sv.dot(sv));
            if a < 1.0e-300 || c < 1.0e-300 {
                break;
            }
            // Newton where the distance is cupped, the same without the
            // surface's own bend where it is not, and each parameter on its
            // own where the two run together.
            let (h11, h12, h22) = (a + d.dot(suu), b + d.dot(suv), c + d.dot(svv));
            let full = h11 * h22 - h12 * h12;
            let flat = a * c - b * b;
            let mut step = if h11 > 0.0 && full > 1.0e-9 * a * c {
                v2(
                    -(h22 * g.x - h12 * g.y) / full,
                    -(h11 * g.y - h12 * g.x) / full,
                )
            } else if flat > 1.0e-12 * a * c {
                v2(-(c * g.x - b * g.y) / flat, -(a * g.y - b * g.x) / flat)
            } else {
                v2(-g.x / a, -g.y / c)
            };
            let over = (step.x.abs() / (2.0 * reach.x)).max(step.y.abs() / (2.0 * reach.y));
            if over > 1.0 {
                step = step * (1.0 / over);
            }
            // Never further away than before.
            let mut moved = false;
            for _ in 0..10 {
                let to = q + step;
                let d2 = (self.point(to.x, to.y) - p).len2();
                if d2 <= away * (1.0 + 1.0e-14) + 1.0e-300 {
                    q = to;
                    away = d2;
                    moved = true;
                    break;
                }
                step = step * 0.5;
            }
            let length = (su * step.x + sv * step.y).len();
            if !moved || length < 1.0e-15 * (1.0 + p.len()) {
                break;
            }
        }
        self.canonical(q)
    }

    /// Angles of closed directions brought into `[-π, π)`.
    fn canonical(&self, q: V2) -> V2 {
        v2(
            if self.closed[0] {
                wrap_from(q.x, -PI)
            } else {
                q.x
            },
            if self.closed[1] {
                wrap_from(q.y, -PI)
            } else {
                q.y
            },
        )
    }

    /// The part of the parameter rectangle that can come within `region`,
    /// given in the spline's own coordinates: the corners of every seed
    /// interval that has a corner in the region, or within an interval's
    /// width of it. `None` when no part of the surface is near.
    pub fn window(&self, region: &Aabb) -> Option<(V2, V2)> {
        let [gu, gv] = self.grid;
        // How far apart neighbouring seeds get.
        let mut mesh = 0.0f64;
        for j in 0..=gv {
            for i in 0..=gu {
                let at = i + (gu + 1) * j;
                if i < gu {
                    mesh = mesh.max(self.seeds[at].dist(self.seeds[at + 1]));
                }
                if j < gv {
                    mesh = mesh.max(self.seeds[at].dist(self.seeds[at + gu + 1]));
                }
            }
        }
        let near = region.grown(mesh);
        let (mut i0, mut i1, mut j0, mut j1) = (usize::MAX, 0, usize::MAX, 0);
        for j in 0..=gv {
            for i in 0..=gu {
                if near.contains(self.seeds[i + (gu + 1) * j]) {
                    (i0, i1) = (i0.min(i), i1.max(i));
                    (j0, j1) = (j0.min(j), j1.max(j));
                }
            }
        }
        if i0 == usize::MAX {
            return None;
        }
        let at = |i: usize, n: usize, lo: f64, hi: f64| lo + (hi - lo) * i as f64 / n as f64;
        Some((
            v2(
                at(i0.saturating_sub(1), gu, self.lo.x, self.hi.x),
                at(j0.saturating_sub(1), gv, self.lo.y, self.hi.y),
            ),
            v2(
                at((i1 + 1).min(gu), gu, self.lo.x, self.hi.x),
                at((j1 + 1).min(gv), gv, self.lo.y, self.hi.y),
            ),
        ))
    }

    /// How much the surface bends over the patch from `lo` to `hi`.
    pub fn bend(&self, lo: V2, hi: V2) -> Bend {
        const N: usize = 8;
        let mut out = Bend {
            uu: 0.0,
            uv: 0.0,
            vv: 0.0,
            slowest: v2(f64::INFINITY, f64::INFINITY),
            fastest: v2(0.0, 0.0),
        };
        for j in 0..=N {
            for i in 0..=N {
                // Off the knots, where a derivative may jump.
                let fu = (i as f64 + 0.013) / (N as f64 + 0.026);
                let fv = (j as f64 + 0.017) / (N as f64 + 0.034);
                let d = self.derivs(lo.x + (hi.x - lo.x) * fu, lo.y + (hi.y - lo.y) * fv, 2);
                out.uu = out.uu.max(d[3].len());
                out.uv = out.uv.max(d[4].len());
                out.vv = out.vv.max(d[5].len());
                out.slowest = v2(out.slowest.x.min(d[1].len()), out.slowest.y.min(d[2].len()));
                out.fastest = v2(out.fastest.x.max(d[1].len()), out.fastest.y.max(d[2].len()));
            }
        }
        out
    }

    /// The curve along which one parameter is constant: `dir` is the one
    /// held (`0` for `u`), `t` its value. The curve's own parameter is the
    /// surface's other one.
    pub fn iso(&self, dir: usize, t: f64) -> Result<CurveForm, String> {
        let (tc, _) = self.within(dir, t);
        let b = basis(&self.knots[dir], self.degree[dir], tc, 0);
        let first = b.k - self.degree[dir];
        let other = 1 - dir;
        let mut hom = vec![[0.0; 4]; self.count[other]];
        for (m, h) in hom.iter_mut().enumerate() {
            for l in 0..=self.degree[dir] {
                let at = if dir == 0 {
                    (first + l) + self.count[0] * m
                } else {
                    m + self.count[0] * (first + l)
                };
                add(h, &self.hom[at], b.n[0][l]);
            }
        }
        SplineCurve::from_hom(self.degree[other], self.knots[other].clone(), hom)
    }
}

// ----------------------------------------------------------------- store

struct Store {
    curves: HashMap<u64, Vec<&'static SplineCurve>>,
    surfaces: HashMap<u64, Vec<&'static SplineSurface>>,
}

fn store() -> &'static Mutex<Store> {
    static STORE: OnceLock<Mutex<Store>> = OnceLock::new();
    STORE.get_or_init(|| {
        Mutex::new(Store {
            curves: HashMap::new(),
            surfaces: HashMap::new(),
        })
    })
}

fn intern_curve(c: SplineCurve) -> &'static SplineCurve {
    let key = hash_numbers(&[&c.knots, flat(&c.hom)], &[c.degree, c.hom.len()]);
    let mut store = store().lock().unwrap_or_else(|e| e.into_inner());
    let same = store.curves.entry(key).or_default();
    if let Some(found) = same
        .iter()
        .find(|o| o.degree == c.degree && o.knots == c.knots && o.hom == c.hom)
    {
        return found;
    }
    let kept: &'static SplineCurve = Box::leak(Box::new(c));
    same.push(kept);
    kept
}

fn intern_surface(s: SplineSurface) -> &'static SplineSurface {
    let key = hash_numbers(
        &[&s.knots[0], &s.knots[1], flat(&s.hom)],
        &[s.degree[0], s.degree[1], s.count[0], s.count[1]],
    );
    let mut store = store().lock().unwrap_or_else(|e| e.into_inner());
    let same = store.surfaces.entry(key).or_default();
    if let Some(found) = same.iter().find(|o| {
        o.degree == s.degree && o.count == s.count && o.knots == s.knots && o.hom == s.hom
    }) {
        return found;
    }
    let kept: &'static SplineSurface = Box::leak(Box::new(s));
    same.push(kept);
    kept
}

// --------------------------------------------------------------- shapes

/// A straight piece from `a` to `b` as a curve of degree two, so that it
/// can stand beside arcs.
pub fn line_piece(a: V3, b: V3) -> [Hom; 3] {
    [lift(a, 1.0), lift((a + b) * 0.5, 1.0), lift(b, 1.0)]
}

/// An arc of less than half a turn from `a` to `b` about `centre`, exactly,
/// as a rational curve of degree two: its middle control point is where the
/// tangents at the two ends meet, weighted by the cosine of half the angle.
pub fn arc_piece(a: V3, b: V3, centre: V3) -> Result<[Hom; 3], String> {
    let (ra, rb) = (a - centre, b - centre);
    let half = (ra.unit() + rb.unit()) * 0.5;
    let cos = half.len();
    if !(cos > 0.05 && cos.is_finite()) {
        return Err("an arc piece must be less than half a turn".into());
    }
    // The tangents meet on the bisector, at radius / cos from the centre.
    let radius = 0.5 * (ra.len() + rb.len());
    let corner = centre + half.unit() * (radius / cos);
    Ok([lift(a, 1.0), lift(corner, cos), lift(b, 1.0)])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: V3, b: V3, tol: f64) -> bool {
        a.dist(b) <= tol
    }

    #[test]
    fn a_quarter_circle_is_exact() {
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let c = SplineCurve::on(
            2,
            &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            &[v3(5.0, 0.0, 1.0), v3(5.0, 5.0, 1.0), v3(0.0, 5.0, 1.0)],
            &[1.0, w, 1.0],
        )
        .unwrap();
        assert!(c.rational && !c.closed);
        for i in 0..=40 {
            let t = i as f64 / 40.0;
            let [p, d1, d2] = c.derivs(t, 2);
            let radial = p - v3(0.0, 0.0, 1.0);
            assert!((radial.len() - 5.0).abs() < 1.0e-14);
            assert!(radial.dot(d1).abs() < 1.0e-13);
            // Derivatives agree with differences of the points.
            let h = 1.0e-5;
            if i > 0 && i < 40 {
                let fd = (c.point(t + h) - c.point(t - h)) / (2.0 * h);
                assert!(near(fd, d1, 1.0e-8), "{fd:?} {d1:?}");
                let fd2 = (c.point(t + h) + c.point(t - h) - p * 2.0) / (h * h);
                assert!(near(fd2, d2, 1.0e-4), "{fd2:?} {d2:?}");
            }
            // A point off the curve finds its foot.
            let off = v3(0.0, 0.0, 1.0) + radial * 1.3 + v3(0.0, 0.0, 0.4);
            assert!((c.nearest(off) - t).abs() < 1.0e-10, "{t}");
        }
        // Past the ends it goes on straight.
        assert!(near(
            c.point(-0.5),
            v3(5.0, 0.0, 1.0) + c.derivs(0.0, 1)[1] * -0.5,
            1.0e-12
        ));
        // The same definition is the same curve.
        let again = SplineCurve::on(
            2,
            &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            &[v3(5.0, 0.0, 1.0), v3(5.0, 5.0, 1.0), v3(0.0, 5.0, 1.0)],
            &[1.0, w, 1.0],
        )
        .unwrap();
        assert!(c == again);
    }

    #[test]
    fn a_whole_circle_comes_round() {
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let r = 3.0;
        let ring = [
            (r, 0.0, 1.0),
            (r, r, w),
            (0.0, r, 1.0),
            (-r, r, w),
            (-r, 0.0, 1.0),
            (-r, -r, w),
            (0.0, -r, 1.0),
            (r, -r, w),
            (r, 0.0, 1.0),
        ];
        let points: Vec<V3> = ring.iter().map(|&(x, y, _)| v3(x, y, 0.0)).collect();
        let weights: Vec<f64> = ring.iter().map(|&(_, _, w)| w).collect();
        let knots = [0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0, 4.0];
        let c = SplineCurve::on(2, &knots, &points, &weights).unwrap();
        assert!(c.closed);
        assert_eq!((c.lo, c.hi), (-PI, PI));
        for i in 0..100 {
            let t = -PI + TAU * i as f64 / 100.0;
            assert!((c.point(t).len() - r).abs() < 1.0e-14);
            assert!(near(c.point(t), c.point(t + TAU), 1.0e-13));
            let p = c.point(t) * 0.7 + v3(0.0, 0.0, 0.2);
            assert!(wrap_near(c.nearest(p), t).sub_abs(t) < 1.0e-9);
        }
    }

    trait SubAbs {
        fn sub_abs(self, o: f64) -> f64;
    }
    impl SubAbs for f64 {
        fn sub_abs(self, o: f64) -> f64 {
            (self - o).abs()
        }
    }

    /// `z = x² - y²/2` over a rectangle, as a patch of degree two each way.
    fn saddle() -> SurfForm {
        // A parabola `a + b t + c t²` on `[0, 1]` has control values
        // `a`, `a + b/2` and `a + b + c`.
        let (x0, x1, y0, y1) = (-1.0, 2.0, -1.5, 1.0);
        let fx = [x0 * x0, x0 * x1, x1 * x1];
        let fy = [y0 * y0, y0 * y1, y1 * y1];
        let mut points = Vec::new();
        for (j, fy) in fy.iter().enumerate() {
            for (i, fx) in fx.iter().enumerate() {
                let x = x0 + (x1 - x0) * i as f64 / 2.0;
                let y = y0 + (y1 - y0) * j as f64 / 2.0;
                points.push(v3(x, y, fx - 0.5 * fy));
            }
        }
        let k = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        SplineSurface::on([2, 2], [&k, &k], [3, 3], &points, &[]).unwrap()
    }

    #[test]
    fn a_saddle_and_the_points_nearest_to_it() {
        let s = saddle();
        assert!(!s.rational && s.closed == [false, false]);
        for j in 0..=10 {
            for i in 0..=10 {
                let (u, v) = (i as f64 / 10.0, j as f64 / 10.0);
                let [p, su, sv, suu, suv, svv] = s.derivs(u, v, 2);
                let (x, y) = (-1.0 + 3.0 * u, -1.5 + 2.5 * v);
                assert!(near(p, v3(x, y, x * x - 0.5 * y * y), 1.0e-14));
                assert!(near(su, v3(3.0, 0.0, 6.0 * x), 1.0e-13));
                assert!(near(sv, v3(0.0, 2.5, -2.5 * y), 1.0e-13));
                assert!(near(suu, v3(0.0, 0.0, 18.0), 1.0e-12));
                assert!(near(suv, V3::ZERO, 1.0e-12));
                assert!(near(svv, v3(0.0, 0.0, -6.25), 1.0e-12));
                // Off along the normal, either side, the foot is found.
                let n = su.cross(sv).unit();
                for h in [0.2, -0.15] {
                    let q = s.nearest(p + n * h);
                    assert!(
                        (q.x - u).abs() < 1.0e-9 && (q.y - v).abs() < 1.0e-9,
                        "{q:?}"
                    );
                }
            }
        }
        // Beyond an edge the surface goes on straight, and a point there
        // has its foot on the continuation.
        let [edge, su, _, ..] = s.derivs(1.0, 0.3, 1);
        assert!(near(s.point(1.2, 0.3), edge + su * 0.2, 1.0e-13));
        let q = s.nearest(s.point(1.2, 0.3));
        assert!(
            (q.x - 1.2).abs() < 1.0e-9 && (q.y - 0.3).abs() < 1.0e-9,
            "{q:?}"
        );
        let b = s.bend(s.lo, s.hi);
        assert!((b.uu - 18.0).abs() < 1.0e-9 && (b.vv - 6.25).abs() < 1.0e-9);
    }

    #[test]
    fn a_quarter_cylinder_and_its_lines() {
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let arc = [(4.0, 0.0, 1.0), (4.0, 4.0, w), (0.0, 4.0, 1.0)];
        let (mut points, mut weights) = (Vec::new(), Vec::new());
        for z in [0.0, 10.0] {
            for &(x, y, w) in &arc {
                points.push(v3(x, y, z));
                weights.push(w);
            }
        }
        let s = SplineSurface::on(
            [2, 1],
            [&[0.0, 0.0, 0.0, 1.0, 1.0, 1.0], &[0.0, 0.0, 1.0, 1.0]],
            [3, 2],
            &points,
            &weights,
        )
        .unwrap();
        assert!(s.rational);
        for j in 0..=8 {
            for i in 0..=8 {
                let (u, v) = (i as f64 / 8.0, j as f64 / 8.0);
                let [p, su, sv, ..] = s.derivs(u, v, 1);
                assert!((p.x.hypot(p.y) - 4.0).abs() < 1.0e-14);
                assert!((p.z - 10.0 * v).abs() < 1.0e-13);
                let n = su.cross(sv).unit();
                assert!(near(n, v3(p.x, p.y, 0.0) / 4.0, 1.0e-13));
                let q = s.nearest(p + n * 0.8);
                assert!((q.x - u).abs() < 1.0e-9 && (q.y - v).abs() < 1.0e-9);
            }
        }
        // The line at the end of the arc, and the arc at mid height.
        let line = s.iso(0, 1.0).unwrap();
        assert_eq!(line.degree, 1);
        assert!(near(line.point(0.25), v3(0.0, 4.0, 2.5), 1.0e-13));
        let arc = s.iso(1, 0.5).unwrap();
        assert!(arc.rational && arc.degree == 2);
        for i in 0..=8 {
            let p = arc.point(i as f64 / 8.0);
            assert!((p.x.hypot(p.y) - 4.0).abs() < 1.0e-14 && (p.z - 5.0).abs() < 1.0e-13);
        }
    }

    #[test]
    fn a_tube_closed_one_way() {
        // A circle of control points swept along a bent line: closed and
        // smooth around, open along.
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let ring = [
            (1.0, 0.0, 1.0),
            (1.0, 1.0, w),
            (0.0, 1.0, 1.0),
            (-1.0, 1.0, w),
            (-1.0, 0.0, 1.0),
            (-1.0, -1.0, w),
            (0.0, -1.0, 1.0),
            (1.0, -1.0, w),
            (1.0, 0.0, 1.0),
        ];
        let (mut points, mut weights) = (Vec::new(), Vec::new());
        for (z, r, shift) in [(0.0, 2.0, 0.0), (5.0, 3.0, 1.0), (10.0, 2.5, 0.0)] {
            for &(x, y, w) in &ring {
                points.push(v3(r * x + shift, r * y, z));
                weights.push(w);
            }
        }
        let ku = [0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0, 4.0];
        let kv = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        let s = SplineSurface::on([2, 2], [&ku, &kv], [9, 3], &points, &weights).unwrap();
        assert_eq!(s.closed, [true, false]);
        assert_eq!((s.lo.x, s.hi.x), (-PI, PI));
        for j in 0..=6 {
            for i in 0..24 {
                let (u, v) = (-PI + TAU * (i as f64 + 0.37) / 24.0, j as f64 / 6.0);
                let [p, su, sv, ..] = s.derivs(u, v, 1);
                assert!(near(p, s.point(u + TAU, v), 1.0e-12));
                let n = su.cross(sv).unit();
                let q = s.nearest(p + n * 0.3);
                assert!(
                    (wrap_near(q.x, u) - u).abs() < 1.0e-8 && (q.y - v).abs() < 1.0e-8,
                    "{q:?} for {u} {v}"
                );
            }
        }
    }

    #[test]
    fn knots_that_are_not_clamped_are_clamped() {
        // A uniform cubic on seven points; the curve lives on [3, 7].
        let pts = [
            v3(0.0, 0.0, 0.0),
            v3(1.0, 2.0, 0.0),
            v3(3.0, 3.0, 1.0),
            v3(5.0, 1.0, 0.0),
            v3(6.0, -2.0, 2.0),
            v3(8.0, 0.0, 0.0),
            v3(9.0, 3.0, 1.0),
        ];
        let knots: Vec<f64> = (0..11).map(f64::from).collect();
        let eval = |t: f64| {
            // De Boor, directly.
            let k = (t.floor() as usize).clamp(3, 6);
            let mut d: Vec<V3> = (0..=3).map(|j| pts[j + k - 3]).collect();
            for r in 1..=3 {
                for j in (r..=3).rev() {
                    let i = j + k - 3;
                    let a = (t - knots[i]) / (knots[i + 3 - r + 1] - knots[i]);
                    d[j] = d[j - 1] * (1.0 - a) + d[j] * a;
                }
            }
            d[3]
        };
        let mut rows = vec![pts.iter().map(|&p| lift(p, 1.0)).collect::<Vec<Hom>>()];
        let clamped = clamp(&knots, 3, &mut rows).unwrap();
        let c = SplineCurve::from_hom(3, clamped, rows.remove(0)).unwrap();
        assert_eq!((c.lo, c.hi), (3.0, 7.0));
        for i in 0..=40 {
            let t = 3.0 + 4.0 * i as f64 / 40.0;
            assert!(near(c.point(t), eval(t.min(6.999_999_999)), 1.0e-8), "{t}");
        }
    }

    #[test]
    fn pieces_of_lines_and_arcs() {
        let a = arc_piece(v3(2.0, 0.0, 0.0), v3(0.0, 2.0, 0.0), V3::ZERO).unwrap();
        let c = SplineCurve::from_hom(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], a.to_vec()).unwrap();
        for i in 0..=10 {
            assert!((c.point(i as f64 / 10.0).len() - 2.0).abs() < 1.0e-14);
        }
        let l = line_piece(v3(1.0, 1.0, 1.0), v3(3.0, 1.0, 5.0));
        let c = SplineCurve::from_hom(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], l.to_vec()).unwrap();
        assert!(near(c.point(0.25), v3(1.5, 1.0, 2.0), 1.0e-14));
        assert!(arc_piece(v3(2.0, 0.0, 0.0), v3(-2.0, 1.0e-9, 0.0), V3::ZERO).is_err());
    }
}
