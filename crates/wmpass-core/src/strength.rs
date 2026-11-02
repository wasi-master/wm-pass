//! How strong a password looks to an attacker, and how two compare.

use crate::generator;

/// One password's assessment.
#[derive(Clone, Debug, PartialEq)]
pub struct Strength {
    /// 0 to 4, zxcvbn's scale.
    pub score: u8,
    /// log10 of estimated guesses.
    pub guesses_log10: f64,
    /// Readable time to crack offline against a slow hash, at 10,000 guesses a second.
    pub crack_slow_hash: String,
    /// Readable time to crack offline against a fast hash, at 10 billion a second.
    pub crack_fast_hash: String,
    /// Why it is weak, if zxcvbn has a reason.
    pub warning: Option<String>,
    /// How to improve it.
    pub suggestions: Vec<String>,
}

/// The weakest score not treated as weak by the health report.
pub const STRONG_ENOUGH: u8 = 3;

/// Assesses a password. `context` holds words the password should not lean on,
/// such as the site's name and the username.
pub fn assess(password: &str, context: &[&str]) -> Strength {
    // zxcvbn's cost grows quickly past this length and strength has long plateaued.
    let capped: String = password.chars().take(256).collect();
    let entropy = zxcvbn::zxcvbn(&capped, context);
    let times = entropy.crack_times();
    let (warning, suggestions) = entropy.feedback().map_or((None, Vec::new()), |feedback| {
        (
            feedback.warning().map(|w| w.to_string()),
            feedback.suggestions().iter().map(ToString::to_string).collect(),
        )
    });
    Strength {
        score: u8::from(entropy.score()),
        guesses_log10: entropy.guesses_log10(),
        crack_slow_hash: times.offline_slow_hashing_1e4_per_second().to_string(),
        crack_fast_hash: times.offline_fast_hashing_1e10_per_second().to_string(),
        warning,
        suggestions,
    }
}

/// Which of two passwords is stronger.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Winner {
    /// The first.
    First,
    /// The second.
    Second,
    /// Too close to call: under one order of magnitude of guesses apart.
    Even,
}

/// A side-by-side result.
#[derive(Clone, Debug, PartialEq)]
pub struct Comparison {
    /// First password's assessment.
    pub first: Strength,
    /// Second password's assessment.
    pub second: Strength,
    /// The verdict.
    pub winner: Winner,
    /// How many times more guesses the winner takes.
    pub factor: f64,
}

/// Compares two passwords.
pub fn compare(first: &str, second: &str, context: &[&str]) -> Comparison {
    let (a, b) = (assess(first, context), assess(second, context));
    let gap = a.guesses_log10 - b.guesses_log10;
    let winner = if gap.abs() < 1.0 {
        Winner::Even
    } else if gap > 0.0 {
        Winner::First
    } else {
        Winner::Second
    };
    Comparison { first: a, second: b, winner, factor: 10f64.powf(gap.abs()) }
}

/// The entropy a generator recipe guarantees, for showing beside its output. A
/// generated password's real strength is this, not what pattern matching guesses.
pub fn generated_bits(recipe: &generator::Recipe) -> f64 {
    generator::entropy_bits(recipe)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_passwords_score_low_and_long_random_ones_high() {
        assert_eq!(assess("password", &[]).score, 0);
        assert!(assess("password", &[]).warning.is_some());
        assert_eq!(assess("xK9#mP2$vL7@qR4!tN8&", &[]).score, 4);
    }

    #[test]
    fn context_words_count_against_a_password() {
        let free = assess("wasimaster2026", &[]);
        let leaning = assess("wasimaster2026", &["wasimaster"]);
        assert!(leaning.guesses_log10 < free.guesses_log10);
    }

    #[test]
    fn comparison_picks_the_stronger_one() {
        let result = compare("letmein", "correct-horse-battery-staple-9", &[]);
        assert_eq!(result.winner, Winner::Second);
        assert!(result.factor > 10.0);
        assert_eq!(compare("Tr0ub4dor&3", "Tr0ub4dor&3", &[]).winner, Winner::Even);
    }

    #[test]
    fn very_long_input_stays_fast() {
        let long = "a".repeat(100_000);
        let start = std::time::Instant::now();
        assess(&long, &[]);
        assert!(start.elapsed().as_secs() < 5);
    }
}
