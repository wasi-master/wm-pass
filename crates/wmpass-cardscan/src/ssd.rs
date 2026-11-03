//! The detector's two outputs as digit boxes.
//!
//! Ported from Stripe CardScan's decoding in stripe-android (`stripecardscan`: `SSDOcr.kt`,
//! `SSD.kt`, `CombinePriors.kt`, `SizeAndCenter.kt`, `NonMaximumSuppression.kt`),
//! MIT License, Copyright (c) 2011- Stripe, Inc. The constants are the ones the model was
//! trained with and must not change without it.

use std::sync::LazyLock;

/// The model's input, a card filling the frame.
pub const WIDTH: usize = 600;
pub const HEIGHT: usize = 375;

/// Background, then 1 to 9, then 0.
const CLASSES: usize = 11;
const COORDINATES: usize = 4;
const PRIORS_PER_CELL: usize = 3;

/// Each output layer: its feature map (width, height), shrinkage, and smallest and largest box.
const LAYERS: [Layer; 2] = [
    Layer { width: 38, height: 24, shrinkage: 16.0, min: 14.0, max: 30.0 },
    Layer { width: 19, height: 12, shrinkage: 31.0, min: 30.0, max: 45.0 },
];
const ASPECT: f32 = 3.0;

/// Every box the model scores: 3420.
pub const PRIORS: usize = (38 * 24 + 19 * 12) * PRIORS_PER_CELL;
pub const CLASS_OUTPUTS: usize = PRIORS * CLASSES;
pub const LOCATION_OUTPUTS: usize = PRIORS * COORDINATES;

const PROBABILITY: f32 = 0.5;
const OVERLAP: f32 = 0.5;
const PER_CLASS: usize = 20;
const CANDIDATES: usize = 200;
const CENTER_VARIANCE: f32 = 0.1;
const SIZE_VARIANCE: f32 = 0.2;

struct Layer {
    width: usize,
    height: usize,
    shrinkage: f32,
    min: f32,
    max: f32,
}

/// A box as fractions of the input: centre and size.
#[derive(Clone, Copy, Debug)]
struct Prior {
    cx: f32,
    cy: f32,
    w: f32,
    h: f32,
}

/// One digit found, with its box as fractions of the input.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Digit {
    pub value: u8,
    pub confidence: f32,
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Digit {
    pub fn cx(&self) -> f32 {
        (self.left + self.right) / 2.0
    }

    pub fn cy(&self) -> f32 {
        (self.top + self.bottom) / 2.0
    }

    pub fn width(&self) -> f32 {
        self.right - self.left
    }

    pub fn height(&self) -> f32 {
        self.bottom - self.top
    }

    /// Intersection over union, with sizes clamped at 0 so boxes apart give 0.
    pub fn overlap(&self, other: &Digit) -> f32 {
        let area = |w: f32, h: f32| w.clamp(0.0, 1000.0) * h.clamp(0.0, 1000.0);
        let shared = area(
            self.right.min(other.right) - self.left.max(other.left),
            self.bottom.min(other.bottom) - self.top.max(other.top),
        );
        shared / (area(self.width(), self.height()) + area(other.width(), other.height()) - shared + 0.00001)
    }
}

static PRIOR_BOXES: LazyLock<Vec<Prior>> = LazyLock::new(|| {
    let mut out = Vec::with_capacity(PRIORS);
    let ratio = ASPECT.sqrt();
    for layer in &LAYERS {
        let scale_w = WIDTH as f32 / layer.shrinkage;
        let scale_h = HEIGHT as f32 / layer.shrinkage;
        for index in 0..layer.width * layer.height * PRIORS_PER_CELL {
            let row = index / PRIORS_PER_CELL / layer.width;
            let column = (index / PRIORS_PER_CELL) % layer.width;
            let (size, ratio) = match index % PRIORS_PER_CELL {
                0 => (layer.min, 1.0),
                1 => ((layer.max * layer.min).sqrt(), ratio),
                _ => (layer.min, ratio),
            };
            out.push(Prior {
                cx: ((column as f32 + 0.5) / scale_w).clamp(0.0, 1.0),
                cy: ((row as f32 + 0.5) / scale_h).clamp(0.0, 1.0),
                w: (size / WIDTH as f32).clamp(0.0, 1.0),
                h: (size / HEIGHT as f32 * ratio).clamp(0.0, 1.0),
            });
        }
    }
    out
});

/// Where value `at` of prior `prior` sits in an output with `per_prior` values a prior. The
/// model writes each layer with the feature map's row as the fastest axis; priors run row,
/// column, then the three shapes of a cell.
fn index(per_prior: usize, prior: usize, at: usize) -> usize {
    let mut offset = 0;
    let mut first = 0;
    for layer in &LAYERS {
        let count = layer.width * layer.height * PRIORS_PER_CELL;
        if prior < first + count {
            let local = prior - first;
            let row = local / (layer.width * PRIORS_PER_CELL);
            let rest = local % (layer.width * PRIORS_PER_CELL);
            return offset + row + (rest * per_prior + at) * layer.height;
        }
        offset += count * per_prior;
        first += count;
    }
    usize::MAX
}

fn at(output: &[f32], per_prior: usize, prior: usize, at: usize) -> f32 {
    output.get(index(per_prior, prior, at)).copied().unwrap_or(0.0)
}

/// The digits in the model's two outputs, `classes` ([`CLASS_OUTPUTS`] scores) and
/// `locations` ([`LOCATION_OUTPUTS`] offsets). At most [`PER_CLASS`] of each digit.
pub fn decode(classes: &[f32], locations: &[f32]) -> Vec<Digit> {
    if classes.len() != CLASS_OUTPUTS || locations.len() != LOCATION_OUTPUTS {
        return Vec::new();
    }
    let mut boxes = Vec::with_capacity(PRIORS);
    let mut scores = vec![0f32; PRIORS * CLASSES];
    for (prior, base) in PRIOR_BOXES.iter().enumerate() {
        let cx = at(locations, COORDINATES, prior, 0) * CENTER_VARIANCE * base.w + base.cx;
        let cy = at(locations, COORDINATES, prior, 1) * CENTER_VARIANCE * base.h + base.cy;
        let w = (at(locations, COORDINATES, prior, 2) * SIZE_VARIANCE).exp() * base.w;
        let h = (at(locations, COORDINATES, prior, 3) * SIZE_VARIANCE).exp() * base.h;
        boxes.push([cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0]);
        let row = &mut scores[prior * CLASSES..(prior + 1) * CLASSES];
        for (class, score) in row.iter_mut().enumerate() {
            *score = at(classes, CLASSES, prior, class).exp();
        }
        let sum: f32 = row.iter().sum();
        row.iter_mut().for_each(|score| *score /= sum);
    }
    let mut found = Vec::new();
    for class in 1..CLASSES {
        let mut candidates: Vec<Digit> = (0..PRIORS)
            .filter(|&prior| scores[prior * CLASSES + class] >= PROBABILITY)
            .map(|prior| {
                let [left, top, right, bottom] = boxes[prior];
                Digit {
                    value: if class == 10 { 0 } else { class as u8 },
                    confidence: scores[prior * CLASSES + class],
                    left,
                    top,
                    right,
                    bottom,
                }
            })
            .collect();
        candidates.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
        candidates.truncate(CANDIDATES);
        let mut kept: Vec<Digit> = Vec::new();
        for candidate in candidates {
            if kept.len() == PER_CLASS {
                break;
            }
            if kept.iter().all(|k| k.overlap(&candidate) < OVERLAP) {
                kept.push(candidate);
            }
        }
        found.extend(kept);
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priors_cover_both_layers() {
        assert_eq!(PRIOR_BOXES.len(), PRIORS);
        assert_eq!(CLASS_OUTPUTS, 37620);
        assert_eq!(LOCATION_OUTPUTS, 13680);
        let first = PRIOR_BOXES[0];
        assert!((first.cx - 8.0 / 600.0).abs() < 1e-6);
        assert!((first.cy - 8.0 / 375.0).abs() < 1e-6);
    }

    #[test]
    fn output_order_is_row_fastest() {
        // Prior 0 of the first layer: row 0, column 0, shape 0. Its second value comes one
        // feature map height later, not one place later.
        let mut output = vec![0f32; LOCATION_OUTPUTS];
        output[24] = 7.0;
        assert_eq!(at(&output, COORDINATES, 0, 1), 7.0);
        // The second layer starts after all of the first.
        let second = 38 * 24 * PRIORS_PER_CELL;
        output[second * COORDINATES] = 3.0;
        assert_eq!(at(&output, COORDINATES, second, 0), 3.0);
    }

    #[test]
    fn every_value_has_its_own_place() {
        let mut seen = vec![false; CLASS_OUTPUTS];
        for prior in 0..PRIORS {
            for class in 0..CLASSES {
                let i = index(CLASSES, prior, class);
                assert!(!seen[i], "prior {prior} class {class} shares {i}");
                seen[i] = true;
            }
        }
        assert!(seen.iter().all(|s| *s));
    }

    #[test]
    fn background_everywhere_finds_nothing() {
        assert!(decode(&[0.0; 3], &[0.0; 3]).is_empty());
        let mut classes = vec![0f32; CLASS_OUTPUTS];
        for prior in 0..PRIORS {
            classes[index(CLASSES, prior, 0)] = 10.0;
        }
        assert!(decode(&classes, &vec![0f32; LOCATION_OUTPUTS]).is_empty());
    }

    #[test]
    fn one_sure_prior_is_one_digit_on_its_prior() {
        let mut classes = vec![0f32; CLASS_OUTPUTS];
        for prior in 0..PRIORS {
            classes[index(CLASSES, prior, 0)] = 10.0;
        }
        let prior = 500;
        classes[index(CLASSES, prior, 0)] = 0.0;
        classes[index(CLASSES, prior, 10)] = 10.0;
        let found = decode(&classes, &vec![0f32; LOCATION_OUTPUTS]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].value, 0);
        let base = PRIOR_BOXES[prior];
        assert!((found[0].cx() - base.cx).abs() < 1e-6);
        assert!((found[0].height() - base.h).abs() < 1e-6);
    }
}
