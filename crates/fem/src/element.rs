pub(crate) struct Geometry {
    pub gradients: [[f64; 3]; 4],
    pub volume: f64,
}
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}
pub(crate) fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.into_iter().zip(b).map(|(x, y)| x * y).sum()
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
pub(crate) fn geometry(points: [[f64; 3]; 4]) -> Result<Geometry, String> {
    let edges = [
        sub(points[1], points[0]),
        sub(points[2], points[0]),
        sub(points[3], points[0]),
    ];
    // All six Euclidean edge lengths keep the quality threshold invariant
    // under rigid rotation and local node permutations.
    let length = (0..4)
        .flat_map(|i| ((i + 1)..4).map(move |j| (i, j)))
        .map(|(i, j)| {
            sub(points[i], points[j])
                .iter()
                .fold(0_f64, |n, x| n.hypot(*x))
        })
        .fold(0., f64::max);
    if !length.is_finite() || length == 0. {
        return Err("Degenerate or unrepresentable tetrahedron".into());
    }
    let e = edges.map(|v| v.map(|x| x / length));
    let det = dot(e[0], cross(e[1], e[2]));
    if !det.is_finite() || det.abs() <= 1e-12 {
        return Err("Degenerate or excessively ill-conditioned tetrahedron".into());
    }
    let volume = (length * length * length) * (det.abs() / 6.);
    let mut gradients = [[0.; 3]; 4];
    for (i, v) in [cross(e[1], e[2]), cross(e[2], e[0]), cross(e[0], e[1])]
        .into_iter()
        .enumerate()
    {
        gradients[i + 1] = v.map(|x| x / det / length);
    }
    gradients[0] = std::array::from_fn(|j| -gradients[1][j] - gradients[2][j] - gradients[3][j]);
    if !volume.is_finite() || volume <= 0. || !gradients.iter().flatten().all(|x| x.is_finite()) {
        return Err("Tetrahedron exceeds numeric range".into());
    }
    Ok(Geometry { gradients, volume })
}

pub(crate) fn face_area(points: [[f64; 3]; 3]) -> Result<f64, String> {
    let a = sub(points[1], points[0]);
    let b = sub(points[2], points[0]);
    let area = cross(a, b).iter().fold(0_f64, |n, x| n.hypot(*x)) / 2.;
    if area.is_finite() && area > 0. {
        Ok(area)
    } else {
        Err("Thermal boundary face has invalid area".into())
    }
}
