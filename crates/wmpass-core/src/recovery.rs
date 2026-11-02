//! Checks on recovery phrases. Phrases are stored whatever they hold, because plenty
//! of wallets use their own word lists, so a failed check is a warning only.

use bip39::Mnemonic;

/// What was found out about a phrase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhraseCheck {
    /// A valid BIP-39 phrase with this many words.
    Valid {
        /// Word count.
        words: usize,
    },
    /// BIP-39 words, but the checksum fails, so a word is probably wrong.
    BadChecksum,
    /// Not BIP-39. May still be right for its wallet.
    NotBip39,
}

/// Checks a phrase against the English BIP-39 list.
pub fn check_phrase(phrase: &str) -> PhraseCheck {
    match Mnemonic::parse_normalized(&phrase.split_whitespace().collect::<Vec<_>>().join(" ")) {
        Ok(found) => PhraseCheck::Valid {
            words: found.word_count(),
        },
        Err(bip39::Error::InvalidChecksum) => PhraseCheck::BadChecksum,
        Err(_) => PhraseCheck::NotBip39,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bip39_vectors() {
        let good = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        assert_eq!(check_phrase(good), PhraseCheck::Valid { words: 12 });
        assert_eq!(
            check_phrase(&format!("  {good}\n")),
            PhraseCheck::Valid { words: 12 }
        );
        let bad = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon";
        assert_eq!(check_phrase(bad), PhraseCheck::BadChecksum);
        assert_eq!(
            check_phrase("correct horse battery staple"),
            PhraseCheck::NotBip39
        );
    }
}
