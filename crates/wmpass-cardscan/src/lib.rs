//! Reads a payment card's number, and its expiry when the detector sees it, from camera
//! frames. Everything runs here: no frame is kept, and nothing leaves the device.
//!
//! The model is Stripe CardScan's digit detector (`darknite`, MIT), with its float16
//! weights widened to float32 by `tools/data/widen_cardscan.py` so tract can run it. It finds
//! digits and nothing else, so a card's name cannot be read. [`ssd`] turns its outputs into
//! digit boxes, [`card`] makes a card of those, and [`card::Tally`] waits for two frames to
//! agree.

pub mod card;
pub mod frame;
pub mod ssd;

use std::sync::Arc;

use tract_tflite::prelude::*;

pub use card::{Card, Expiry, Month, Tally, TextLine};
pub use frame::{Crop, Frame};

/// The model would not load, or a frame would not run through it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadError(String);

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ReadError {}

fn failed(what: &str, error: impl std::fmt::Display) -> ReadError {
    ReadError(format!("{what}: {error}"))
}

/// The detector, loaded once and run on each frame.
pub struct Reader {
    plan: Arc<TypedRunnableModel>,
}

impl std::fmt::Debug for Reader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Reader")
    }
}

impl Reader {
    /// Loads the widened model from its bytes.
    pub fn new(model: &[u8]) -> Result<Reader, ReadError> {
        let model = tract_tflite::tflite()
            .model_for_read(&mut &*model)
            .map_err(|e| failed("the card reader would not load", e))?;
        let plan = model
            .into_optimized()
            .and_then(|m| m.into_runnable())
            .map_err(|e| failed("the card reader would not load", e))?;
        Ok(Reader { plan })
    }

    /// The digits in `input`, which is [`ssd::HEIGHT`] by [`ssd::WIDTH`] RGB as
    /// [`Frame::input`] makes it.
    pub fn digits(&self, input: Vec<f32>) -> Result<Vec<ssd::Digit>, ReadError> {
        let tensor = Tensor::from_shape(&[1, ssd::HEIGHT, ssd::WIDTH, 3], &input).map_err(|e| failed("the frame has the wrong size", e))?;
        let outputs = self.plan.run(tvec!(tensor.into())).map_err(|e| failed("the card reader failed", e))?;
        let values = |i: usize| -> Vec<f32> {
            outputs.get(i).and_then(|o| o.to_plain_array_view::<f32>().ok()).map(|v| v.iter().copied().collect()).unwrap_or_default()
        };
        Ok(ssd::decode(&values(0), &values(1)))
    }

    /// What `crop` of `frame` shows, judged against `today` for the expiry.
    pub fn read(&self, frame: &Frame<'_>, crop: Crop, today: Month) -> Result<Card, ReadError> {
        let input = frame.input(crop).ok_or_else(|| ReadError("the frame is not the size it says".to_owned()))?;
        Ok(card::read(&self.digits(input)?, today))
    }
}
