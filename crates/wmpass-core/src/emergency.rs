//! Emergency access by Shamir shares. A random recovery key opens a dedicated slot,
//! and the key is split so that any `k` of `n` trustees can rebuild it. There is no
//! server, so the stated waiting period is advice, not a lock: trustees who hold
//! enough shares can act at once. The docs and the kit both say this.

use rand::rand_core::UnwrapErr;
use rand::rngs::SysRng;
use serde::{Deserialize, Serialize};
use vsss_rs::Gf256;
use zeroize::Zeroizing;

use crate::format::{Header, RootKey, SlotAction, Unlock, VaultKey};
use crate::otp::{base32_decode, base32_encode};
use crate::vault::Vault;
use crate::{Error, Result, crypto};

const PREFIX: &str = "WMPS1-";
const CHECK_LEN: usize = 4;
const MAX_SHARES: u8 = 32;

/// What one trustee receives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrusteeShare {
    /// The share as text. Treat it like a password.
    pub text: String,
    /// Which share this is.
    pub number: u8,
}

/// Facts printed on every trustee's kit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KitInfo {
    /// Shares needed.
    pub threshold: u8,
    /// Shares made.
    pub total: u8,
    /// The owner's wish for how long trustees wait before acting. Not enforced.
    pub stated_wait_days: u32,
}

fn encode_share(raw: &[u8], threshold: u8, total: u8) -> String {
    let mut body = Vec::with_capacity(raw.len() + 2 + CHECK_LEN);
    body.push(threshold);
    body.push(total);
    body.extend_from_slice(raw);
    let check = blake3::hash(&body);
    body.extend_from_slice(&check.as_bytes()[..CHECK_LEN]);
    let encoded = base32_encode(&body);
    let groups: Vec<&str> = encoded
        .as_bytes()
        .chunks(5)
        .filter_map(|c| std::str::from_utf8(c).ok())
        .collect();
    format!("{PREFIX}{}", groups.join("-"))
}

fn decode_share(text: &str) -> Result<(Zeroizing<Vec<u8>>, u8, u8)> {
    let bad = || Error::Invalid("that share was not typed correctly".to_string());
    let trimmed = text.trim();
    let upper = trimmed.to_ascii_uppercase();
    let rest = upper.strip_prefix(PREFIX).ok_or_else(bad)?;
    let body = base32_decode(rest).map_err(|_| bad())?;
    let split = body.len().checked_sub(CHECK_LEN).ok_or_else(bad)?;
    let (data, check) = body.split_at(split);
    if &blake3::hash(data).as_bytes()[..CHECK_LEN] != check || data.len() < 3 {
        return Err(bad());
    }
    Ok((Zeroizing::new(data[2..].to_vec()), data[0], data[1]))
}

/// Adds a recovery slot and returns one share per trustee. `threshold` shares open
/// the vault; fewer reveal nothing about the key.
pub fn create_shares(
    vault: &mut Vault,
    threshold: u8,
    total: u8,
    now_ms: u64,
) -> Result<Vec<TrusteeShare>> {
    if threshold < 2 || threshold > total || total > MAX_SHARES {
        return Err(Error::Invalid(
            "choose at least two trustees, and no more than 32".into(),
        ));
    }
    let secret = crypto::random_key()?;
    let root = RootKey::from_high_entropy(&secret)?;
    vault.add_slot_with_root(&root, "Emergency access", SlotAction::Open, None, now_ms)?;
    let rng = UnwrapErr(SysRng);
    let shares = Gf256::split_bytes(usize::from(threshold), usize::from(total), &secret[..], rng)
        .map_err(|_| Error::Crypto)?;
    Ok(shares
        .into_iter()
        .enumerate()
        .map(|(index, share)| {
            let share = Zeroizing::new(share);
            TrusteeShare {
                text: encode_share(&share, threshold, total),
                number: index as u8 + 1,
            }
        })
        .collect())
}

/// Rebuilds the key from shares and opens a vault file.
pub fn recover(file: &[u8], shares: &[&str], device: u32) -> Result<Vault> {
    let mut raw = Vec::with_capacity(shares.len());
    let mut threshold = 0;
    for share in shares {
        let (bytes, needed, _) = decode_share(share)?;
        threshold = needed;
        raw.push(bytes.to_vec());
    }
    if shares.len() < usize::from(threshold) {
        return Err(Error::Invalid(format!(
            "{threshold} shares are needed, and {} were given",
            shares.len()
        )));
    }
    let mut secret = Gf256::combine_bytes(&raw)
        .map_err(|_| Error::Invalid("those shares do not belong together".into()))?;
    for share in &mut raw {
        zeroize::Zeroize::zeroize(share);
    }
    let bytes: [u8; 32] = secret
        .as_slice()
        .try_into()
        .map_err(|_| Error::Invalid("those shares do not belong together".into()))?;
    let bytes = Zeroizing::new(bytes);
    zeroize::Zeroize::zeroize(&mut secret);
    let root = RootKey::from_high_entropy(&bytes)?;
    let header = Header::parse(file)?;
    match header.unlock(&root, &[])? {
        Unlock::Opened { key, .. } => open_recovered(file, key, device),
        _ => Err(Error::Invalid("those shares do not open this vault".into())),
    }
}

fn open_recovered(file: &[u8], key: VaultKey, device: u32) -> Result<Vault> {
    Vault::from_file(file, key, device)
}

/// A note a trustee's app drops in a shared location when a claim begins, so the
/// owner's devices can raise an alert. Rekeying the vault in response stops the
/// shares from opening any copy written afterwards.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimNotice {
    /// Vault id, as hex.
    pub vault_id: String,
    /// Trustee's own label.
    pub trustee: String,
    /// When the claim began, in milliseconds.
    pub claimed_ms: u64,
    /// When the trustee's app will let them proceed. Honour system only.
    pub proceed_after_ms: u64,
}

impl ClaimNotice {
    /// Starts a claim for a vault file.
    pub fn new(file: &[u8], trustee: &str, now_ms: u64, wait_days: u32) -> Result<Self> {
        let header = Header::parse(file)?;
        Ok(Self {
            vault_id: crate::vault::hex(header.vault_id()),
            trustee: trustee.to_owned(),
            claimed_ms: now_ms,
            proceed_after_ms: now_ms.saturating_add(u64::from(wait_days) * 86_400_000),
        })
    }

    /// Whether the stated wait has passed.
    pub fn wait_is_over(&self, now_ms: u64) -> bool {
        now_ms >= self.proceed_after_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::KdfParams;
    use crate::format::KdfConfig;
    use crate::vault::{login_entry, test_support::fast_vault};

    fn setup() -> (Vec<u8>, Vec<TrusteeShare>, String) {
        let mut vault = fast_vault(1);
        let at = vault.tick(2000);
        let id = vault
            .add(login_entry("Mail", "me", "s3cret", at), 2000)
            .unwrap();
        let shares = create_shares(&mut vault, 2, 3, 3000).unwrap();
        (vault.save().unwrap(), shares, id)
    }

    #[test]
    fn any_two_of_three_recover_and_one_does_not() {
        let (file, shares, id) = setup();
        for pair in [[0, 1], [0, 2], [1, 2]] {
            let texts = [shares[pair[0]].text.as_str(), shares[pair[1]].text.as_str()];
            let vault = recover(&file, &texts, 9).unwrap();
            assert_eq!(
                vault.get(&id).unwrap().login().unwrap().password.v.expose(),
                "s3cret"
            );
        }
        let err = recover(&file, &[shares[0].text.as_str()], 9).unwrap_err();
        assert!(err.to_string().contains("2 shares are needed"), "{err}");
    }

    #[test]
    fn all_three_also_work() {
        let (file, shares, _) = setup();
        let texts: Vec<&str> = shares.iter().map(|s| s.text.as_str()).collect();
        assert!(recover(&file, &texts, 9).is_ok());
    }

    #[test]
    fn a_typo_is_caught_by_the_checksum() {
        let (file, shares, _) = setup();
        // Every position but the final character, whose low bits can be padding
        // that decodes to the same bytes.
        let original = shares[0].text.clone();
        for at in PREFIX.len()..original.len() - 1 {
            if original.as_bytes()[at] == b'-' {
                continue;
            }
            let mut typo = original.clone();
            let flip = if original.as_bytes()[at] == b'A' {
                "B"
            } else {
                "A"
            };
            typo.replace_range(at..=at, flip);
            assert!(
                recover(&file, &[&typo, shares[1].text.as_str()], 9).is_err(),
                "position {at}"
            );
        }
    }

    #[test]
    fn shares_are_case_tolerant_and_trimmed() {
        let (file, shares, _) = setup();
        let lower = format!("  {}  ", shares[0].text.to_lowercase());
        assert!(recover(&file, &[&lower, shares[1].text.as_str()], 9).is_ok());
    }

    #[test]
    fn shares_from_another_set_do_not_open_it() {
        let (file, shares, _) = setup();
        let (_, others, _) = setup();
        assert!(
            recover(
                &file,
                &[others[0].text.as_str(), others[1].text.as_str()],
                9
            )
            .is_err()
        );
        assert!(
            recover(
                &file,
                &[shares[0].text.as_str(), others[1].text.as_str()],
                9
            )
            .is_err()
        );
    }

    #[test]
    fn rekeying_stops_the_shares() {
        let mut vault = fast_vault(1);
        let shares = create_shares(&mut vault, 2, 2, 3000).unwrap();
        let config = KdfConfig::new(KdfParams {
            m_kib: 64 * 1024,
            t: 3,
            p: 1,
        })
        .unwrap();
        vault.rekey(b"new password", &config, 4000).unwrap();
        let file = vault.save().unwrap();
        assert!(
            recover(
                &file,
                &[shares[0].text.as_str(), shares[1].text.as_str()],
                9
            )
            .is_err()
        );
    }

    #[test]
    fn bad_counts_are_refused() {
        let mut vault = fast_vault(1);
        assert!(create_shares(&mut vault, 1, 3, 1).is_err());
        assert!(create_shares(&mut vault, 4, 3, 1).is_err());
        assert!(create_shares(&mut vault, 2, 200, 1).is_err());
    }

    #[test]
    fn claim_notice_counts_down() {
        let (file, _, _) = setup();
        let notice = ClaimNotice::new(&file, "Sam", 1000, 7).unwrap();
        assert!(!notice.wait_is_over(1000));
        assert!(notice.wait_is_over(1000 + 7 * 86_400_000));
    }
}
