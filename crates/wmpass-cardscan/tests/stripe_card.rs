//! The whole read on Stripe's own test card (`ocr_card_numbers.png` from stripe-android's
//! CardScan tests, MIT): number, expiry and name printed flat on a 600 by 375 card.

use wmpass_cardscan::{Crop, Frame, Month, Reader};

const MODEL: &[u8] = include_bytes!("../../../data/cardscan/darknite.tflite");

fn rgba(png_bytes: &[u8]) -> (Vec<u8>, usize, usize) {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png_bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8() | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().expect("a PNG");
    let mut buffer = vec![0; reader.output_buffer_size().expect("a size")];
    let info = reader.next_frame(&mut buffer).expect("a frame");
    buffer.truncate(info.buffer_size());
    assert_eq!(info.color_type, png::ColorType::Rgba);
    (buffer, info.width as usize, info.height as usize)
}

#[test]
fn reads_stripes_test_card() {
    let reader = Reader::new(MODEL).expect("the model loads");
    let (pixels, width, height) = rgba(include_bytes!("stripe-test-card.png"));
    let frame = Frame { pixels: &pixels, width, height, row_stride: width * 4, rotation: 0 };
    let today = Month { year: 2026, month: 11 };
    let card = reader.read(&frame, Crop::WHOLE, today).expect("a read");
    assert_eq!(card.number.as_deref().map(String::as_str), Some("4242424242424242"));
    // The detector was trained on card numbers. It finds nothing at all on the smaller
    // `07 / 37` line, even zoomed in, so this card's expiry is typed by hand.
    assert_eq!(card.expiry, None);
}
