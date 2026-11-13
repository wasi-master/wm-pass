//! Generating passwords and passphrases. All randomness is from the OS, and every
//! choice is uniform.

use diceware_wordlists::Wordlist;
use serde::{Deserialize, Serialize};

use crate::crypto::random_below;
use crate::{Error, Result, Secret};

const LOWER: &str = "abcdefghijklmnopqrstuvwxyz";
const UPPER: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const DIGITS: &str = "0123456789";
const SYMBOLS: &str = "!@#$%^&*()-_=+[]{};:,.?/~";
const AMBIGUOUS: &str = "Il1O0o|`'\"";
const CONSONANTS: &str = "bcdfghjklmnprstvwz";
const VOWELS: &str = "aeiou";

/// Random characters.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PasswordSpec {
    /// Length.
    pub length: u32,
    /// Lower-case letters.
    pub lower: bool,
    /// Capitals.
    pub upper: bool,
    /// Digits.
    pub digits: bool,
    /// Symbols.
    pub symbols: bool,
    /// Replaces the default symbol set.
    pub symbol_set: Option<String>,
    /// Leaves out look-alike characters.
    pub exclude_ambiguous: bool,
    /// Characters never to use.
    pub exclude: String,
    /// Longest allowed run of one character. Zero means no limit.
    pub max_consecutive: u32,
}

impl Default for PasswordSpec {
    fn default() -> Self {
        Self {
            length: 20,
            lower: true,
            upper: true,
            digits: true,
            symbols: true,
            symbol_set: None,
            exclude_ambiguous: false,
            exclude: String::new(),
            max_consecutive: 0,
        }
    }
}

/// Random words.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PassphraseSpec {
    /// Number of words.
    pub words: u32,
    /// Between words.
    pub separator: String,
    /// Capitalises each word.
    pub capitalize: bool,
    /// Adds a digit to one random word.
    pub include_digit: bool,
    /// Which list.
    pub list: WordList,
}

impl Default for PassphraseSpec {
    fn default() -> Self {
        Self {
            words: 6,
            separator: "-".into(),
            capitalize: false,
            include_digit: false,
            list: WordList::EffLong,
        }
    }
}

/// Bundled word lists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WordList {
    /// EFF long list: 7,776 words.
    #[default]
    EffLong,
    /// EFF short list: 1,296 short words.
    EffShort,
}

impl WordList {
    fn words(self) -> &'static [&'static str] {
        match self {
            Self::EffLong => Wordlist::EffLong.get_list(),
            Self::EffShort => Wordlist::EffShort1.get_list(),
        }
    }
}

/// What to generate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Recipe {
    /// Random characters.
    Password(PasswordSpec),
    /// Random words.
    Passphrase(PassphraseSpec),
    /// Alternating consonants and vowels, easier to say and type.
    Pronounceable {
        /// Syllables.
        syllables: u32,
        /// Between syllable groups.
        separator: String,
    },
    /// Digits only.
    Pin {
        /// Digits.
        length: u32,
    },
    /// `a` lower, `A` upper, `c` and `C` a consonant, `v` and `V` a vowel, `9` digit,
    /// `#` symbol, `?` any, `\` literal next.
    Pattern(String),
    /// A phone's unlock pattern on the 3×3 grid of dots, numbered 1 to 9 left to right, top
    /// to bottom, and written as the dots in the order drawn: `2-6-7-1`.
    LockPattern {
        /// Dots joined, 4 to 9.
        dots: u32,
    },
}

impl Default for Recipe {
    fn default() -> Self {
        Self::Password(PasswordSpec::default())
    }
}

/// A saved recipe.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preset {
    /// Label.
    pub name: String,
    /// Settings.
    pub recipe: Recipe,
}

/// Presets every vault starts with.
pub fn default_presets() -> Vec<Preset> {
    let symbols_off = PasswordSpec {
        symbols: false,
        ..PasswordSpec::default()
    };
    vec![
        Preset {
            name: "Strong".into(),
            recipe: Recipe::default(),
        },
        Preset {
            name: "No symbols".into(),
            recipe: Recipe::Password(symbols_off),
        },
        // Four words are what people remember; six are what they write down.
        Preset {
            name: "Memorable".into(),
            recipe: Recipe::Passphrase(PassphraseSpec {
                words: 4,
                ..PassphraseSpec::default()
            }),
        },
        Preset {
            name: "PIN".into(),
            recipe: Recipe::Pin { length: 6 },
        },
    ]
}

fn pick(pool: &[char]) -> Result<char> {
    let bound = u32::try_from(pool.len()).map_err(|_| Error::TooLong)?;
    Ok(pool[random_below(bound)? as usize])
}

fn shuffle<T>(items: &mut [T]) -> Result<()> {
    for at in (1..items.len()).rev() {
        let bound = u32::try_from(at + 1).map_err(|_| Error::TooLong)?;
        items.swap(at, random_below(bound)? as usize);
    }
    Ok(())
}

/// Each character once: a symbol typed twice in a custom set must not come up twice as often.
fn pool(chars: &str, spec: &PasswordSpec) -> Vec<char> {
    let mut out: Vec<char> = Vec::new();
    for c in chars
        .chars()
        .filter(|c| !spec.exclude.contains(*c))
        .filter(|c| !spec.exclude_ambiguous || !AMBIGUOUS.contains(*c))
    {
        if !out.contains(&c) {
            out.push(c);
        }
    }
    out
}

/// Every character the classes allow, each once, though a custom symbol set may repeat a letter.
fn every_char(classes: &[Vec<char>]) -> Vec<char> {
    let mut all: Vec<char> = classes.iter().flatten().copied().collect();
    all.sort_unstable();
    all.dedup();
    all
}

fn classes(spec: &PasswordSpec) -> Vec<Vec<char>> {
    let symbols = spec.symbol_set.as_deref().unwrap_or(SYMBOLS);
    let mut out = Vec::new();
    if spec.lower {
        out.push(pool(LOWER, spec));
    }
    if spec.upper {
        out.push(pool(UPPER, spec));
    }
    if spec.digits {
        out.push(pool(DIGITS, spec));
    }
    if spec.symbols {
        out.push(pool(symbols, spec));
    }
    out.retain(|class| !class.is_empty());
    out
}

#[cfg(test)]
fn too_many_repeats(chars: &[char], limit: u32) -> bool {
    limit > 0
        && chars
            .windows(limit as usize + 1)
            .any(|run| run.iter().all(|c| *c == run[0]))
}

/// Random characters with at least one from each chosen class.
pub fn password(spec: &PasswordSpec) -> Result<Secret> {
    let classes = classes(spec);
    if classes.is_empty() {
        return Err(Error::Invalid(
            "choose at least one kind of character".into(),
        ));
    }
    let length = spec.length as usize;
    if !(4..=1024).contains(&length) {
        return Err(Error::Invalid("choose a length between 4 and 1024".into()));
    }
    if length < classes.len() {
        return Err(Error::Invalid(
            "that length is too short for every kind of character chosen".into(),
        ));
    }
    let all = every_char(&classes);
    let pools: Vec<&[char]> = classes.iter().map(Vec::as_slice).collect();
    drawn(&pools, &all, length, spec.max_consecutive)
        .ok_or_else(|| Error::Invalid("those settings leave too few possibilities".into()))?
}

/// One character from each of `required`, the rest from `every`, in random order, with no
/// run of one character longer than `limit` (zero for no limit). A character that would make
/// such a run is drawn again from the set it came from, so every required kind stays. `None`
/// when a run cannot be broken, as with a single character to draw from.
fn drawn(required: &[&[char]], every: &[char], length: usize, limit: u32) -> Option<Result<Secret>> {
    let mut chars: Vec<(char, &[char])> = Vec::with_capacity(length);
    for pool in required {
        match pick(pool) {
            Ok(c) => chars.push((c, pool)),
            Err(e) => return Some(Err(e)),
        }
    }
    while chars.len() < length {
        match pick(every) {
            Ok(c) => chars.push((c, every)),
            Err(e) => return Some(Err(e)),
        }
    }
    if let Err(e) = shuffle(&mut chars) {
        return Some(Err(e));
    }
    let limit = limit as usize;
    if limit > 0 {
        for at in limit..chars.len() {
            let current = chars[at].0;
            if chars[at - limit..at].iter().all(|(c, _)| *c == current) {
                let others: Vec<char> = chars[at].1.iter().copied().filter(|c| *c != current).collect();
                if others.is_empty() {
                    return None;
                }
                match pick(&others) {
                    Ok(c) => chars[at].0 = c,
                    Err(e) => return Some(Err(e)),
                }
            }
        }
    }
    Some(Ok(Secret::from(chars.into_iter().map(|(c, _)| c).collect::<String>())))
}

/// Random words.
pub fn passphrase(spec: &PassphraseSpec) -> Result<Secret> {
    if !(2..=40).contains(&spec.words) {
        return Err(Error::Invalid("choose between 2 and 40 words".into()));
    }
    let list = spec.list.words();
    let bound = u32::try_from(list.len()).map_err(|_| Error::TooLong)?;
    let mut words = Vec::with_capacity(spec.words as usize);
    for _ in 0..spec.words {
        let word = list[random_below(bound)? as usize];
        words.push(if spec.capitalize {
            capitalise(word)
        } else {
            word.to_owned()
        });
    }
    if spec.include_digit {
        let at = random_below(spec.words)? as usize;
        words[at].push(char::from(b'0' + random_below(10)? as u8));
    }
    Ok(Secret::from(words.join(&spec.separator)))
}

fn capitalise(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

fn pronounceable(syllables: u32, separator: &str) -> Result<Secret> {
    if !(2..=64).contains(&syllables) {
        return Err(Error::Invalid("choose between 2 and 64 syllables".into()));
    }
    let consonants: Vec<char> = CONSONANTS.chars().collect();
    let vowels: Vec<char> = VOWELS.chars().collect();
    let mut out = String::new();
    for index in 0..syllables {
        if index > 0 && index % 3 == 0 {
            out.push_str(separator);
        }
        out.push(pick(&consonants)?);
        out.push(pick(&vowels)?);
    }
    Ok(Secret::from(out))
}

fn pin(length: u32) -> Result<Secret> {
    if !(4..=64).contains(&length) {
        return Err(Error::Invalid("choose between 4 and 64 digits".into()));
    }
    let mut out = String::with_capacity(length as usize);
    for _ in 0..length {
        out.push(char::from(b'0' + random_below(10)? as u8));
    }
    Ok(Secret::from(out))
}

fn pattern(template: &str) -> Result<Secret> {
    // An empty pattern would hand back an empty password to save.
    if template.is_empty() {
        return Err(Error::Invalid("write a pattern, such as Cvcv-9999".into()));
    }
    let lower: Vec<char> = LOWER.chars().collect();
    let upper: Vec<char> = UPPER.chars().collect();
    let digits: Vec<char> = DIGITS.chars().collect();
    let symbols: Vec<char> = SYMBOLS.chars().collect();
    let any: Vec<char> = LOWER
        .chars()
        .chain(UPPER.chars())
        .chain(DIGITS.chars())
        .chain(SYMBOLS.chars())
        .collect();
    let consonants: Vec<char> = CONSONANTS.chars().collect();
    let vowels: Vec<char> = VOWELS.chars().collect();
    let capital = |c: char| c.to_ascii_uppercase();
    let mut out = String::new();
    let mut chars = template.chars();
    while let Some(ch) = chars.next() {
        match ch {
            'a' => out.push(pick(&lower)?),
            'A' => out.push(pick(&upper)?),
            'c' => out.push(pick(&consonants)?),
            'C' => out.push(capital(pick(&consonants)?)),
            'v' => out.push(pick(&vowels)?),
            'V' => out.push(capital(pick(&vowels)?)),
            '9' => out.push(pick(&digits)?),
            '#' => out.push(pick(&symbols)?),
            '?' => out.push(pick(&any)?),
            '\\' => out.extend(chars.next()),
            other => out.push(other),
        }
    }
    // A lone `\` escapes nothing and writes nothing.
    if out.is_empty() {
        return Err(Error::Invalid("write a pattern, such as Cvcv-9999".into()));
    }
    Ok(Secret::from(out))
}

/// The dot a straight line from `a` to `b` passes over, if any. Dots are 0 to 8, left to
/// right, top to bottom.
fn between(a: usize, b: usize) -> Option<usize> {
    let (rows, cols) = (a / 3 + b / 3, a % 3 + b % 3);
    (rows % 2 == 0 && cols % 2 == 0 && a != b).then_some(rows / 2 * 3 + cols / 2)
}

/// Whether a pattern that has drawn `used`, ending on `from`, may go on to `to`. As on a phone,
/// a line cannot pass over a dot that is not yet part of the pattern, since the phone would
/// join that dot on the way.
fn can_step(used: u16, from: usize, to: usize) -> bool {
    used & (1 << to) == 0 && between(from, to).is_none_or(|middle| used & (1 << middle) != 0)
}

/// How many patterns finish from here with `left` more dots. Built once, by fewest dots left
/// first, as a table of every set of dots drawn, the dot it ends on, and how many are to come.
fn completions(used: u16, last: usize, left: u32) -> u64 {
    static TABLE: std::sync::OnceLock<Vec<u64>> = std::sync::OnceLock::new();
    let at = |used: usize, last: usize, left: usize| (left * 512 + used) * 9 + last;
    let table = TABLE.get_or_init(|| {
        let mut table = vec![0u64; 9 * 512 * 9];
        for used in 0..512 {
            for last in 0..9 {
                table[at(used, last, 0)] = 1;
            }
        }
        for left in 1..9 {
            for used in 0..512usize {
                for last in 0..9 {
                    table[at(used, last, left)] = (0..9)
                        .filter(|to| can_step(used as u16, last, *to))
                        .map(|to| table[at(used | (1 << to), to, left - 1)])
                        .sum();
                }
            }
        }
        table
    });
    if left > 8 {
        return 0;
    }
    table[at(usize::from(used), last, left as usize)]
}

/// Every pattern of `dots` dots a phone accepts: 1,624 of four up to 140,704 of nine.
pub fn lock_patterns(dots: u32) -> u64 {
    if !(1..=9).contains(&dots) {
        return 0;
    }
    (0..9)
        .map(|start| completions(1 << start, start, dots - 1))
        .sum()
}

/// One of `weights`, each as likely as its weight.
fn pick_weighted(weights: &[(usize, u64)]) -> Result<usize> {
    let total: u64 = weights.iter().map(|(_, w)| w).sum();
    let bound = u32::try_from(total).map_err(|_| Error::TooLong)?;
    let mut roll = u64::from(random_below(bound)?);
    for (item, weight) in weights {
        if roll < *weight {
            return Ok(*item);
        }
        roll -= weight;
    }
    Err(Error::Invalid("no pattern fits".into()))
}

/// A lock pattern drawn uniformly from every one a phone accepts. Each dot is chosen in
/// proportion to the patterns that can still follow it, which is what makes the whole uniform:
/// picking each next dot evenly would favour the short dead ends near the corners.
fn lock_pattern(dots: u32) -> Result<Secret> {
    if !(4..=9).contains(&dots) {
        return Err(Error::Invalid("choose between 4 and 9 dots".into()));
    }
    let starts: Vec<(usize, u64)> = (0..9)
        .map(|start| (start, completions(1 << start, start, dots - 1)))
        .collect();
    let mut last = pick_weighted(&starts)?;
    let mut used: u16 = 1 << last;
    let mut path = vec![last];
    for left in (0..dots - 1).rev() {
        let next: Vec<(usize, u64)> = (0..9)
            .filter(|to| can_step(used, last, *to))
            .map(|to| (to, completions(used | (1 << to), to, left)))
            .filter(|(_, ways)| *ways > 0)
            .collect();
        last = pick_weighted(&next)?;
        used |= 1 << last;
        path.push(last);
    }
    let text: Vec<String> = path.iter().map(|dot| (dot + 1).to_string()).collect();
    Ok(Secret::from(text.join("-")))
}

/// Runs a recipe.
pub fn generate(recipe: &Recipe) -> Result<Secret> {
    match recipe {
        Recipe::Password(spec) => password(spec),
        Recipe::Passphrase(spec) => passphrase(spec),
        Recipe::Pronounceable {
            syllables,
            separator,
        } => pronounceable(*syllables, separator),
        Recipe::Pin { length } => pin(*length),
        Recipe::Pattern(template) => pattern(template),
        Recipe::LockPattern { dots } => lock_pattern(*dots),
    }
}

/// Entropy of what a recipe produces, in bits. Uses the search space, not the
/// result, so it is the honest number.
pub fn entropy_bits(recipe: &Recipe) -> f64 {
    match recipe {
        Recipe::Password(spec) => {
            let size = every_char(&classes(spec)).len();
            f64::from(spec.length) * (size.max(1) as f64).log2()
        }
        Recipe::Passphrase(spec) => {
            let mut bits = f64::from(spec.words) * (spec.list.words().len() as f64).log2();
            if spec.include_digit {
                bits += (f64::from(spec.words) * 10.0).log2();
            }
            bits
        }
        Recipe::Pronounceable { syllables, .. } => {
            f64::from(*syllables) * ((CONSONANTS.len() * VOWELS.len()) as f64).log2()
        }
        Recipe::Pin { length } => f64::from(*length) * 10f64.log2(),
        Recipe::Pattern(template) => {
            let mut bits = 0.0;
            let mut chars = template.chars();
            while let Some(ch) = chars.next() {
                bits += match ch {
                    'a' | 'A' => 26f64.log2(),
                    'c' | 'C' => (CONSONANTS.len() as f64).log2(),
                    'v' | 'V' => (VOWELS.len() as f64).log2(),
                    '9' => 10f64.log2(),
                    '#' => (SYMBOLS.len() as f64).log2(),
                    '?' => ((26 + 26 + 10 + SYMBOLS.len()) as f64).log2(),
                    '\\' => {
                        chars.next();
                        0.0
                    }
                    _ => 0.0,
                };
            }
            bits
        }
        Recipe::LockPattern { dots } => (lock_patterns(*dots).max(1) as f64).log2(),
    }
}

/// A site's password rules, in the syntax of Apple's password-rules files.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SiteRules {
    /// Fewest characters.
    pub min_length: Option<u32>,
    /// Most characters.
    pub max_length: Option<u32>,
    /// Character sets of which one member is required.
    pub required: Vec<String>,
    /// Further characters allowed.
    pub allowed: Vec<String>,
    /// Longest run of one character.
    pub max_consecutive: Option<u32>,
}

fn class_chars(token: &str) -> Option<String> {
    let token = token.trim();
    if let Some(inner) = token.strip_prefix('[').and_then(|t| t.strip_suffix(']')) {
        return Some(inner.to_owned());
    }
    Some(
        match token {
            "lower" => LOWER,
            "upper" => UPPER,
            "digit" => DIGITS,
            "special" => "-().&@?'#,/\";+",
            "ascii-printable" => {
                return Some((0x20u8..0x7f).map(char::from).collect());
            }
            "unicode" => return Some((0x20u8..0x7f).map(char::from).collect()),
            _ => return None,
        }
        .to_owned(),
    )
}

/// Parses `minlength: 8; required: lower, upper; allowed: [-];`.
pub fn parse_site_rules(text: &str) -> SiteRules {
    let mut rules = SiteRules::default();
    for clause in text.split(';') {
        let Some((key, value)) = clause.split_once(':') else {
            continue;
        };
        match key.trim() {
            "minlength" => rules.min_length = value.trim().parse().ok(),
            "maxlength" => rules.max_length = value.trim().parse().ok(),
            "max-consecutive" => rules.max_consecutive = value.trim().parse().ok(),
            "required" => {
                let joined: String = split_classes(value)
                    .iter()
                    .filter_map(|t| class_chars(t))
                    .collect();
                if !joined.is_empty() {
                    rules.required.push(joined);
                }
            }
            "allowed" => {
                rules
                    .allowed
                    .extend(split_classes(value).iter().filter_map(|t| class_chars(t)));
            }
            _ => {}
        }
    }
    rules
}

fn split_classes(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let (mut current, mut in_set) = (String::new(), false);
    for ch in value.chars() {
        match ch {
            '[' => {
                in_set = true;
                current.push(ch);
            }
            ']' => {
                in_set = false;
                current.push(ch);
            }
            ',' if !in_set => out.push(std::mem::take(&mut current)),
            other => current.push(other),
        }
    }
    if !current.trim().is_empty() {
        out.push(current);
    }
    out
}

static BUNDLED_RULES: &str = include_str!("../../../data/password-rules.json");

/// The bundled rules for a registrable domain or host, if any.
pub fn rules_for_site(domain: &str) -> Option<SiteRules> {
    let all: serde_json::Value = serde_json::from_str(BUNDLED_RULES).ok()?;
    let text = all.get(domain)?.get("password-rules")?.as_str()?;
    Some(parse_site_rules(text))
}

/// A password that satisfies a site's rules, keeping the length as long as they let it.
pub fn password_for_rules(rules: &SiteRules, wanted_length: u32) -> Result<Secret> {
    let mut length = wanted_length;
    if let Some(max) = rules.max_length {
        length = length.min(max);
    }
    if let Some(min) = rules.min_length {
        length = length.max(min);
    }
    let mut pools: Vec<Vec<char>> = rules
        .required
        .iter()
        .map(|set| set.chars().collect())
        .collect();
    if rules.required.is_empty() && rules.allowed.is_empty() {
        return password(&PasswordSpec {
            length: length.max(4),
            max_consecutive: rules.max_consecutive.unwrap_or(0),
            ..PasswordSpec::default()
        });
    }
    let mut every: Vec<char> = pools.iter().flatten().copied().collect();
    every.extend(rules.allowed.iter().flat_map(|set| set.chars()));
    every.sort_unstable();
    every.dedup();
    pools.retain(|pool| !pool.is_empty());
    let target = length as usize;
    if every.is_empty() || target < pools.len() {
        return Err(Error::Invalid("those site rules cannot be met".into()));
    }
    let required: Vec<&[char]> = pools.iter().map(Vec::as_slice).collect();
    drawn(&required, &every, target, rules.max_consecutive.unwrap_or(0))
        .ok_or_else(|| Error::Invalid("those site rules cannot be met".into()))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_has_every_class_and_the_right_length() {
        for _ in 0..200 {
            let out = password(&PasswordSpec {
                length: 8,
                ..PasswordSpec::default()
            })
            .unwrap();
            let text = out.expose();
            assert_eq!(text.chars().count(), 8);
            assert!(text.chars().any(|c| c.is_ascii_lowercase()));
            assert!(text.chars().any(|c| c.is_ascii_uppercase()));
            assert!(text.chars().any(|c| c.is_ascii_digit()));
            assert!(text.chars().any(|c| SYMBOLS.contains(c)));
        }
    }

    #[test]
    fn a_repeated_character_in_a_custom_set_is_no_likelier() {
        let spec = PasswordSpec {
            lower: false,
            upper: false,
            digits: false,
            symbol_set: Some("!!!!!!!!!@a".into()),
            ..PasswordSpec::default()
        };
        assert_eq!(classes(&spec), vec![vec!['!', '@', 'a']]);
        let overlapping = PasswordSpec { symbol_set: Some("ab".into()), ..PasswordSpec::default() };
        assert_eq!(every_char(&classes(&overlapping)).len(), 26 + 26 + 10);
        assert!((entropy_bits(&Recipe::Password(spec)) - 20.0 * 3f64.log2()).abs() < 1e-9);
    }

    #[test]
    fn exclusions_hold() {
        let spec = PasswordSpec {
            exclude_ambiguous: true,
            exclude: "abc".into(),
            ..PasswordSpec::default()
        };
        for _ in 0..100 {
            let out = password(&spec).unwrap();
            assert!(
                !out.expose()
                    .chars()
                    .any(|c| AMBIGUOUS.contains(c) || "abc".contains(c))
            );
        }
    }

    #[test]
    fn impossible_settings_are_refused() {
        let none = PasswordSpec {
            lower: false,
            upper: false,
            digits: false,
            symbols: false,
            ..PasswordSpec::default()
        };
        assert!(password(&none).is_err());
        assert!(
            password(&PasswordSpec {
                length: 2,
                ..PasswordSpec::default()
            })
            .is_err()
        );
        let one_char = PasswordSpec {
            lower: false,
            upper: false,
            symbols: false,
            exclude: "012345678".into(),
            max_consecutive: 1,
            length: 8,
            ..PasswordSpec::default()
        };
        assert!(password(&one_char).is_err());
    }

    #[test]
    fn two_runs_differ_and_distribution_is_flat() {
        let spec = PasswordSpec {
            length: 64,
            ..PasswordSpec::default()
        };
        assert_ne!(password(&spec).unwrap(), password(&spec).unwrap());
        // Each digit of a long PIN should appear roughly equally.
        let mut counts = [0u32; 10];
        for _ in 0..500 {
            for b in pin(64).unwrap().expose().bytes() {
                counts[usize::from(b - b'0')] += 1;
            }
        }
        let expected = 3200.0;
        for count in counts {
            assert!(
                (f64::from(count) - expected).abs() < expected * 0.12,
                "{counts:?}"
            );
        }
    }

    #[test]
    fn a_run_limit_is_met_however_long_the_password() {
        // Ten digits and no two alike in a row: drawing again until one came out whole failed
        // nearly always at this length.
        let spec = PasswordSpec {
            lower: false,
            upper: false,
            symbols: false,
            max_consecutive: 1,
            length: 1024,
            ..PasswordSpec::default()
        };
        for _ in 0..20 {
            let out = password(&spec).unwrap();
            let chars: Vec<char> = out.expose().chars().collect();
            assert_eq!(chars.len(), 1024);
            assert!(!too_many_repeats(&chars, 1));
        }
        let classes = PasswordSpec { length: 4, max_consecutive: 1, ..PasswordSpec::default() };
        for _ in 0..200 {
            let out = password(&classes).unwrap();
            let text = out.expose();
            assert!(text.chars().any(|c| c.is_ascii_lowercase()) && text.chars().any(|c| c.is_ascii_uppercase()));
            assert!(text.chars().any(|c| c.is_ascii_digit()) && text.chars().any(|c| SYMBOLS.contains(c)));
        }
    }

    #[test]
    fn passphrase_comes_from_the_list() {
        let out = passphrase(&PassphraseSpec::default()).unwrap();
        let words: Vec<_> = out.expose().split('-').collect();
        assert_eq!(words.len(), 6);
        let list = WordList::EffLong.words();
        assert_eq!(list.len(), 7776);
        assert!(words.iter().all(|word| list.contains(word)));
    }

    #[test]
    fn passphrase_options() {
        let spec = PassphraseSpec {
            words: 4,
            separator: " ".into(),
            capitalize: true,
            include_digit: true,
            ..PassphraseSpec::default()
        };
        let out = passphrase(&spec).unwrap();
        let text = out.expose();
        assert!(
            text.split(' ')
                .all(|w| w.chars().next().is_some_and(char::is_uppercase))
        );
        assert_eq!(text.chars().filter(char::is_ascii_digit).count(), 1);
    }

    #[test]
    fn entropy_numbers() {
        let six = Recipe::Passphrase(PassphraseSpec::default());
        assert!((entropy_bits(&six) - 77.5).abs() < 0.1);
        let pin6 = Recipe::Pin { length: 6 };
        assert!((entropy_bits(&pin6) - 19.9).abs() < 0.1);
        let digits_only = Recipe::Password(PasswordSpec {
            lower: false,
            upper: false,
            symbols: false,
            length: 10,
            ..PasswordSpec::default()
        });
        assert!((entropy_bits(&digits_only) - 33.2).abs() < 0.1);
        assert!((entropy_bits(&Recipe::Pattern("99\\a".into())) - 6.64).abs() < 0.01);
    }

    #[test]
    fn lock_patterns_are_counted_as_phones_count_them() {
        let counts: Vec<u64> = (4..=9).map(lock_patterns).collect();
        assert_eq!(counts, vec![1624, 7152, 26016, 72912, 140_704, 140_704]);
        assert!((entropy_bits(&Recipe::LockPattern { dots: 4 }) - 10.67).abs() < 0.01);
        assert_eq!(between(0, 8), Some(4));
        assert_eq!(between(0, 2), Some(1));
        assert_eq!(between(0, 5), None);
        assert_eq!(between(1, 7), Some(4));
        assert_eq!(between(3, 4), None);
    }

    #[test]
    fn a_lock_pattern_never_jumps_a_dot_it_has_not_drawn() {
        for dots in 4..=9 {
            for _ in 0..200 {
                let out = generate(&Recipe::LockPattern { dots }).unwrap();
                let path: Vec<usize> = out
                    .expose()
                    .split('-')
                    .map(|d| d.parse::<usize>().unwrap() - 1)
                    .collect();
                assert_eq!(path.len(), dots as usize);
                let mut used = 1u16 << path[0];
                for pair in path.windows(2) {
                    assert!(can_step(used, pair[0], pair[1]), "{path:?}");
                    used |= 1 << pair[1];
                }
            }
        }
        assert!(generate(&Recipe::LockPattern { dots: 3 }).is_err());
        assert!(generate(&Recipe::LockPattern { dots: 10 }).is_err());
    }

    #[test]
    fn lock_patterns_start_anywhere_in_proportion() {
        // More patterns start from the middle dot than from a corner, since every dot is next to
        // it. A generator that picked the first dot evenly would start there a ninth of the time.
        let mut middle = 0u32;
        let runs = 9000;
        for _ in 0..runs {
            if generate(&Recipe::LockPattern { dots: 4 })
                .unwrap()
                .expose()
                .starts_with('5')
            {
                middle += 1;
            }
        }
        let expected = f64::from(runs) * completions(1 << 4, 4, 3) as f64 / 1624.0;
        assert!(
            (f64::from(middle) - expected).abs() < expected * 0.15,
            "{middle} vs {expected}"
        );
    }

    #[test]
    fn memorable_is_four_words() {
        let memorable = default_presets()
            .into_iter()
            .find(|p| p.name == "Memorable")
            .unwrap();
        assert!(matches!(
            memorable.recipe,
            Recipe::Passphrase(PassphraseSpec { words: 4, .. })
        ));
    }

    #[test]
    fn pattern_knows_consonants_and_vowels() {
        for _ in 0..50 {
            let out = generate(&Recipe::Pattern("Cvcv-9999".into())).unwrap();
            let chars: Vec<char> = out.expose().chars().collect();
            assert_eq!(chars.len(), 9);
            assert!(
                chars[0].is_ascii_uppercase() && CONSONANTS.contains(chars[0].to_ascii_lowercase())
            );
            assert!(
                VOWELS.contains(chars[1])
                    && CONSONANTS.contains(chars[2])
                    && VOWELS.contains(chars[3])
            );
            assert_eq!(chars[4], '-');
            assert!(chars[5..].iter().all(char::is_ascii_digit));
        }
        let bits = entropy_bits(&Recipe::Pattern("Cvcv-9999".into()));
        assert!(
            (bits - (2.0 * 18f64.log2() + 2.0 * 5f64.log2() + 4.0 * 10f64.log2())).abs() < 0.01
        );
    }

    #[test]
    fn an_empty_pattern_is_refused() {
        assert!(generate(&Recipe::Pattern(String::new())).is_err());
        assert!(generate(&Recipe::Pattern("\\".into())).is_err());
    }

    #[test]
    fn pattern_follows_its_template() {
        let out = generate(&Recipe::Pattern("Aaa-999\\9".into())).unwrap();
        let text = out.expose();
        let bytes = text.as_bytes();
        assert!(bytes[0].is_ascii_uppercase() && bytes[1].is_ascii_lowercase());
        assert_eq!(bytes[3], b'-');
        assert!(bytes[4].is_ascii_digit());
        assert_eq!(bytes[7], b'9');
    }

    #[test]
    fn pronounceable_and_presets_run() {
        let out = generate(&Recipe::Pronounceable {
            syllables: 6,
            separator: "-".into(),
        })
        .unwrap();
        assert_eq!(out.expose().len(), 13);
        for preset in default_presets() {
            assert!(generate(&preset.recipe).is_ok(), "{}", preset.name);
        }
    }

    #[test]
    fn site_rules_parse() {
        let rules = parse_site_rules(
            "minlength: 8; maxlength: 12; required: lower, upper; required: digit; allowed: [-_]; max-consecutive: 2;",
        );
        assert_eq!(rules.min_length, Some(8));
        assert_eq!(rules.max_length, Some(12));
        assert_eq!(rules.required.len(), 2);
        assert_eq!(rules.allowed, vec!["-_".to_owned()]);
        assert_eq!(rules.max_consecutive, Some(2));
    }

    #[test]
    fn generated_password_satisfies_site_rules() {
        let rules = parse_site_rules(
            "minlength: 8; maxlength: 10; required: lower; required: digit; allowed: [-]; max-consecutive: 2;",
        );
        for _ in 0..100 {
            let out = password_for_rules(&rules, 20).unwrap();
            let text = out.expose();
            assert!(text.len() == 10, "{}", text.len());
            assert!(text.chars().any(|c| c.is_ascii_lowercase()));
            assert!(text.chars().any(|c| c.is_ascii_digit()));
            assert!(
                text.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            );
            let chars: Vec<char> = text.chars().collect();
            assert!(!too_many_repeats(&chars, 2));
        }
    }

    #[test]
    fn bundled_rules_load_and_every_entry_can_be_met() {
        let all: serde_json::Value = serde_json::from_str(BUNDLED_RULES).unwrap();
        let map = all.as_object().unwrap();
        assert!(map.len() > 300);
        assert!(rules_for_site("163.com").is_some());
        assert!(rules_for_site("not-a-listed-site.example").is_none());
        for (site, value) in map {
            let text = value["password-rules"].as_str().unwrap();
            let rules = parse_site_rules(text);
            assert!(password_for_rules(&rules, 20).is_ok(), "{site}: {text}");
        }
    }
}
