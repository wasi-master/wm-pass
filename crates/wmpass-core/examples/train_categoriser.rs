//! Trains `data/categoriser.bin`, a naive Bayes model over hashed features.
//!
//! ```bash
//! cargo run -p wmpass-core --example train_categoriser
//! ```
//!
//! The model must exist for the crate to build, so a first run on a clean tree
//! needs an empty placeholder file, which this tool overwrites.
#![allow(clippy::unwrap_used)]

use std::fs;

use wmpass_core::categorise::{BUCKETS, CATEGORIES, SCALE, classify_with, features};

const KEYWORD_WEIGHT: u32 = 8;

struct Sample {
    text: String,
    class: usize,
    weight: u32,
}

fn class_of(name: &str) -> Option<usize> {
    CATEGORIES.iter().position(|c| *c == name)
}

fn root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

fn samples() -> Vec<Sample> {
    let mut out = Vec::new();
    for line in fs::read_to_string(root().join("domains.tsv")).unwrap().lines() {
        let mut columns = line.split('\t');
        let (domain, category) = (columns.next().unwrap(), columns.next().unwrap());
        if let Some(class) = class_of(category) {
            // The name without its public suffix.
            let name = domain.split('.').next().unwrap().to_owned();
            out.push(Sample { text: name, class, weight: 1 });
        }
    }
    for line in fs::read_to_string(root().join("keywords.tsv")).unwrap().lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let (text, category) = line.split_once('\t').unwrap();
        out.push(Sample { text: text.to_owned(), class: class_of(category).unwrap(), weight: KEYWORD_WEIGHT });
    }
    out
}

fn train<'a>(data: impl Iterator<Item = &'a Sample> + Clone) -> Vec<u8> {
    let classes = CATEGORIES.len();
    let mut counts = vec![1f64; classes * BUCKETS]; // Laplace smoothing
    let mut totals = vec![BUCKETS as f64; classes];
    let mut docs = vec![1f64; classes];
    for sample in data {
        docs[sample.class] += f64::from(sample.weight);
        for feature in features(&sample.text) {
            counts[sample.class * BUCKETS + feature] += f64::from(sample.weight);
            totals[sample.class] += f64::from(sample.weight);
        }
    }
    let all_docs: f64 = docs.iter().sum();
    let mut model = b"WMCG".to_vec();
    for doc in &docs {
        model.push(quantise(-(doc / all_docs).ln()));
    }
    for class in 0..classes {
        for bucket in 0..BUCKETS {
            model.push(quantise(-(counts[class * BUCKETS + bucket] / totals[class]).ln()));
        }
    }
    model
}

fn quantise(negative_log: f64) -> u8 {
    (negative_log * f64::from(SCALE)).round().clamp(0.0, 255.0) as u8
}

fn main() {
    let data = samples();
    // Five-fold check on domains only, to report honest numbers.
    let mut asked = 0u32;
    let mut said = 0u32;
    let mut right = 0u32;
    for fold in 0..5 {
        let train_set = data.iter().enumerate().filter(|(i, s)| s.weight > 1 || i % 5 != fold).map(|(_, s)| s);
        let model = train(train_set);
        for (index, sample) in data.iter().enumerate() {
            if sample.weight > 1 || index % 5 != fold {
                continue;
            }
            asked += 1;
            if let Some((class, confidence)) = classify_with(&model, &sample.text)
                && confidence >= wmpass_core::categorise::MIN_CONFIDENCE
            {
                said += 1;
                if class == sample.class {
                    right += 1;
                }
            }
        }
    }
    let model = train(data.iter());
    fs::write(root().join("categoriser.bin"), &model).unwrap();
    println!(
        "{} samples, {} bytes. Held-out names: answered {}/{} ({:.0}%), right on {:.0}% of those.",
        data.len(),
        model.len(),
        said,
        asked,
        100.0 * f64::from(said) / f64::from(asked),
        100.0 * f64::from(right) / f64::from(said.max(1)),
    );
}
