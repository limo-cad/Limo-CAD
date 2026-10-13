//! Pixel oracle shared by the real GPU matrix and its broken-render fixtures.
//! Probe locations come from known edge geometry projected through the camera.
#[derive(Debug)]
pub(super) struct StrokeEvidence {
    pub visible_samples: usize,
    pub samples: usize,
}

pub(super) fn visible_stroke(
    width: u32,
    height: u32,
    with: &[u8],
    without: &[u8],
    probes: &[[f32; 2]],
) -> Result<StrokeEvidence, &'static str> {
    let bytes = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or("invalid capture dimensions")?;
    if with.len() != bytes || without.len() != bytes || probes.len() < 8 {
        return Err("missing RGBA captures or insufficient edge probes");
    }
    let mut visible_samples = 0;
    for &[x, y] in probes {
        if !x.is_finite()
            || !y.is_finite()
            || x < 2.
            || y < 2.
            || x >= width as f32 - 2.
            || y >= height as f32 - 2.
        {
            return Err("known edge probe falls outside the capture");
        }
        let mut contrast = 0;
        for dy in -2..=2 {
            for dx in -2..=2 {
                let px = (x.round() as i32 + dx) as usize;
                let py = (y.round() as i32 + dy) as usize;
                if px >= width as usize || py >= height as usize {
                    continue;
                }
                let offset = (py * width as usize + px) * 4;
                for channel in 0..3 {
                    contrast =
                        contrast.max(with[offset + channel].abs_diff(without[offset + channel]));
                }
            }
        }
        // 24/255 excludes encoding/rounding noise, while tolerating antialiasing.
        visible_samples += usize::from(contrast >= 24);
    }
    let evidence = StrokeEvidence {
        visible_samples,
        samples: probes.len(),
    };
    // A distributed stroke, rather than one isolated bright pixel. The 5x5
    // neighborhoods accommodate raster coverage without comparing PNG sizes.
    if evidence.visible_samples * 4 < evidence.samples * 3 {
        return Err("known visible edge is absent at more than a quarter of its probes");
    }
    Ok(evidence)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn positive_oracle_rejects_all_strokes_missing_unrelated_art_and_sparse_noise() {
        let baseline = vec![180; 64 * 64 * 4];
        let probes = (0..12)
            .map(|i| [10. + i as f32 * 3., 30.])
            .collect::<Vec<_>>();
        assert!(visible_stroke(64, 64, &baseline, &baseline, &probes).is_err());
        let mut unrelated = baseline.clone();
        unrelated[..64 * 4 * 5].fill(30);
        assert!(visible_stroke(64, 64, &unrelated, &baseline, &probes).is_err());
        let mut sparse = baseline.clone();
        sparse[(30 * 64 + 10) * 4] = 30;
        assert!(visible_stroke(64, 64, &sparse, &baseline, &probes).is_err());
        let mut stroke = baseline.clone();
        // An antialiased, one-pixel stroke shifted one raster pixel.
        for x in 8..48 {
            stroke[(31 * 64 + x) * 4] = 120;
        }
        let evidence = visible_stroke(64, 64, &stroke, &baseline, &probes).unwrap();
        assert_eq!(evidence.visible_samples, 12);
        assert!(visible_stroke(64, 64, &stroke, &baseline[..20], &probes).is_err());
        assert!(visible_stroke(64, 64, &stroke, &baseline, &[[f32::NAN, 30.]; 12]).is_err());
    }
}
