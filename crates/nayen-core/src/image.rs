//! Grayscale page buffers and the resampling rules the Indra models expect.
//!
//! Every stage downstream works on an ink map where `0.0` is paper and `1.0` is
//! ink, so a page is normalised once here and never re-derived.

use serde::{Deserialize, Serialize};

/// A single-channel page. Pixel `(x, y)` lives at `data[y * width + x]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gray8 {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

impl Gray8 {
    pub fn new(width: usize, height: usize, data: Vec<u8>) -> Result<Self, ImageError> {
        if width == 0 || height == 0 {
            return Err(ImageError::Empty);
        }
        if data.len() != width * height {
            return Err(ImageError::LengthMismatch {
                expected: width * height,
                got: data.len(),
            });
        }
        Ok(Self {
            width,
            height,
            data,
        })
    }

    /// A blank page of the given size, filled with `value`.
    pub fn filled(width: usize, height: usize, value: u8) -> Result<Self, ImageError> {
        if width == 0 || height == 0 {
            return Err(ImageError::Empty);
        }
        Ok(Self {
            width,
            height,
            data: vec![value; width * height],
        })
    }

    #[inline]
    pub fn get(&self, x: usize, y: usize) -> u8 {
        self.data[y * self.width + x]
    }

    /// Convert an RGBA or RGB byte buffer. Alpha is ignored: scanned forms are
    /// opaque, and compositing onto an unknown backdrop would invent pixels.
    pub fn from_rgb(buf: &[u8], width: usize, height: usize, channels: usize) -> Result<Self, ImageError> {
        if channels != 3 && channels != 4 {
            return Err(ImageError::UnsupportedChannels(channels));
        }
        if buf.len() < width * height * channels {
            return Err(ImageError::LengthMismatch {
                expected: width * height * channels,
                got: buf.len(),
            });
        }
        let mut data = Vec::with_capacity(width * height);
        for px in 0..width * height {
            let o = px * channels;
            // Rec. 709 luma, in integer fixed point (x1024) to stay branch-free.
            let luma = (buf[o] as u32 * 218 + buf[o + 1] as u32 * 732 + buf[o + 2] as u32 * 74) >> 10;
            data.push(luma.min(255) as u8);
        }
        Self::new(width, height, data)
    }

    /// Resize with the interpolation rule `cv2.resize` uses for each direction:
    /// area averaging when shrinking, bilinear when growing. Matching that
    /// choice matters because `det.onnx` and `rec.onnx` were trained on it.
    pub fn resize(&self, nw: usize, nh: usize) -> Result<Self, ImageError> {
        if nw == 0 || nh == 0 {
            return Err(ImageError::Empty);
        }
        if nw == self.width && nh == self.height {
            return Ok(self.clone());
        }
        let shrinking = (nw as f64) / (self.width as f64) < 1.0 || (nh as f64) / (self.height as f64) < 1.0;
        if shrinking {
            self.resize_area(nw, nh)
        } else {
            self.resize_bilinear(nw, nh)
        }
    }

    /// `cv2.INTER_AREA`: exact fractional-area averaging, separable.
    ///
    /// A plain box mean is not equivalent. Each output pixel covers a fractional
    /// source span, and the source pixels at the span's edges contribute only
    /// partially. On 4px-wide glyph strokes that difference is several percent
    /// of the ink mass, which is enough to shift what the recognizer sees. The
    /// weights are precomputed per axis, so this stays O(pixels).
    fn resize_area(&self, nw: usize, nh: usize) -> Result<Self, ImageError> {
        let wx = area_weights(self.width, nw);
        let wy = area_weights(self.height, nh);

        // Horizontal pass into f64, so the vertical pass does not quantise twice.
        let mut mid = vec![0f64; self.height * nw];
        for y in 0..self.height {
            let src = &self.data[y * self.width..(y + 1) * self.width];
            let dst = &mut mid[y * nw..(y + 1) * nw];
            for (ox, w) in wx.iter().enumerate() {
                let mut acc = 0f64;
                for &(i, f) in w.iter() {
                    acc += src[i] as f64 * f;
                }
                dst[ox] = acc;
            }
        }

        let mut data = vec![0u8; nw * nh];
        for (oy, w) in wy.iter().enumerate() {
            for ox in 0..nw {
                let mut acc = 0f64;
                for &(i, f) in w.iter() {
                    acc += mid[i * nw + ox] * f;
                }
                data[oy * nw + ox] = acc.round().clamp(0.0, 255.0) as u8;
            }
        }
        Self::new(nw, nh, data)
    }

    /// Bilinear upsample, matching `cv2.INTER_LINEAR`'s half-pixel centre
    /// convention so upscaled crops land on the same subpixel grid.
    fn resize_bilinear(&self, nw: usize, nh: usize) -> Result<Self, ImageError> {
        let sx = self.width as f64 / nw as f64;
        let sy = self.height as f64 / nh as f64;
        let mut data = vec![0u8; nw * nh];
        for oy in 0..nh {
            let fy = ((oy as f64 + 0.5) * sy - 0.5).clamp(0.0, (self.height - 1) as f64);
            let y0 = fy.floor() as usize;
            let y1 = (y0 + 1).min(self.height - 1);
            let wy = fy - y0 as f64;
            for ox in 0..nw {
                let fx = ((ox as f64 + 0.5) * sx - 0.5).clamp(0.0, (self.width - 1) as f64);
                let x0 = fx.floor() as usize;
                let x1 = (x0 + 1).min(self.width - 1);
                let wx = fx - x0 as f64;
                let p00 = self.data[y0 * self.width + x0] as f64;
                let p01 = self.data[y0 * self.width + x1] as f64;
                let p10 = self.data[y1 * self.width + x0] as f64;
                let p11 = self.data[y1 * self.width + x1] as f64;
                let top = p00 + (p01 - p00) * wx;
                let bot = p10 + (p11 - p10) * wx;
                data[oy * nw + ox] = (top + (bot - top) * wy).round().clamp(0.0, 255.0) as u8;
            }
        }
        Self::new(nw, nh, data)
    }

    /// Median of the whole page, used by the deskew search.
    pub fn median(&self) -> u8 {
        let mut hist = [0u32; 256];
        for &p in &self.data {
            hist[p as usize] += 1;
        }
        let half = self.data.len() as u32 / 2;
        let mut acc = 0u32;
        for (v, &c) in hist.iter().enumerate() {
            acc += c;
            if acc > half {
                return v as u8;
            }
        }
        255
    }
}

/// Per-output-pixel source weights for area resampling.
///
/// For output pixel `o` the source span is `[o * scale, (o + 1) * scale)`. Each
/// source pixel `i` contributes the length of its overlap with that span,
/// normalised so the weights sum to 1. Weights are normalised by the actual
/// covered span rather than by `scale`, so the last pixel of an axis — where the
/// span is clipped by the source extent — still averages correctly.
fn area_weights(src: usize, dst: usize) -> Vec<Vec<(usize, f64)>> {
    if dst == 0 {
        return Vec::new();
    }
    let scale = src as f64 / dst as f64;
    let mut out = Vec::with_capacity(dst);
    for o in 0..dst {
        let a = o as f64 * scale;
        let b = ((o as f64 + 1.0) * scale).min(src as f64);
        let i0 = (a.floor() as usize).min(src.saturating_sub(1));
        let i1 = ((b.ceil() as usize).max(i0 + 1)).min(src);
        let mut total = 0f64;
        let mut ws: Vec<(usize, f64)> = Vec::with_capacity(i1 - i0);
        for i in i0..i1 {
            let lo = (i as f64).max(a);
            let hi = (i as f64 + 1.0).min(b);
            let w = (hi - lo).max(0.0);
            if w > 0.0 {
                ws.push((i, w));
                total += w;
            }
        }
        if total > 0.0 {
            for (_, w) in ws.iter_mut() {
                *w /= total;
            }
        } else {
            // Degenerate span (scale far below one pixel): nearest neighbour.
            ws.push((i0, 1.0));
        }
        out.push(ws);
    }
    out
}

/// A float ink map: `0.0` = paper, `1.0` = ink. This is what every model eats.
#[derive(Clone, Debug, PartialEq)]
pub struct InkMap {
    pub width: usize,
    pub height: usize,
    pub data: Vec<f32>,
}

impl InkMap {
    pub fn zeros(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            data: vec![0.0; width * height],
        }
    }

    #[inline]
    pub fn get(&self, x: usize, y: usize) -> f32 {
        self.data[y * self.width + x]
    }

    #[inline]
    pub fn set(&mut self, x: usize, y: usize, v: f32) {
        self.data[y * self.width + x] = v;
    }
}

/// `det_post.page_to_input`: shrink so the long side is `long_side`, pad to a
/// multiple of 32 with paper, and invert to an ink map. Returns the map plus
/// the scale factor needed to project boxes back to original page pixels.
pub fn page_to_input(gray: &Gray8, long_side: usize) -> Result<(InkMap, f64), ImageError> {
    const INPUT_MULTIPLE: usize = 32;
    let longest = gray.width.max(gray.height);
    let scale = long_side as f64 / longest as f64;
    let nh = ((gray.height as f64 * scale).round() as usize).max(1);
    let nw = ((gray.width as f64 * scale).round() as usize).max(1);

    let small = gray.resize(nw, nh)?;
    let ph = nh.div_ceil(INPUT_MULTIPLE) * INPUT_MULTIPLE;
    let pw = nw.div_ceil(INPUT_MULTIPLE) * INPUT_MULTIPLE;

    let mut out = InkMap::zeros(pw, ph);
    for y in 0..nh {
        for x in 0..nw {
            // Padding stays 0.0 (paper), which is exactly what the caller wants.
            out.set(x, y, 1.0 - small.get(x, y) as f32 / 255.0);
        }
    }
    Ok((out, scale))
}

/// Percentiles straight from a 256-bin histogram, with numpy's linear
/// interpolation. `_percentiles` in `rec_preproc.py` avoids a full sort for the
/// same reason: this runs on every detected line.
pub fn percentiles(values: &[u8], qs: &[f64]) -> Vec<f64> {
    let n = values.len();
    if n == 0 {
        return vec![0.0; qs.len()];
    }
    let mut hist = [0usize; 256];
    for &v in values {
        hist[v as usize] += 1;
    }
    let mut cum = [0usize; 256];
    let mut acc = 0usize;
    for i in 0..256 {
        acc += hist[i];
        cum[i] = acc;
    }
    // searchsorted(cum, i, side="right"): first bucket whose cumulative count
    // exceeds the rank.
    let rank = |k: usize| -> f64 {
        (0..256).find(|&b| cum[b] > k).unwrap_or(255) as f64
    };

    qs.iter()
        .map(|&q| {
            let x = q * (n as f64 - 1.0);
            let i = x.floor();
            let g = x - i;
            let lo = rank(i as usize);
            let hi = rank((i as usize + 1).min(n - 1));
            let d = hi - lo;
            if g >= 0.5 {
                hi - d * (1.0 - g)
            } else {
                lo + d * g
            }
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImageError {
    Empty,
    LengthMismatch { expected: usize, got: usize },
    UnsupportedChannels(usize),
}

impl std::fmt::Display for ImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "image has a zero dimension"),
            Self::LengthMismatch { expected, got } => {
                write!(f, "pixel buffer length {got}, expected {expected}")
            }
            Self::UnsupportedChannels(c) => write!(f, "unsupported channel count {c}"),
        }
    }
}

impl std::error::Error for ImageError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb_to_gray_uses_luma() {
        let g = Gray8::from_rgb(&[255, 0, 0], 1, 1, 3).unwrap();
        assert_eq!(g.get(0, 0), 54); // 0.2126 * 255
        let g = Gray8::from_rgb(&[0, 255, 0], 1, 1, 3).unwrap();
        assert_eq!(g.get(0, 0), 182);
        let g = Gray8::from_rgb(&[0, 0, 255], 1, 1, 3).unwrap();
        assert_eq!(g.get(0, 0), 18);
    }

    #[test]
    fn page_to_input_pads_to_multiple_of_32_and_inverts() {
        // 100x40 black-on-white page -> long side 100 scaled to 64.
        let page = Gray8::filled(100, 40, 255).unwrap();
        let (ink, scale) = page_to_input(&page, 64).unwrap();
        assert_eq!(ink.width % 32, 0);
        assert_eq!(ink.height % 32, 0);
        assert!((scale - 0.64).abs() < 1e-9);
        // Pure white page -> all paper.
        assert!(ink.data.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn page_to_input_ink_is_one_for_black() {
        let page = Gray8::filled(32, 32, 0).unwrap();
        let (ink, _) = page_to_input(&page, 32).unwrap();
        assert!(ink.data.iter().all(|&v| (v - 1.0).abs() < 1e-6));
    }

    #[test]
    fn downscale_area_averages() {
        let g = Gray8::new(2, 2, vec![0, 255, 255, 0]).unwrap();
        let r = g.resize(1, 1).unwrap();
        assert!((r.get(0, 0) as i32 - 128).abs() <= 1, "exact mean is 127.5");
    }

    #[test]
    fn area_weights_sum_to_one() {
        for (src, dst) in [(400, 173), (110, 96), (7, 3), (3, 7)] {
            let w = super::area_weights(src, dst);
            assert_eq!(w.len(), dst);
            for row in &w {
                let sum: f64 = row.iter().map(|(_, f)| *f).sum();
                assert!((sum - 1.0).abs() < 1e-9, "src {src} -> dst {dst}: weights sum to {sum}");
                assert!(row.iter().all(|&(i, _)| i < src), "index in range");
            }
        }
    }

    #[test]
    fn area_resample_preserves_total_ink_approximately() {
        // Downscaling a striped pattern must not lose or gain ink mass, which a
        // naive box mean does when the scale is not an integer.
        let mut data = vec![255u8; 300 * 100];
        for y in 0..100 {
            for x in (0..300).step_by(6) {
                for xx in x..(x + 3).min(300) {
                    data[y * 300 + xx] = 0;
                }
            }
        }
        let g = Gray8::new(300, 100, data).unwrap();
        let src_mean = g.data.iter().map(|&v| v as f64).sum::<f64>() / g.data.len() as f64;
        for (nw, nh) in [(173, 58), (96, 32), (250, 83)] {
            let r = g.resize(nw, nh).unwrap();
            let dst_mean = r.data.iter().map(|&v| v as f64).sum::<f64>() / r.data.len() as f64;
            assert!(
                (dst_mean - src_mean).abs() / src_mean < 0.02,
                "{nw}x{nh}: mean {dst_mean} vs source {src_mean}"
            );
        }
    }

    #[test]
    fn percentile_matches_known_median() {
        let v: Vec<u8> = (0..=100).collect();
        let p = percentiles(&v, &[0.5]);
        assert!((p[0] - 50.0).abs() < 1.0);
    }
}
