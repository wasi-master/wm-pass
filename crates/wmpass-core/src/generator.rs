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
    /// `a` lower, `A` upper, `9` digit, `#` symbol, `?` any, `\` literal next.
    Pattern(String),
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
        Preset {
            name: "Memorable".into(),
            recipe: Recipe::Passphrase(PassphraseSpec::default()),
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

fn pool(chars: &str, spec: &PasswordSpec) -> Vec<char> {
    chars
        .chars()
        .filter(|c| !spec.exclude.contains(*c))
        .filter(|c| !spec.exclude_ambiguous || !AMBIGUOUS.contains(*c))
        .collect()
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
    if length < classes.len() || !(4..=1024).contains(&length) {
        return Err(Error::Invalid(
            "that length does not fit the settings".into(),
        ));
    }
    let all: Vec<char> = classes.iter().flatten().copied().collect();
    for _ in 0..256 {
        let mut chars = Vec::with_capacity(length);
        for class in &classes {
            chars.push(pick(class)?);
        }
        while chars.len() < length {
            chars.push(pick(&all)?);
        }
        shuffle(&mut chars)?;
        if !too_many_repeats(&chars, spec.max_consecutive) {
            return Ok(Secret::from(chars.into_iter().collect::<String>()));
        }
    }
    Err(Error::Invalid(
        "those settings leave too few possibilities".into(),
    ))
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
    let mut out = String::new();
    let mut chars = template.chars();
    while let Some(ch) = chars.next() {
        match ch {
            'a' => out.push(pick(&lower)?),
            'A' => out.push(pick(&upper)?),
            '9' => out.push(pick(&digits)?),
            '#' => out.push(pick(&symbols)?),
            '?' => out.push(pick(&any)?),
            '\\' => out.extend(chars.next()),
            other => out.push(other),
        }
    }
    Ok(Secret::from(out))
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
    }
}

/// Entropy of what a recipe produces, in bits. Uses the search space, not the
/// result, so it is the honest number.
pub fn entropy_bits(recipe: &Recipe) -> f64 {
    match recipe {
        Recipe::Password(spec) => {
            let size: usize = classes(spec).iter().map(Vec::len).sum();
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
    let limit = rules.max_consecutive.unwrap_or(0);
    for _ in 0..256 {
        let mut chars = Vec::with_capacity(target);
        for pool in &pools {
            chars.push(pick(pool)?);
        }
        while chars.len() < target {
            chars.push(pick(&every)?);
        }
        shuffle(&mut chars)?;
        if !too_many_repeats(&chars, limit) {
            return Ok(Secret::from(chars.into_iter().collect::<String>()));
        }
    }
    Err(Error::Invalid("those site rules cannot be met".into()))
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
