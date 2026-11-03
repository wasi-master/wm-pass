//! A card from digit boxes: lines of digits, the number, the expiry, and agreement between
//! frames. The detector sees digits only, so a slash or a space between digits reads as a gap.

use std::fmt;

use zeroize::Zeroizing;

use crate::ssd::Digit;

/// Digits further apart than this many digit widths have a separator between them.
const GAP: f32 = 0.5;
/// How many frames must read the same value before it is used.
const AGREE: u32 = 2;
/// Different values remembered at once. A misread is forgotten once more turn up.
const REMEMBERED: usize = 12;
/// Further ahead than this, a date is not a card's.
const LONGEST_YEARS: u16 = 20;

/// A calendar month.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Month {
    pub year: u16,
    pub month: u8,
}

impl Month {
    /// The month (UTC) that `ms` since the Unix epoch falls in.
    pub fn at(ms: u64) -> Month {
        // Howard Hinnant's days-to-civil, for days since 1970-01-01.
        let days = i64::try_from(ms / 86_400_000).unwrap_or(0) + 719_468;
        let era = days / 146_097;
        let day_of_era = days - era * 146_097;
        let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let shifted = (5 * day_of_year + 2) / 153;
        let month = if shifted < 10 { shifted + 3 } else { shifted - 9 };
        let year = year_of_era + era * 400 + i64::from(month <= 2);
        Month { year: u16::try_from(year).unwrap_or(u16::MAX), month: u8::try_from(month).unwrap_or(1) }
    }
}

/// A card's expiry, written `MM/YYYY`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Expiry(Month);

impl Expiry {
    pub fn month(&self) -> u8 {
        self.0.month
    }

    pub fn year(&self) -> u16 {
        self.0.year
    }
}

impl fmt::Display for Expiry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}/{}", self.0.month, self.0.year)
    }
}

/// What one frame showed.
#[derive(Default, PartialEq, Eq)]
pub struct Card {
    pub number: Option<Zeroizing<String>>,
    pub expiry: Option<Expiry>,
}

impl fmt::Debug for Card {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never the number itself, so a log line cannot carry it.
        f.debug_struct("Card").field("number", &self.number.is_some()).field("expiry", &self.expiry).finish()
    }
}

/// The Luhn check every payment card number passes. Digits only.
pub fn luhn(digits: &str) -> bool {
    let mut sum = 0u32;
    for (place, c) in digits.bytes().rev().enumerate() {
        if !c.is_ascii_digit() {
            return false;
        }
        let mut d = u32::from(c - b'0');
        if place % 2 == 1 {
            d *= 2;
            if d > 9 {
                d -= 9;
            }
        }
        sum += d;
    }
    !digits.is_empty() && sum.is_multiple_of(10)
}

/// A card number in `text`: 13 to 19 digits, spaces allowed between groups, that pass
/// [`luhn`]. Anything else in the text, or too few or too many digits, is no number.
pub fn number(text: &str) -> Option<Zeroizing<String>> {
    let text = text.trim();
    if text.is_empty() || !text.bytes().all(|c| c.is_ascii_digit() || c == b' ') {
        return None;
    }
    let digits = Zeroizing::new(text.chars().filter(char::is_ascii_digit).collect::<String>());
    ((13..=19).contains(&digits.len()) && luhn(&digits)).then_some(digits)
}

/// The latest expiry in `text`, as `MM/YY` or `MM/YYYY`, that is not before `today` nor
/// more than 20 years after it. The separator may be `/`, `-` or a gap (a space), since the
/// detector sees no slash. Digits next to the date make it part of something else.
pub fn expiry(text: &str, today: Month) -> Option<Expiry> {
    let bytes = text.as_bytes();
    let digit = |i: usize| bytes.get(i).is_some_and(u8::is_ascii_digit);
    let mut best = None;
    for start in 0..bytes.len() {
        if !digit(start) || !digit(start + 1) || (start > 0 && digit(start - 1)) {
            continue;
        }
        // The separator, with any spaces around it, as `07 / 37` prints.
        let mut at = start + 2;
        while bytes.get(at) == Some(&b' ') {
            at += 1;
        }
        if matches!(bytes.get(at), Some(b'/' | b'-')) {
            at += 1;
            while bytes.get(at) == Some(&b' ') {
                at += 1;
            }
        }
        if at == start + 2 || !digit(at) || !digit(at + 1) {
            continue;
        }
        let year_len = if digit(at + 2) && digit(at + 3) && !digit(at + 4) {
            4
        } else if !digit(at + 2) {
            2
        } else {
            continue;
        };
        let number = |from: usize, len: usize| text.get(from..from + len).and_then(|s| s.parse::<u16>().ok());
        let (Some(month), Some(year)) = (number(start, 2), number(at, year_len)) else { continue };
        let year = if year_len == 2 { 2000 + year } else { year };
        let Ok(month) = u8::try_from(month) else { continue };
        let date = Month { year, month };
        if (1..=12).contains(&month) && date >= today && year <= today.year + LONGEST_YEARS {
            best = best.max(Some(Expiry(date)));
        }
    }
    best
}

/// The digits a frame found as lines of text, top to bottom: a space wherever two digits
/// stand apart, and a duplicate of a digit (another class on the same spot) dropped.
pub fn lines(digits: &[Digit]) -> Vec<String> {
    let mut sorted: Vec<Digit> = digits.to_vec();
    sorted.sort_by(|a, b| a.cy().total_cmp(&b.cy()));
    let mut rows: Vec<Vec<Digit>> = Vec::new();
    for digit in sorted {
        let joins = rows.last().is_some_and(|row| {
            let cy = row.iter().map(Digit::cy).sum::<f32>() / row.len() as f32;
            let height = row.iter().map(Digit::height).fold(digit.height(), f32::max);
            (digit.cy() - cy).abs() <= height / 2.0
        });
        if joins && let Some(row) = rows.last_mut() {
            row.push(digit);
        } else {
            rows.push(vec![digit]);
        }
    }
    rows.into_iter()
        .map(|mut row| {
            row.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
            let mut kept: Vec<Digit> = Vec::new();
            for digit in row {
                if kept.iter().all(|k| k.overlap(&digit) < 0.5) {
                    kept.push(digit);
                }
            }
            kept.sort_by(|a, b| a.left.total_cmp(&b.left));
            let mut widths: Vec<f32> = kept.iter().map(Digit::width).collect();
            widths.sort_by(f32::total_cmp);
            let width = widths.get(widths.len() / 2).copied().unwrap_or(0.0);
            let mut text = String::with_capacity(kept.len() * 2);
            for (i, digit) in kept.iter().enumerate() {
                if i > 0 && digit.left - kept[i - 1].right > GAP * width {
                    text.push(' ');
                }
                text.push(char::from(b'0' + digit.value));
            }
            text
        })
        .collect()
}

/// What `digits` from one frame show: the number from one line, or from four lines of four
/// as some cards print it, and the latest expiry on any other line.
pub fn read(digits: &[Digit], today: Month) -> Card {
    let lines = lines(digits);
    let mut used = None;
    let mut found = None;
    for (i, line) in lines.iter().enumerate() {
        if let Some(n) = number(line) {
            found = Some(n);
            used = Some(i..i + 1);
            break;
        }
    }
    if found.is_none() {
        for (i, four) in lines.windows(4).enumerate() {
            if four.iter().all(|l| l.len() == 4 && l.bytes().all(|c| c.is_ascii_digit())) {
                let joined = Zeroizing::new(four.concat());
                if let Some(n) = number(&joined) {
                    found = Some(n);
                    used = Some(i..i + 4);
                    break;
                }
            }
        }
    }
    let expiry = lines
        .iter()
        .enumerate()
        .filter(|(i, _)| !used.as_ref().is_some_and(|u| u.contains(i)))
        .filter_map(|(_, line)| expiry(line, today))
        .max();
    Card { number: found, expiry }
}

/// Reads over several frames. A value is used only once [`AGREE`] frames read it the same.
#[derive(Default)]
pub struct Tally {
    numbers: Vec<(Zeroizing<String>, u32)>,
    expiries: Vec<(Expiry, u32)>,
}

impl fmt::Debug for Tally {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tally").field("numbers", &self.numbers.len()).field("expiries", &self.expiries.len()).finish()
    }
}

fn count<T: PartialEq>(seen: &mut Vec<(T, u32)>, value: T) {
    if let Some(entry) = seen.iter_mut().find(|(v, _)| *v == value) {
        entry.1 += 1;
    } else {
        if seen.len() == REMEMBERED {
            seen.remove(0);
        }
        seen.push((value, 1));
    }
}

impl Tally {
    pub fn add(&mut self, card: Card) {
        if let Some(number) = card.number {
            count(&mut self.numbers, number);
        }
        if let Some(expiry) = card.expiry {
            count(&mut self.expiries, expiry);
        }
    }

    /// The number most frames agree on, once [`AGREE`] of them do.
    pub fn number(&self) -> Option<&str> {
        self.numbers.iter().filter(|(_, n)| *n >= AGREE).max_by_key(|(_, n)| *n).map(|(v, _)| v.as_str())
    }

    /// The expiry most frames agree on, once [`AGREE`] of them do.
    pub fn expiry(&self) -> Option<Expiry> {
        self.expiries.iter().filter(|(_, n)| *n >= AGREE).max_by_key(|(_, n)| *n).map(|(v, _)| *v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOV_2026: Month = Month { year: 2026, month: 11 };

    fn digit(value: u8, left: f32, top: f32) -> Digit {
        Digit { value, confidence: 0.9, left, top, right: left + 0.03, bottom: top + 0.06 }
    }

    /// A row of digits from `text`, one per character, a space leaving a digit's room.
    fn row(text: &str, left: f32, top: f32) -> Vec<Digit> {
        let mut x = left;
        let mut out = Vec::new();
        for c in text.chars() {
            if let Some(d) = c.to_digit(10) {
                out.push(digit(d as u8, x, top));
            }
            x += if c == ' ' { 0.03 } else { 0.035 };
        }
        out
    }

    #[test]
    fn luhn_accepts_real_numbers_only() {
        for good in ["4242424242424242", "378282246310005", "6011111111111117", "5555555555554444", "4222222222222"] {
            assert!(luhn(good), "{good}");
        }
        for bad in ["4242424242424241", "", "42424242a4242424"] {
            assert!(!luhn(bad), "{bad}");
        }
    }

    #[test]
    fn number_allows_spaces_between_groups() {
        assert_eq!(number("4242 4242 4242 4242").as_deref().map(String::as_str), Some("4242424242424242"));
        assert_eq!(number("3782 822463 10005").as_deref().map(String::as_str), Some("378282246310005"));
        assert_eq!(number("4222222222222").as_deref().map(String::as_str), Some("4222222222222"));
        // 19 digits.
        assert!(number("6011 0000 0000 0000 001").is_some_and(|n| n.len() == 19));
    }

    #[test]
    fn number_rejects_wrong_lengths_and_other_text() {
        assert!(number("4242 4242 4242 4241").is_none());
        assert!(number("424242424242").is_none());
        assert!(number("42424242424242424242").is_none());
        assert!(number("4242-4242-4242-4242").is_none());
        assert!(number("VISA 4242 4242 4242 4242").is_none());
        assert!(number("").is_none());
    }

    #[test]
    fn expiry_reads_both_year_lengths() {
        assert_eq!(expiry("08/29", NOV_2026).map(|e| e.to_string()).as_deref(), Some("08/2029"));
        assert_eq!(expiry("08/2029", NOV_2026).map(|e| e.to_string()).as_deref(), Some("08/2029"));
        assert_eq!(expiry("VALID THRU 12/26", NOV_2026).map(|e| e.to_string()).as_deref(), Some("12/2026"));
        assert_eq!(expiry("07 / 37", NOV_2026).map(|e| e.to_string()).as_deref(), Some("07/2037"));
        // The detector's gap where the slash was.
        assert_eq!(expiry("03 28", NOV_2026).map(|e| e.to_string()).as_deref(), Some("03/2028"));
        assert_eq!(expiry("03-28", NOV_2026).map(|e| e.to_string()).as_deref(), Some("03/2028"));
    }

    #[test]
    fn expiry_this_month_is_still_good() {
        assert_eq!(expiry("11/26", NOV_2026).map(|e| e.to_string()).as_deref(), Some("11/2026"));
    }

    #[test]
    fn expiry_rejects_past_dates_and_non_dates() {
        assert!(expiry("10/26", NOV_2026).is_none());
        assert!(expiry("08/2025", NOV_2026).is_none());
        assert!(expiry("13/29", NOV_2026).is_none());
        assert!(expiry("00/29", NOV_2026).is_none());
        assert!(expiry("01/2099", NOV_2026).is_none());
        assert!(expiry("0829", NOV_2026).is_none());
        assert!(expiry("1234 5678", NOV_2026).is_none());
        assert!(expiry("4242 4242 4242 4242", NOV_2026).is_none());
        assert!(expiry("08/295", NOV_2026).is_none());
        assert!(expiry("108/29", NOV_2026).is_none());
    }

    #[test]
    fn expiry_takes_the_later_of_two_dates() {
        // "Valid from" and "expires end", as UK cards print them.
        assert_eq!(expiry("01/26 01/31", NOV_2026).map(|e| e.to_string()).as_deref(), Some("01/2031"));
    }

    #[test]
    fn month_from_unix_time() {
        assert_eq!(Month::at(0), Month { year: 1970, month: 1 });
        // 2026-11-03T12:00:00Z
        assert_eq!(Month::at(1_793_707_200_000), NOV_2026);
        // 2024-02-29T00:00:00Z
        assert_eq!(Month::at(1_709_164_800_000), Month { year: 2024, month: 2 });
        // 2025-12-31T23:59:59Z
        assert_eq!(Month::at(1_767_225_599_000), Month { year: 2025, month: 12 });
    }

    #[test]
    fn lines_keep_gaps_and_drop_duplicates() {
        let mut digits = row("4242 4242", 0.1, 0.5);
        // The model's second guess at the first digit, on the same spot.
        digits.push(Digit { value: 1, confidence: 0.6, ..digits[0] });
        digits.extend(row("08 29", 0.3, 0.65));
        assert_eq!(lines(&digits), ["4242 4242", "08 29"]);
    }

    #[test]
    fn a_card_reads_number_and_expiry() {
        let mut digits = row("4242 4242 4242 4242", 0.1, 0.55);
        digits.extend(row("07 37", 0.4, 0.68));
        let card = read(&digits, NOV_2026);
        assert_eq!(card.number.as_deref().map(String::as_str), Some("4242424242424242"));
        assert_eq!(card.expiry.map(|e| e.to_string()).as_deref(), Some("07/2037"));
    }

    #[test]
    fn number_in_four_rows() {
        let mut digits = Vec::new();
        for (i, group) in ["4242", "4242", "4242", "4242"].iter().enumerate() {
            digits.extend(row(group, 0.6, 0.2 + i as f32 * 0.1));
        }
        assert_eq!(read(&digits, NOV_2026).number.as_deref().map(String::as_str), Some("4242424242424242"));
    }

    #[test]
    fn no_number_from_a_misread() {
        let digits = row("4242 4242 4242 4241", 0.1, 0.55);
        assert_eq!(read(&digits, NOV_2026), Card::default());
    }

    #[test]
    fn tally_waits_for_two_frames_to_agree() {
        let mut tally = Tally::default();
        let card = |n: &str, e: Option<&str>| Card {
            number: number(n),
            expiry: e.and_then(|e| expiry(e, NOV_2026)),
        };
        tally.add(card("4242 4242 4242 4242", Some("07/37")));
        assert!(tally.number().is_none());
        tally.add(card("5555 5555 5555 4444", None));
        assert!(tally.number().is_none());
        tally.add(card("4242 4242 4242 4242", None));
        assert_eq!(tally.number(), Some("4242424242424242"));
        assert!(tally.expiry().is_none());
        tally.add(card("", Some("07 37")));
        assert_eq!(tally.expiry().map(|e| e.to_string()).as_deref(), Some("07/2037"));
    }
}
