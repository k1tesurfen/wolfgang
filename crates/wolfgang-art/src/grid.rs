//! Finding the pixel grid in "pixel-art style" images.
//!
//! Real pixel art upscaled by p has colour edges every p image pixels. We sum
//! edge strength per column (and per row) and look for the period whose comb
//! of positions lines up with the edges best (the magnitude of the Fourier
//! component at 1/p). Multiples of the true frequency score as well, so we
//! take the *largest* period that scores close to the best.

use wolfgang_core::image::Pixels;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Axis {
    /// Size of one art pixel in image pixels.
    pub period: f64,
    /// Offset of the first cell border.
    pub phase: f64,
    /// Relative strength of the detected grid (0–1).
    pub confidence: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grid {
    pub x: Axis,
    pub y: Axis,
}

fn luma(p: &[u8]) -> f64 {
    0.299 * p[0] as f64 + 0.587 * p[1] as f64 + 0.114 * p[2] as f64
}

/// Edge strength profile along x (`horizontal = true`) or y.
fn profile(img: &Pixels, horizontal: bool, region: (u32, u32, u32, u32)) -> Vec<f64> {
    let (x0, y0, w, h) = region;
    let at = |x: u32, y: u32| luma(&img.rgba[((y * img.width + x) * 4) as usize..][..4]);
    let len = if horizontal { w } else { h };
    let mut e = vec![0.0; len as usize];
    for i in 1..len {
        let mut sum = 0.0;
        let across = if horizontal { h } else { w };
        for j in 0..across {
            let (a, b) = if horizontal { (at(x0 + i, y0 + j), at(x0 + i - 1, y0 + j)) } else { (at(x0 + j, y0 + i), at(x0 + j, y0 + i - 1)) };
            sum += (a - b).abs();
        }
        e[i as usize] = sum;
    }
    e
}

fn detect(e: &[f64], min: f64, max: f64) -> Axis {
    let mean = e.iter().sum::<f64>() / e.len() as f64;
    let centered: Vec<f64> = e.iter().map(|v| v - mean).collect();
    let energy: f64 = centered.iter().map(|v| v * v).sum::<f64>().sqrt() * (e.len() as f64).sqrt();
    let mut scores = Vec::new();
    let mut p = min;
    while p <= max {
        let (mut c, mut s) = (0.0, 0.0);
        for (x, v) in centered.iter().enumerate() {
            let a = std::f64::consts::TAU * x as f64 / p;
            c += v * a.cos();
            s += v * a.sin();
        }
        scores.push((p, (c * c + s * s).sqrt() / energy.max(1e-9), s.atan2(c)));
        p += 0.01;
    }
    let best = scores.iter().map(|s| s.1).fold(0.0, f64::max);
    let (period, score, angle) = *scores.iter().rev().find(|s| s.1 >= best * 0.85).expect("non-empty");
    let phase = (angle / std::f64::consts::TAU * period).rem_euclid(period);
    Axis { period, phase, confidence: score }
}

/// Detects the grid in a region `(x, y, w, h)` of the image.
pub fn find(img: &Pixels, region: (u32, u32, u32, u32), min: f64, max: f64) -> Grid {
    Grid { x: detect(&profile(img, true, region), min, max), y: detect(&profile(img, false, region), min, max) }
}
