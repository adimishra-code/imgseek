pub fn rgb_to_hsv(r: u8, g: u8, b: u8) -> (f64, f64, f64) {
    let rf = r as f64 / 255.0;
    let gf = g as f64 / 255.0;
    let bf = b as f64 / 255.0;

    let max = rf.max(gf.max(bf));
    let min = rf.min(gf.min(bf));
    let delta = max - min;

    let v = max;
    let s = if max == 0.0 { 0.0 } else { delta / max };

    let h = if delta == 0.0 {
        0.0
    } else if max == rf {
        let mut h = 60.0 * ((gf - bf) / delta);
        if h < 0.0 {
            h += 360.0;
        }
        h
    } else if max == gf {
        60.0 * (((bf - rf) / delta) + 2.0)
    } else {
        60.0 * (((rf - gf) / delta) + 4.0)
    };

    (h, s, v)
}

#[inline]
pub fn hsv_to_bin(h: f64, s: f64, v: f64) -> usize {
    let h_bin = ((h / 45.0).floor() as usize) % 8;
    let s_bin = ((s * 8.0).floor() as usize).min(7);
    let v_bin = ((v * 4.0).floor() as usize).min(3);
    (h_bin * 8 + s_bin) * 4 + v_bin
}

pub fn compute_color_histogram(rgb: &[u8]) -> [f32; 256] {
    let mut hist = [0f32; 256];
    let (chunks, _) = rgb.as_chunks::<3>();
    if chunks.is_empty() {
        return hist;
    }

    for chunk in chunks {
        let (h, s, v) = rgb_to_hsv(chunk[0], chunk[1], chunk[2]);
        let bin = hsv_to_bin(h, s, v);
        hist[bin] += 1.0;
    }

    let total = chunks.len() as f32;
    for bin in &mut hist {
        *bin /= total;
    }

    hist
}

pub fn histogram_intersection(h1: &[f32; 256], h2: &[f32; 256]) -> f32 {
    let mut sim = 0.0f32;
    for i in 0..256 {
        sim += h1[i].min(h2[i]);
    }
    sim.clamp(0.0, 1.0)
}

pub fn histogram_distance(h1: &[f32; 256], h2: &[f32; 256]) -> f32 {
    (1.0 - histogram_intersection(h1, h2)).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pure_colors_histogram_intersection() {
        let red_rgb = vec![255, 0, 0, 255, 0, 0, 255, 0, 0, 255, 0, 0];
        let green_rgb = vec![0, 255, 0, 0, 255, 0, 0, 255, 0, 0, 255, 0];
        let blue_rgb = vec![0, 0, 255, 0, 0, 255, 0, 0, 255, 0, 0, 255];

        let h_red = compute_color_histogram(&red_rgb);
        let h_green = compute_color_histogram(&green_rgb);
        let h_blue = compute_color_histogram(&blue_rgb);

        assert!((histogram_intersection(&h_red, &h_red) - 1.0).abs() < 1e-6);
        assert!((histogram_intersection(&h_green, &h_green) - 1.0).abs() < 1e-6);
        assert!((histogram_intersection(&h_blue, &h_blue) - 1.0).abs() < 1e-6);

        assert_eq!(histogram_intersection(&h_red, &h_green), 0.0);
        assert_eq!(histogram_intersection(&h_red, &h_blue), 0.0);
        assert_eq!(histogram_intersection(&h_green, &h_blue), 0.0);

        assert_eq!(histogram_distance(&h_red, &h_green), 1.0);
        assert!((histogram_distance(&h_red, &h_red) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_mixed_color_histogram_intersection() {
        let mut half_red_half_green = Vec::new();
        for _ in 0..50 {
            half_red_half_green.extend_from_slice(&[255, 0, 0]);
        }
        for _ in 0..50 {
            half_red_half_green.extend_from_slice(&[0, 255, 0]);
        }

        let pure_red = vec![255, 0, 0];
        let h_mixed = compute_color_histogram(&half_red_half_green);
        let h_red = compute_color_histogram(&pure_red);

        let sim = histogram_intersection(&h_mixed, &h_red);
        assert!((sim - 0.5).abs() < 1e-6);
        let dist = histogram_distance(&h_mixed, &h_red);
        assert!((dist - 0.5).abs() < 1e-6);
    }
}
