//! A camera frame cut down to the model's input: the card's part of the picture, turned
//! upright, scaled to 600 by 375, and normalized as the model was trained.

use crate::ssd::{HEIGHT, WIDTH};

/// One RGBA frame as the camera gives it: `rotation` degrees clockwise make it upright.
#[derive(Clone, Copy, Debug)]
pub struct Frame<'a> {
    pub pixels: &'a [u8],
    pub width: usize,
    pub height: usize,
    pub row_stride: usize,
    pub rotation: u16,
}

/// The card's place in the upright frame, as fractions of its width and height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crop {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Crop {
    pub const WHOLE: Crop = Crop { left: 0.0, top: 0.0, right: 1.0, bottom: 1.0 };
}

// Stripe's training normalization.
const MEAN: f32 = 127.5;
const SPREAD: f32 = 128.5;

impl Frame<'_> {
    fn upright(&self) -> (usize, usize) {
        if self.rotation % 180 == 90 { (self.height, self.width) } else { (self.width, self.height) }
    }

    /// The pixel at upright `(u, v)`, as RGB.
    fn pixel(&self, u: usize, v: usize) -> [f32; 3] {
        let (x, y) = match self.rotation {
            90 => (v, self.height - 1 - u),
            180 => (self.width - 1 - u, self.height - 1 - v),
            270 => (self.width - 1 - v, u),
            _ => (u, v),
        };
        let at = y * self.row_stride + x * 4;
        match self.pixels.get(at..at + 3) {
            Some(p) => [f32::from(p[0]), f32::from(p[1]), f32::from(p[2])],
            None => [0.0; 3],
        }
    }

    /// The model's input from `crop` of this frame: NHWC, RGB, one float per channel. Shrunk,
    /// each output pixel averages the frame pixels under it, each by how much of it lies under;
    /// enlarged, it blends the four nearest. None when the buffer is smaller than its sizes say or
    /// the crop is empty.
    ///
    /// Counting whole pixels only, as this once did, took one source pixel for some outputs and
    /// two for the next whenever the scale was not a whole number, which bent thin strokes. The
    /// model then missed digits on any card held at the wrong distance, and read them only when
    /// the card filled the window at a lucky size.
    pub fn input(&self, crop: Crop) -> Option<Vec<f32>> {
        let needed = self.height.checked_sub(1)?.checked_mul(self.row_stride)?.checked_add(self.width.checked_mul(4)?)?;
        if self.width == 0 || self.pixels.len() < needed || self.row_stride < self.width * 4 {
            return None;
        }
        let (w, h) = self.upright();
        let left = crop.left.clamp(0.0, 1.0) * w as f32;
        let top = crop.top.clamp(0.0, 1.0) * h as f32;
        let span_x = (crop.right.clamp(0.0, 1.0) * w as f32 - left) / WIDTH as f32;
        let span_y = (crop.bottom.clamp(0.0, 1.0) * h as f32 - top) / HEIGHT as f32;
        if span_x <= 0.0 || span_y <= 0.0 {
            return None;
        }
        let columns = taps(left, span_x, WIDTH, w);
        let rows = taps(top, span_y, HEIGHT, h);
        let mut out = Vec::with_capacity(WIDTH * HEIGHT * 3);
        for row in &rows {
            for column in &columns {
                let mut sum = [0f32; 3];
                for &(v, wy) in row {
                    for &(u, wx) in column {
                        let p = self.pixel(u, v);
                        let weight = wy * wx;
                        sum[0] += p[0] * weight;
                        sum[1] += p[1] * weight;
                        sum[2] += p[2] * weight;
                    }
                }
                out.extend(sum.map(|s| (s - MEAN) / SPREAD));
            }
        }
        Some(out)
    }
}

/// For each of `count` outputs along one side, the source pixels it draws on and how much each
/// counts, adding up to one. Output `i` covers source `[start + i * span, start + (i + 1) * span)`
/// of a side `limit` long.
fn taps(start: f32, span: f32, count: usize, limit: usize) -> Vec<Vec<(usize, f32)>> {
    let last = limit.saturating_sub(1);
    (0..count)
        .map(|i| {
            let from = start + i as f32 * span;
            let to = from + span;
            if span < 1.0 {
                // Enlarging: the two pixels either side of the output's centre, by nearness.
                let centre = (from + to) / 2.0 - 0.5;
                let low = centre.floor().max(0.0);
                let near = (centre - low).clamp(0.0, 1.0);
                let a = (low as usize).min(last);
                let b = (a + 1).min(last);
                return if a == b { vec![(a, 1.0)] } else { vec![(a, 1.0 - near), (b, near)] };
            }
            let mut out = Vec::new();
            let mut total = 0.0;
            let mut u = from.floor().max(0.0) as usize;
            while (u as f32) < to && u <= last {
                let covered = (to.min(u as f32 + 1.0) - from.max(u as f32)).max(0.0);
                if covered > 0.0 {
                    out.push((u, covered));
                    total += covered;
                }
                u += 1;
            }
            if out.is_empty() {
                return vec![((from as usize).min(last), 1.0)];
            }
            out.iter_mut().for_each(|t| t.1 /= total);
            out
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A frame `width` by `height` whose pixel (x, y) is red x, green y.
    fn gradient(width: usize, height: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(width * height * 4);
        for y in 0..height {
            for x in 0..width {
                out.extend([x as u8, y as u8, 7, 255]);
            }
        }
        out
    }

    fn rgb(input: &[f32], x: usize, y: usize) -> [f32; 3] {
        let at = (y * WIDTH + x) * 3;
        [0, 1, 2].map(|c| ((input[at + c] * SPREAD + MEAN) * 100.0).round() / 100.0)
    }

    #[test]
    fn same_size_frame_passes_through() {
        let pixels = gradient(600, 375);
        let frame = Frame { pixels: &pixels, width: 600, height: 375, row_stride: 2400, rotation: 0 };
        let input = frame.input(Crop::WHOLE).unwrap_or_default();
        assert_eq!(input.len(), WIDTH * HEIGHT * 3);
        assert_eq!(rgb(&input, 10, 20), [10.0, 20.0, 7.0]);
        assert_eq!(rgb(&input, 255, 255), [255.0, 255.0, 7.0]);
    }

    #[test]
    fn a_quarter_turn_makes_the_frame_upright() {
        // Sensor frame 375 wide, 600 tall; turned 90° clockwise it is 600 by 375.
        let pixels = gradient(375, 600);
        let frame = Frame { pixels: &pixels, width: 375, height: 600, row_stride: 375 * 4, rotation: 90 };
        let input = frame.input(Crop::WHOLE).unwrap_or_default();
        // Upright (u, v) comes from sensor (v, height - 1 - u): (10, 599), whose green wrapped to 87.
        assert_eq!(rgb(&input, 0, 10), [10.0, 87.0, 7.0]);
        // A three-quarter turn reads sensor (width - 1 - v, u) instead: (364, 0).
        let frame = Frame { rotation: 270, ..frame };
        let input = frame.input(Crop::WHOLE).unwrap_or_default();
        assert_eq!(rgb(&input, 0, 10), [108.0, 0.0, 7.0]);
    }

    #[test]
    fn a_crop_is_averaged_down() {
        // 1200 by 750: each input pixel averages a 2 by 2 block.
        let pixels = gradient(1200, 750);
        let frame = Frame { pixels: &pixels, width: 1200, height: 750, row_stride: 4800, rotation: 0 };
        let input = frame.input(Crop::WHOLE).unwrap_or_default();
        assert_eq!(rgb(&input, 3, 4), [6.5, 8.5, 7.0]);
    }

    #[test]
    fn a_scale_that_is_not_whole_weighs_each_pixel_by_its_share() {
        // 900 wide: each output covers one and a half input pixels.
        let pixels = gradient(900, 375);
        let frame = Frame { pixels: &pixels, width: 900, height: 375, row_stride: 3600, rotation: 0 };
        let input = frame.input(Crop::WHOLE).unwrap_or_default();
        // Output 0 is all of pixel 0 and half of pixel 1; output 1 is the other half and pixel 2.
        assert_eq!(rgb(&input, 0, 5)[0], 0.33);
        assert_eq!(rgb(&input, 1, 5)[0], 1.67);
    }

    #[test]
    fn a_small_frame_is_blended_up() {
        // 300 wide: each input pixel spreads over two outputs.
        let pixels = gradient(300, 375);
        let frame = Frame { pixels: &pixels, width: 300, height: 375, row_stride: 1200, rotation: 0 };
        let input = frame.input(Crop::WHOLE).unwrap_or_default();
        assert_eq!(rgb(&input, 20, 5)[0], 9.75);
        assert_eq!(rgb(&input, 21, 5)[0], 10.25);
    }

    #[test]
    fn short_buffers_and_empty_crops_are_refused() {
        let pixels = gradient(10, 10);
        let frame = Frame { pixels: &pixels[..300], width: 10, height: 10, row_stride: 40, rotation: 0 };
        assert!(frame.input(Crop::WHOLE).is_none());
        let frame = Frame { pixels: &pixels, width: 10, height: 10, row_stride: 40, rotation: 0 };
        assert!(frame.input(Crop { left: 0.5, top: 0.0, right: 0.5, bottom: 1.0 }).is_none());
    }
}
