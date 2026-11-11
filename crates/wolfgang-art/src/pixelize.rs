//! Turning a "pixel-art style" illustration into real pixel art:
//! a fixed palette (k-means in OKLab), then each target pixel takes the most
//! frequent palette colour of its source block (no blurry in-between colours).

use std::collections::BTreeMap;
use wolfgang_core::image::Pixels;

pub type Rgb = [u8; 3];

/// An image of palette indices.
pub struct Indexed {
    pub width: u32,
    pub height: u32,
    pub index: Vec<u16>,
    pub palette: Vec<Rgb>,
}

impl Indexed {
    pub fn rgba(&self) -> Vec<u8> {
        self.index.iter().flat_map(|&i| if i == TRANSPARENT { [0, 0, 0, 0] } else { let c = self.palette[i as usize]; [c[0], c[1], c[2], 255] }).collect()
    }
}

/// How much darker (OKLab L) a colour must be than the block's majority to count as outline.
const OUTLINE_CONTRAST: f32 = 0.18;
/// Outlines are dark: OKLab L below this.
const OUTLINE_MAX_L: f32 = 0.42;

/// Palette index meaning "transparent".
pub const TRANSPARENT: u16 = u16::MAX;

fn srgb_to_linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn linear_to_srgb(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let v = if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (v * 255.0).round() as u8
}

/// Constants as published with OKLab (Björn Ottosson, 2020).
#[allow(clippy::excessive_precision)]
pub fn oklab(c: Rgb) -> [f32; 3] {
    let [r, g, b] = c.map(srgb_to_linear);
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    [
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    ]
}

#[allow(clippy::excessive_precision)]
fn from_oklab([l, a, b]: [f32; 3]) -> Rgb {
    let l_ = (l + 0.3963377774 * a + 0.2158037573 * b).powi(3);
    let m_ = (l - 0.1055613458 * a - 0.0638541728 * b).powi(3);
    let s_ = (l - 0.0894841775 * a - 1.2914855480 * b).powi(3);
    [
        linear_to_srgb(4.0767416621 * l_ - 3.3077115913 * m_ + 0.2309699292 * s_),
        linear_to_srgb(-1.2684380046 * l_ + 2.6097574011 * m_ - 0.3413193965 * s_),
        linear_to_srgb(-0.0041960863 * l_ - 0.7034186147 * m_ + 1.7076147010 * s_),
    ]
}

fn dist(a: &[f32; 3], b: &[f32; 3]) -> f32 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
}

pub fn nearest(palette_lab: &[[f32; 3]], c: &[f32; 3]) -> u16 {
    let mut best = (f32::MAX, 0u16);
    for (i, p) in palette_lab.iter().enumerate() {
        let d = dist(p, c);
        if d < best.0 {
            best = (d, i as u16);
        }
    }
    best.1
}

/// k-means palette of `k` colours from a sample of `colors` (deterministic).
pub fn palette(colors: &[Rgb], k: usize) -> Vec<Rgb> {
    let step = (colors.len() / 60_000).max(1);
    let sample: Vec<[f32; 3]> = colors.iter().step_by(step).map(|c| oklab(*c)).collect();
    // k-means++-ish init: spread by farthest-point.
    let mut centers = vec![sample[sample.len() / 2]];
    let mut d: Vec<f32> = sample.iter().map(|s| dist(s, &centers[0])).collect();
    while centers.len() < k {
        let (i, _) = d.iter().enumerate().fold((0, -1.0), |acc, (i, v)| if *v > acc.1 { (i, *v) } else { acc });
        centers.push(sample[i]);
        for (j, s) in sample.iter().enumerate() {
            d[j] = d[j].min(dist(s, &centers[centers.len() - 1]));
        }
    }
    for _ in 0..12 {
        let mut sums = vec![[0f32; 4]; k];
        for s in &sample {
            let i = nearest(&centers, s) as usize;
            for c in 0..3 {
                sums[i][c] += s[c];
            }
            sums[i][3] += 1.0;
        }
        for (c, s) in centers.iter_mut().zip(&sums) {
            if s[3] > 0.0 {
                *c = [s[0] / s[3], s[1] / s[3], s[2] / s[3]];
            }
        }
    }
    let mut pal: Vec<Rgb> = centers.into_iter().map(from_oklab).collect();
    pal.sort_by_key(|c| (oklab(*c)[0] * 1000.0) as i32);
    pal.dedup();
    pal
}

/// Region `(x, y, w, h)` of `img` → `out_w` × `out_h` pixels in `pal`.
pub fn pixelize(img: &Pixels, region: (u32, u32, u32, u32), out_w: u32, out_h: u32, pal: &[Rgb]) -> Indexed {
    let lab: Vec<[f32; 3]> = pal.iter().map(|c| oklab(*c)).collect();
    let (x0, y0, w, h) = region;
    // Map every source pixel once.
    let mut src = vec![0u16; (w * h) as usize];
    let mut cache = std::collections::HashMap::new();
    for y in 0..h {
        for x in 0..w {
            let o = (((y0 + y) * img.width + x0 + x) * 4) as usize;
            let c = [img.rgba[o], img.rgba[o + 1], img.rgba[o + 2]];
            // Quantize the cache key a little: JPEG noise makes nearly every pixel unique.
            let key = [c[0] >> 1, c[1] >> 1, c[2] >> 1];
            src[(y * w + x) as usize] = *cache.entry(key).or_insert_with(|| nearest(&lab, &oklab(c)));
        }
    }
    let mut index = Vec::with_capacity((out_w * out_h) as usize);
    let mut counts = vec![0u32; pal.len()];
    for oy in 0..out_h {
        for ox in 0..out_w {
            // Inner 80% of the block: borders are where neighbouring blocks bleed in.
            let bx0 = (ox as f64 + 0.1) * w as f64 / out_w as f64;
            let bx1 = (ox as f64 + 0.9) * w as f64 / out_w as f64;
            let by0 = (oy as f64 + 0.1) * h as f64 / out_h as f64;
            let by1 = (oy as f64 + 0.9) * h as f64 / out_h as f64;
            counts.iter_mut().for_each(|c| *c = 0);
            for y in by0.floor() as u32..(by1.ceil() as u32).min(h) {
                for x in bx0.floor() as u32..(bx1.ceil() as u32).min(w) {
                    counts[src[(y * w + x) as usize] as usize] += 1;
                }
            }
            let (mode, _) = counts.iter().enumerate().max_by_key(|(_, c)| **c).unwrap();
            // Thin dark outlines rarely win a block; keep them if they cover a fifth of it.
            let total: u32 = counts.iter().sum();
            let darkest = counts
                .iter()
                .enumerate()
                .filter(|(_, c)| **c * 5 >= total)
                .min_by(|a, b| lab[a.0][0].total_cmp(&lab[b.0][0]))
                .map(|(i, _)| i)
                .unwrap_or(mode);
            let pick = if lab[darkest][0] < lab[mode][0] - OUTLINE_CONTRAST && lab[darkest][0] < OUTLINE_MAX_L { darkest } else { mode };
            index.push(pick as u16);
        }
    }
    close_outline_gaps(&mut index, out_w, out_h, &lab);
    Indexed { width: out_w, height: out_h, index, palette: pal.to_vec() }
}

/// A line that runs between two blocks comes out dashed; a pixel with outline
/// pixels on both sides (horizontally or vertically) becomes outline as well.
fn close_outline_gaps(index: &mut [u16], w: u32, h: u32, lab: &[[f32; 3]]) {
    let dark = |i: u16| i != TRANSPARENT && lab[i as usize][0] < OUTLINE_MAX_L;
    let before = index.to_vec();
    let at = |x: u32, y: u32| before[(y * w + x) as usize];
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let me = at(x, y);
            if dark(me) {
                continue;
            }
            let pair = [(at(x - 1, y), at(x + 1, y)), (at(x, y - 1), at(x, y + 1))]
                .into_iter()
                .find(|(a, b)| dark(*a) && dark(*b) && lab[*a as usize][0] < lab[me as usize][0] - OUTLINE_CONTRAST);
            if let Some((a, b)) = pair {
                index[(y * w + x) as usize] = if lab[a as usize][0] <= lab[b as usize][0] { a } else { b };
            }
        }
    }
}

/// The pixels Gemini drew: `period` image pixels each, the first border at `phase` (per axis).
pub struct Cells {
    pub period: (f64, f64),
    pub phase: (f64, f64),
}

/// The colour of every cell in `first .. first + n` (cell coordinates): the OKLab mean
/// of its middle half. Cell borders are where JPEG halos and soft edges sit.
pub fn cell_colors(img: &Pixels, cells: &Cells, first: (i64, i64), n: (u32, u32)) -> Vec<Rgb> {
    let span = |i: i64, phase: f64, period: f64, max: u32| {
        let a = (phase + (i as f64 + 0.25) * period).round().clamp(0.0, max as f64 - 1.0) as u32;
        let b = (phase + (i as f64 + 0.75) * period).round().clamp(0.0, max as f64 - 1.0) as u32;
        a..=b.max(a)
    };
    let mut out = Vec::with_capacity((n.0 * n.1) as usize);
    for cy in 0..n.1 as i64 {
        for cx in 0..n.0 as i64 {
            let (mut sum, mut k) = ([0f32; 3], 0f32);
            for y in span(first.1 + cy, cells.phase.1, cells.period.1, img.height) {
                for x in span(first.0 + cx, cells.phase.0, cells.period.0, img.width) {
                    let o = ((y * img.width + x) * 4) as usize;
                    let l = oklab([img.rgba[o], img.rgba[o + 1], img.rgba[o + 2]]);
                    (0..3).for_each(|c| sum[c] += l[c]);
                    k += 1.0;
                }
            }
            out.push(from_oklab(sum.map(|s| s / k)));
        }
    }
    out
}

/// A small feature must differ this much (OKLab) from what surrounds it to be rescued.
const RESCUE_CONTRAST: f32 = 0.12;

/// `f`×`f` cells → one pixel. The majority wins; on a tie the colour that stands out
/// more against the surrounding cells (a line through the block survives). Then small,
/// distinct features that lost every pixel (a pupil straddling four blocks) get back
/// exactly one, the block they cover most. Nothing ever grows past its own cells.
/// Features touching `background` are never rescued: those are blended edge cells.
pub fn reduce(native: &[u16], nw: u32, nh: u32, f: u32, lab: &[[f32; 3]], background: Option<u16>) -> Vec<u16> {
    if f == 1 {
        return native.to_vec();
    }
    let (w, h) = (nw / f, nh / f);
    let at = |x: u32, y: u32| native[(y * nw + x) as usize] as usize;
    let mut out = vec![0u16; (w * h) as usize];
    let mut counts = vec![0u32; lab.len()];
    for by in 0..h {
        for bx in 0..w {
            counts.iter_mut().for_each(|c| *c = 0);
            for y in by * f..(by + 1) * f {
                for x in bx * f..(bx + 1) * f {
                    counts[at(x, y)] += 1;
                }
            }
            let best = *counts.iter().max().unwrap();
            let tied: Vec<usize> = (0..lab.len()).filter(|&i| counts[i] == best).collect();
            out[(by * w + bx) as usize] = if tied.len() == 1 {
                tied[0]
            } else {
                // Mean of the ring of cells around the block.
                let (mut sum, mut k) = ([0f32; 3], 0f32);
                for y in (by * f).saturating_sub(f)..((by + 2) * f).min(nh) {
                    for x in (bx * f).saturating_sub(f)..((bx + 2) * f).min(nw) {
                        if x / f != bx || y / f != by {
                            (0..3).for_each(|c| sum[c] += lab[at(x, y)][c]);
                            k += 1.0;
                        }
                    }
                }
                let ring = sum.map(|s| s / k.max(1.0));
                *tied.iter().max_by(|a, b| dist(&lab[**a], &ring).total_cmp(&dist(&lab[**b], &ring))).unwrap()
            } as u16;
        }
    }

    // Rescue: connected same-colour features of at most two blocks' worth of cells.
    let mut seen = vec![false; native.len()];
    let mut rescued = vec![false; out.len()];
    for start in 0..native.len() {
        if seen[start] {
            continue;
        }
        let color = native[start];
        let (mut comp, mut todo, mut border, mut on_background) = (vec![start], vec![start], ([0f32; 3], 0f32), false);
        seen[start] = true;
        while let Some(o) = todo.pop() {
            let (x, y) = ((o as u32 % nw) as i64, (o as u32 / nw) as i64);
            for (nx, ny) in [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)] {
                if nx < 0 || ny < 0 || nx >= nw as i64 || ny >= nh as i64 {
                    continue;
                }
                let n = (ny * nw as i64 + nx) as usize;
                if native[n] == color {
                    if !seen[n] {
                        seen[n] = true;
                        comp.push(n);
                        todo.push(n);
                    }
                } else {
                    on_background |= Some(native[n]) == background;
                    (0..3).for_each(|c| border.0[c] += lab[native[n] as usize][c]);
                    border.1 += 1.0;
                }
            }
        }
        if comp.len() > (2 * f * f) as usize || border.1 == 0.0 || on_background {
            continue;
        }
        let around = border.0.map(|s| s / border.1);
        if dist(&lab[color as usize], &around) < RESCUE_CONTRAST * RESCUE_CONTRAST {
            continue;
        }
        let mut cover: BTreeMap<usize, usize> = BTreeMap::new();
        for &o in &comp {
            let (x, y) = (o as u32 % nw / f, o as u32 / nw / f);
            if x < w && y < h {
                *cover.entry((y * w + x) as usize).or_default() += 1;
            }
        }
        if cover.keys().any(|b| out[*b] == color) {
            continue;
        }
        if let Some((&b, _)) = cover.iter().filter(|(b, _)| !rescued[**b]).max_by_key(|(_, n)| **n) {
            out[b] = color;
            rescued[b] = true;
        }
    }
    out
}

pub fn colors(img: &Pixels, region: (u32, u32, u32, u32)) -> Vec<Rgb> {
    let (x0, y0, w, h) = region;
    let mut v = Vec::with_capacity((w * h) as usize);
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            let o = ((y * img.width + x) * 4) as usize;
            v.push([img.rgba[o], img.rgba[o + 1], img.rgba[o + 2]]);
        }
    }
    v
}

/// Makes the background transparent: flood fill from the border through
/// pixels close (OKLab) to the corner colour.
pub fn cut_background(img: &mut Indexed, tolerance: f32) {
    let lab: Vec<[f32; 3]> = img.palette.iter().map(|c| oklab(*c)).collect();
    let (w, h) = (img.width as i32, img.height as i32);
    let bg = lab[img.index[0] as usize];
    let near = |i: u16| i != TRANSPARENT && dist(&lab[i as usize], &bg) < tolerance * tolerance;
    let mut stack: Vec<(i32, i32)> = Vec::new();
    for x in 0..w {
        stack.push((x, 0));
        stack.push((x, h - 1));
    }
    for y in 0..h {
        stack.push((0, y));
        stack.push((w - 1, y));
    }
    while let Some((x, y)) = stack.pop() {
        if x < 0 || y < 0 || x >= w || y >= h {
            continue;
        }
        let o = (y * w + x) as usize;
        if !near(img.index[o]) {
            continue;
        }
        img.index[o] = TRANSPARENT;
        stack.extend([(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]);
    }
    // Background enclosed by the figure (e.g. between a raised arm and the head):
    // background-coloured islands of `MIN_HOLE` pixels or more go too.
    const MIN_HOLE: usize = 6;
    let mut seen = vec![false; img.index.len()];
    for start in 0..img.index.len() {
        if seen[start] || !near(img.index[start]) {
            continue;
        }
        let mut island = vec![start];
        let mut todo = vec![start];
        seen[start] = true;
        while let Some(o) = todo.pop() {
            let (x, y) = ((o as i32) % w, (o as i32) / w);
            for (nx, ny) in [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)] {
                if nx < 0 || ny < 0 || nx >= w || ny >= h {
                    continue;
                }
                let n = (ny * w + nx) as usize;
                if !seen[n] && near(img.index[n]) {
                    seen[n] = true;
                    island.push(n);
                    todo.push(n);
                }
            }
        }
        if island.len() >= MIN_HOLE {
            island.into_iter().for_each(|o| img.index[o] = TRANSPARENT);
        }
    }
}
