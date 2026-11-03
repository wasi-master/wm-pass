//! The generator's settings, as rows a screen can step through.

use serde_json::{Value, json};
use wmpass_core::generator::{PassphraseSpec, PasswordSpec, Recipe, WordList};
use zeroize::Zeroizing;

use crate::engine::Strength;

/// What to make.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Random characters.
    Password,
    /// Random words.
    Passphrase,
    /// Syllables, easy to say.
    Pronounceable,
    /// Digits.
    Pin,
}

impl Mode {
    /// Every mode, in order.
    pub const ALL: [Self; 4] = [Self::Password, Self::Passphrase, Self::Pronounceable, Self::Pin];

    /// Its name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Password => "Password",
            Self::Passphrase => "Passphrase",
            Self::Pronounceable => "Pronounceable",
            Self::Pin => "PIN",
        }
    }
}

/// One setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Knob {
    Mode,
    Length,
    Lower,
    Upper,
    Digits,
    Symbols,
    Ambiguous,
    Words,
    Separator,
    Capitalise,
    WordDigit,
    List,
    Syllables,
}

const SEPARATORS: [&str; 6] = ["-", " ", ".", "_", ",", ""];

/// The generator.
#[derive(Debug)]
pub struct Generator {
    pub mode: Mode,
    pub password: PasswordSpec,
    pub phrase: PassphraseSpec,
    pub syllables: u32,
    pub pin: u32,
    /// Which setting has the keyboard.
    pub at: usize,
    /// The value made last.
    pub value: Zeroizing<String>,
    /// Its entropy, in bits.
    pub bits: f64,
    /// How strong it looks.
    pub strength: Option<Strength>,
}

impl Default for Generator {
    fn default() -> Self {
        Self {
            mode: Mode::Password,
            password: PasswordSpec::default(),
            phrase: PassphraseSpec::default(),
            syllables: 5,
            pin: 6,
            at: 0,
            value: Zeroizing::new(String::new()),
            bits: 0.0,
            strength: None,
        }
    }
}

impl Generator {
    /// The settings that apply to this mode.
    pub fn knobs(&self) -> Vec<Knob> {
        let mut out = vec![Knob::Mode];
        out.extend_from_slice(match self.mode {
            Mode::Password => &[Knob::Length, Knob::Lower, Knob::Upper, Knob::Digits, Knob::Symbols, Knob::Ambiguous][..],
            Mode::Passphrase => &[Knob::Words, Knob::Separator, Knob::Capitalise, Knob::WordDigit, Knob::List][..],
            Mode::Pronounceable => &[Knob::Syllables, Knob::Separator][..],
            Mode::Pin => &[Knob::Length][..],
        });
        out
    }

    /// The setting with the keyboard.
    pub fn knob(&self) -> Knob {
        let knobs = self.knobs();
        knobs[self.at.min(knobs.len() - 1)]
    }

    /// A setting's label and value, for drawing.
    pub fn describe(&self, knob: Knob) -> (&'static str, String) {
        let flag = |on: bool| if on { "on" } else { "off" }.to_owned();
        match knob {
            Knob::Mode => ("Kind", self.mode.name().to_owned()),
            Knob::Length if self.mode == Mode::Pin => ("Digits", self.pin.to_string()),
            Knob::Length => ("Length", self.password.length.to_string()),
            Knob::Lower => ("a–z", flag(self.password.lower)),
            Knob::Upper => ("A–Z", flag(self.password.upper)),
            Knob::Digits => ("0–9", flag(self.password.digits)),
            Knob::Symbols => ("!@#", flag(self.password.symbols)),
            Knob::Ambiguous => ("Avoid look-alikes", flag(self.password.exclude_ambiguous)),
            Knob::Words => ("Words", self.phrase.words.to_string()),
            Knob::Separator => {
                let sep = if self.mode == Mode::Pronounceable { self.pron_separator() } else { self.phrase.separator.clone() };
                ("Separator", match sep.as_str() {
                    " " => "space".to_owned(),
                    "" => "none".to_owned(),
                    other => other.to_owned(),
                })
            }
            Knob::Capitalise => ("Capitals", flag(self.phrase.capitalize)),
            Knob::WordDigit => ("Add a digit", flag(self.phrase.include_digit)),
            Knob::List => ("Word list", match self.phrase.list {
                WordList::EffLong => "EFF long".to_owned(),
                WordList::EffShort => "EFF short".to_owned(),
            }),
            Knob::Syllables => ("Syllables", self.syllables.to_string()),
        }
    }

    fn pron_separator(&self) -> String {
        self.phrase.separator.clone()
    }

    /// Steps the setting with the keyboard by `by`, or flips it.
    pub fn adjust(&mut self, by: i32) {
        let step = |value: u32, min: u32, max: u32| -> u32 {
            let next = i64::from(value) + i64::from(by);
            u32::try_from(next.clamp(i64::from(min), i64::from(max))).unwrap_or(min)
        };
        match self.knob() {
            Knob::Mode => {
                let at = Mode::ALL.iter().position(|m| *m == self.mode).unwrap_or(0);
                let len = Mode::ALL.len();
                let next = if by < 0 { (at + len - 1) % len } else { (at + 1) % len };
                self.mode = Mode::ALL[next];
                self.at = 0;
            }
            Knob::Length if self.mode == Mode::Pin => self.pin = step(self.pin, 4, 20),
            Knob::Length => self.password.length = step(self.password.length, 8, 128),
            Knob::Lower => self.password.lower = !self.password.lower,
            Knob::Upper => self.password.upper = !self.password.upper,
            Knob::Digits => self.password.digits = !self.password.digits,
            Knob::Symbols => self.password.symbols = !self.password.symbols,
            Knob::Ambiguous => self.password.exclude_ambiguous = !self.password.exclude_ambiguous,
            Knob::Words => self.phrase.words = step(self.phrase.words, 3, 20),
            Knob::Separator => {
                let at = SEPARATORS.iter().position(|s| *s == self.phrase.separator).unwrap_or(0);
                let len = SEPARATORS.len();
                let next = if by < 0 { (at + len - 1) % len } else { (at + 1) % len };
                self.phrase.separator = SEPARATORS[next].to_owned();
            }
            Knob::Capitalise => self.phrase.capitalize = !self.phrase.capitalize,
            Knob::WordDigit => self.phrase.include_digit = !self.phrase.include_digit,
            Knob::List => {
                self.phrase.list = match self.phrase.list {
                    WordList::EffLong => WordList::EffShort,
                    WordList::EffShort => WordList::EffLong,
                };
            }
            Knob::Syllables => self.syllables = step(self.syllables, 2, 16),
        }
        // At least one kind of character, or there is nothing to draw from.
        let p = &mut self.password;
        if !(p.lower || p.upper || p.digits || p.symbols) {
            p.lower = true;
        }
    }

    /// The recipe, as the session reads it.
    pub fn recipe(&self) -> Value {
        let recipe = match self.mode {
            Mode::Password => Recipe::Password(self.password.clone()),
            Mode::Passphrase => Recipe::Passphrase(self.phrase.clone()),
            Mode::Pronounceable => Recipe::Pronounceable {
                syllables: self.syllables,
                separator: self.phrase.separator.clone(),
            },
            Mode::Pin => Recipe::Pin { length: self.pin },
        };
        serde_json::to_value(recipe).unwrap_or_else(|_| json!({"Password": {}}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mode_makes_a_recipe_the_core_reads() {
        let mut generator = Generator::default();
        for _ in 0..Mode::ALL.len() {
            let recipe: Recipe = serde_json::from_value(generator.recipe()).expect("reads");
            assert!(wmpass_core::generator::generate(&recipe).is_ok(), "{:?}", generator.mode);
            generator.at = 0;
            generator.adjust(1);
        }
        assert_eq!(generator.mode, Mode::Password);
    }

    #[test]
    fn turning_every_class_off_keeps_one() {
        let mut generator = Generator::default();
        for knob in [Knob::Lower, Knob::Upper, Knob::Digits, Knob::Symbols] {
            generator.at = generator.knobs().iter().position(|k| *k == knob).expect("has it");
            generator.adjust(1);
        }
        assert!(generator.password.lower);
    }

    #[test]
    fn lengths_stay_in_range() {
        let mut generator = Generator { at: 1, ..Generator::default() };
        for _ in 0..500 {
            generator.adjust(1);
        }
        assert_eq!(generator.password.length, 128);
        for _ in 0..500 {
            generator.adjust(-1);
        }
        assert_eq!(generator.password.length, 8);
    }
}
