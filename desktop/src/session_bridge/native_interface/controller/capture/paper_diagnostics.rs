//! Bounded sample of the captured Bevy image, enabled only for fixture probes.
use super::*;

pub(super) fn enabled() -> bool {
    std::env::var("LIMO_CAD_NATIVE_PAPER_DIAGNOSTICS").as_deref() == Ok("1")
}
pub(super) fn sample(image: &bevy::image::Image, snapshot: &Value) -> Result<Value, String> {
    let client = pair(&snapshot["client_size"])?;
    let logical = pair(&snapshot["sample_logical"])?;
    let rgba = image
        .clone()
        .try_into_dynamic()
        .map_err(|error| error.to_string())?
        .to_rgba8();
    let point = sample_pixel(client, logical, [rgba.width(), rgba.height()])?;
    let mut pixels = Vec::with_capacity(9);
    for y in point[1] - 1..=point[1] + 1 {
        for x in point[0] - 1..=point[0] + 1 {
            pixels.push(rgba.get_pixel(x, y).0);
        }
    }
    Ok(json!({"physical_center":point,"rgba_3x3":pixels,
        "fitted_paper_white":snapshot["fitted"]==true && white(&pixels)}))
}
fn pair(value: &Value) -> Result<[f64; 2], String> {
    let x = value[0]
        .as_f64()
        .ok_or("Paper diagnostic X is unavailable")?;
    let y = value[1]
        .as_f64()
        .ok_or("Paper diagnostic Y is unavailable")?;
    if !x.is_finite() || !y.is_finite() {
        return Err("Paper diagnostic coordinate is not finite".into());
    }
    Ok([x, y])
}
fn sample_pixel(client: [f64; 2], logical: [f64; 2], pixels: [u32; 2]) -> Result<[u32; 2], String> {
    if client.iter().any(|n| !n.is_finite() || *n <= 0.) || logical.iter().any(|n| !n.is_finite()) {
        return Err("Paper diagnostic has invalid client geometry".into());
    }
    let point: [f64; 2] =
        std::array::from_fn(|i| (logical[i] * f64::from(pixels[i]) / client[i]).round());
    if (0..2).any(|i| pixels[i] < 3 || point[i] < 1. || point[i] + 1. >= f64::from(pixels[i])) {
        return Err("Paper sample lies outside the captured image".into());
    }
    Ok(point.map(|n| n as u32))
}
fn white(pixels: &[[u8; 4]]) -> bool {
    pixels.len() == 9
        && pixels
            .iter()
            .all(|p| p[0] >= 245 && p[1] >= 245 && p[2] >= 245 && p[3] >= 250)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn known_paper_margin_distinguishes_blank_canvas_at_both_pixel_scales() {
        assert_eq!(
            sample_pixel([1360., 860.], [362., 138.], [1360, 860]).unwrap(),
            [362, 138]
        );
        assert_eq!(
            sample_pixel([1360., 860.], [362., 138.], [2720, 1720]).unwrap(),
            [724, 276]
        );
        assert!(sample_pixel([1360., 860.], [-1., 20.], [1360, 860]).is_err());
        assert!(white(&[[255; 4]; 9]));
        assert!(!white(&[[220, 228, 235, 255]; 9]));
        assert!(!white(&[[255, 255, 255, 0]; 9]));
        assert!(!white(&[[255; 4]; 8]));
    }
}
