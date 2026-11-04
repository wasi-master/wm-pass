//! Stripe's detector over a folder of card pictures, as `tools/cardscan/make_cards.py` makes
//! them: how many numbers it reads right, wrong, or not at all. Off by default; run it with
//! `CARD_CORPUS=<folder> cargo test -p wmpass-cardscan --test corpus -- --ignored --nocapture`.

use wmpass_cardscan::{Crop, Frame, Month, Reader, card};

const MODEL: &[u8] = include_bytes!("../../../data/cardscan/darknite.tflite");

fn rgba(path: &std::path::Path) -> (Vec<u8>, usize, usize) {
    let mut decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).expect("a picture")));
    decoder.set_transformations(png::Transformations::normalize_to_color8() | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().expect("a PNG");
    let mut buffer = vec![0; reader.output_buffer_size().expect("a size")];
    let info = reader.next_frame(&mut buffer).expect("a frame");
    buffer.truncate(info.buffer_size());
    (buffer, info.width as usize, info.height as usize)
}

#[test]
#[ignore = "needs CARD_CORPUS, a folder of pictures with truth.json"]
fn reads_a_corpus() {
    let Ok(folder) = std::env::var("CARD_CORPUS") else { return };
    let folder = std::path::PathBuf::from(folder);
    let truth: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(folder.join("truth.json")).expect("truth.json")).expect("JSON");
    let reader = Reader::new(MODEL).expect("the model loads");
    let today = Month { year: 2026, month: 11 };
    let (mut right, mut wrong, mut none, mut seen) = (0, 0, 0, 0);
    let mut files: Vec<_> = truth.as_object().expect("an object").iter().collect();
    files.sort_by(|a, b| a.0.cmp(b.0));
    for (file, facts) in files {
        let (pixels, width, height) = rgba(&folder.join(file));
        let frame = Frame { pixels: &pixels, width, height, row_stride: width * 4, rotation: 0 };
        let card = reader.read(&frame, Crop::WHOLE, today).expect("a read");
        let want = facts["number"].as_str().unwrap_or_default();
        // Whether the digits were there to be read at all, whatever the checksum says: a
        // specimen's made-up number fails Luhn, and the reader rightly refuses it.
        let digits = reader.digits(frame.input(Crop::WHOLE).expect("an input")).expect("digits");
        let raw = card::lines(&digits).iter().any(|line| line.replace(' ', "").contains(want));
        if raw {
            seen += 1;
        }
        let got = card.number.as_deref().map(String::as_str);
        let verdict = match got {
            Some(n) if n == want => {
                right += 1;
                "right"
            }
            Some(_) => {
                wrong += 1;
                "WRONG"
            }
            None => {
                none += 1;
                "none"
            }
        };
        println!("{file} {verdict:5} digits {} {} {}", if raw { "seen" } else { "-   " }, facts["style"].as_str().unwrap_or_default(), facts["font"].as_str().unwrap_or_default());
    }
    println!("stripe: {right} right, {wrong} wrong, {none} unread, of {}; all digits seen in {seen}", right + wrong + none);
    assert_eq!(wrong, 0, "a wrong number is worse than none");
}
