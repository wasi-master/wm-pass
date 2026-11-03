//! Proton's SRP-6a and password hashing, ported from go-srp, which Proton's own clients use.
//!
//! Unlike textbook SRP everything is little-endian, the hash is [`expand_hash`] (four
//! SHA-512s, 2048 bits), and the password goes through bcrypt with a salt that has
//! "proton" appended before it is stretched.

use base64::Engine;
use blowfish::Blowfish;
use num_bigint::BigUint;
use sha2::{Digest, Sha512};

const BITS: u64 = 2048;
const BYTES: usize = 256;
const BCRYPT_COST: u32 = 10;
const DOT_SLASH: &[u8; 64] = b"./ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

/// The client's half of a sign-in: what goes to the server, and the proof the server
/// should send back.
#[derive(Clone)]
pub struct Proofs {
    /// A, little-endian, 256 bytes.
    pub client_ephemeral: Vec<u8>,
    /// M1.
    pub client_proof: Vec<u8>,
    /// What the server must answer with: without this match a machine in the middle
    /// could have answered.
    pub expected_server_proof: Vec<u8>,
}

impl std::fmt::Debug for Proofs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Proofs(***)")
    }
}

/// SHA-512 four times, each over the data and one counter byte, concatenated.
pub fn expand_hash(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(BYTES);
    for i in 0..4u8 {
        let mut digest = Sha512::new();
        digest.update(data);
        digest.update([i]);
        out.extend_from_slice(&digest.finalize());
    }
    out
}

/// bcrypt's own base64: its alphabet, no padding.
pub fn dot_slash(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 4 / 3 + 2);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = chunk.get(1).copied().unwrap_or(0) as usize;
        let b2 = chunk.get(2).copied().unwrap_or(0) as usize;
        out.push(DOT_SLASH[b0 >> 2] as char);
        out.push(DOT_SLASH[((b0 & 3) << 4) | (b1 >> 4)] as char);
        if chunk.len() > 1 {
            out.push(DOT_SLASH[((b1 & 0xf) << 2) | (b2 >> 6)] as char);
        }
        if chunk.len() > 2 {
            out.push(DOT_SLASH[b2 & 0x3f] as char);
        }
    }
    out
}

fn from_dot_slash(text: &str, bytes: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes);
    let (mut bits, mut acc) = (0u32, 0u32);
    for c in text.bytes() {
        let value = DOT_SLASH.iter().position(|&d| d == c).unwrap_or(0) as u32;
        acc = ((acc << 6) | value) & 0xffff;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            if out.len() < bytes {
                out.push((acc >> bits) as u8);
            }
        }
    }
    out.resize(bytes, 0);
    out
}

/// bcrypt with Go's interface: `$2y$10$` and the salt as bcrypt's 22 characters, the
/// whole 60-character string back. Passwords past 72 bytes are cut, as bcrypt does.
pub fn bcrypt(password: &[u8], encoded_salt: &str) -> String {
    let salt = from_dot_slash(encoded_salt.get(..22).unwrap_or(encoded_salt), 16);
    // NUL-terminated, at most 72 bytes in all: what OpenBSD's bcrypt hashes.
    let key: Vec<u8> = password.iter().copied().chain([0]).take(72).collect();
    let mut state = Blowfish::bc_init_state();
    state.salted_expand_key(&salt, &key);
    for _ in 0..(1u32 << BCRYPT_COST) {
        state.bc_expand_key(&key);
        state.bc_expand_key(&salt);
    }
    // "OrpheanBeholderScryDoubt", as six big-endian words.
    let mut text = [0x4f72_7068, 0x6561_6e42, 0x6568_6f6c, 0x6465_7253, 0x6372_7944, 0x6f75_6274u32];
    for pair in text.chunks_mut(2) {
        let mut lr = [pair[0], pair[1]];
        for _ in 0..64 {
            lr = state.bc_encrypt(lr);
        }
        pair.copy_from_slice(&lr);
    }
    let hash: Vec<u8> = text.iter().flat_map(|word| word.to_be_bytes()).collect();
    format!("$2y${BCRYPT_COST:02}${}{}", dot_slash(&salt), dot_slash(&hash[..23]))
}

/// The key password: bcrypt over the key salt, the hash part alone (the last 31
/// characters).
pub fn salted_key_pass(password: &[u8], key_salt: &[u8]) -> String {
    let crypted = bcrypt(password, &dot_slash(key_salt));
    crypted[crypted.len() - 31..].to_owned()
}

/// The SRP x, for auth versions 3 and 4, which every current account is.
pub fn hash_password(password: &[u8], salt: &[u8], modulus: &[u8]) -> Vec<u8> {
    let mut salted = salt.to_vec();
    salted.extend_from_slice(b"proton");
    let mut input = bcrypt(password, &dot_slash(&salted)).into_bytes();
    input.extend_from_slice(modulus);
    expand_hash(&input)
}

/// Little-endian bytes to a number.
pub fn to_int(bytes: &[u8]) -> BigUint {
    BigUint::from_bytes_le(bytes)
}

/// A number to 256 little-endian bytes.
pub fn from_int(n: &BigUint) -> Vec<u8> {
    let mut out = n.to_bytes_le();
    out.resize(BYTES, 0);
    out
}

/// The modulus text out of its clear-signed message. The signature itself is not
/// checked here; [`proofs`] instead checks the number is a safe prime of the right
/// shape, which is what the signature protects.
pub fn modulus_of(signed: &str) -> Option<Vec<u8>> {
    let text = signed.replace('\r', "");
    let lines: Vec<&str> = text.split('\n').collect();
    let start = lines.iter().position(|line| line.is_empty())?;
    let end = lines.iter().position(|line| line.starts_with("-----BEGIN PGP SIGNATURE"))?;
    if end <= start {
        return None;
    }
    let body: String = lines[start + 1..end].concat();
    base64::engine::general_purpose::STANDARD.decode(body.trim()).ok()
}

fn random_below(limit: &BigUint) -> Option<BigUint> {
    let mut bytes = vec![0u8; (limit.bits() as usize).div_ceil(8) + 8];
    getrandom::fill(&mut bytes).ok()?;
    Some(BigUint::from_bytes_be(&bytes) % limit)
}

/// Miller-Rabin with random bases.
fn probably_prime(n: &BigUint, rounds: usize) -> bool {
    let one = BigUint::from(1u32);
    let two = BigUint::from(2u32);
    if *n < two {
        return false;
    }
    for small in [2u32, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37] {
        let small = BigUint::from(small);
        if *n == small {
            return true;
        }
        if (n % &small) == BigUint::ZERO {
            return false;
        }
    }
    let n_minus_one = n - &one;
    let shift = n_minus_one.trailing_zeros().unwrap_or(0);
    let d = &n_minus_one >> shift;
    let span = n - BigUint::from(3u32);
    'rounds: for _ in 0..rounds {
        let Some(offset) = random_below(&span) else { return false };
        let base = offset + &two;
        let mut x = base.modpow(&d, n);
        if x == one || x == n_minus_one {
            continue;
        }
        for _ in 1..shift {
            x = x.modpow(&two, n);
            if x == n_minus_one {
                continue 'rounds;
            }
        }
        return false;
    }
    true
}

/// Whether `n` is a modulus go-srp would accept: 2048 bits, 3 mod 8, a safe prime with 2
/// as generator.
pub fn valid_modulus(n: &BigUint) -> bool {
    if n.bits() != BITS {
        return false;
    }
    if !n.bit(0) || !n.bit(1) || n.bit(2) {
        return false;
    }
    let half = n >> 1u32;
    if !probably_prime(&half, 10) {
        return false;
    }
    BigUint::from(2u32).modpow(&half, n) == n - BigUint::from(1u32)
}

/// The client proofs for a sign-in, or `None` when the server's numbers are not
/// acceptable.
pub fn proofs(password: &[u8], salt: &[u8], modulus: &[u8], server_ephemeral: &[u8]) -> Option<Proofs> {
    proofs_with(password, salt, modulus, server_ephemeral, None)
}

pub(crate) fn proofs_with(
    password: &[u8],
    salt: &[u8],
    modulus_bytes: &[u8],
    server_ephemeral: &[u8],
    secret: Option<BigUint>,
) -> Option<Proofs> {
    let one = BigUint::from(1u32);
    let n = to_int(modulus_bytes);
    let b = to_int(server_ephemeral);
    if !valid_modulus(&n) {
        return None;
    }
    let n_minus_one = &n - &one;
    if b <= one || b >= n_minus_one {
        return None;
    }

    let g = BigUint::from(2u32);
    let mut gn = from_int(&g);
    gn.extend_from_slice(&from_int(&n));
    let k = to_int(&expand_hash(&gn)) % &n;
    if k <= one || k >= n_minus_one {
        return None;
    }
    let x = to_int(&hash_password(password, salt, modulus_bytes));
    let b_bytes = from_int(&b);

    let lower = BigUint::from(BITS * 2);
    let pick = || -> Option<BigUint> {
        loop {
            let candidate = random_below(&n_minus_one)?;
            if candidate > lower {
                return Some(candidate);
            }
        }
    };
    let fixed = secret.is_some();
    let mut a = match secret {
        Some(secret) => secret,
        None => pick()?,
    };
    let scramble = |a: &BigUint| -> (Vec<u8>, BigUint) {
        let a_bytes = from_int(&g.modpow(a, &n));
        let mut ab = a_bytes.clone();
        ab.extend_from_slice(&b_bytes);
        (a_bytes, to_int(&expand_hash(&ab)))
    };
    let (mut a_bytes, mut u) = scramble(&a);
    while u == BigUint::ZERO && !fixed {
        a = pick()?;
        (a_bytes, u) = scramble(&a);
    }
    if u == BigUint::ZERO {
        return None;
    }

    let kgx = (&k * g.modpow(&x, &n)) % &n;
    let base = (&b + &n - kgx) % &n;
    let exponent = ((&u * &x) % &n_minus_one + &a) % &n_minus_one;
    let shared = from_int(&base.modpow(&exponent, &n));

    let mut client = a_bytes.clone();
    client.extend_from_slice(&b_bytes);
    client.extend_from_slice(&shared);
    let client_proof = expand_hash(&client);
    let mut server = a_bytes.clone();
    server.extend_from_slice(&client_proof);
    server.extend_from_slice(&shared);
    let expected_server_proof = expand_hash(&server);
    Some(Proofs { client_ephemeral: a_bytes, client_proof, expected_server_proof })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODULUS: &str = "W2z5HBi8RvsfYzZTS7qBaUxxPhsfHJFZpu3Kd6s1JafNrCCH9rfvPLrfuqocxWPgWDH2R8neK7PkNvjxto9TStuY5z7jAzWRvFWN9cQhAKkdWgy0JY6ywVn22+HFpF4cYesHrqFIKUPDMSSIlWjBVmEJZ/MusD44ZT29xcPrOqeZvwtCffKtGAIjLYPZIEbZKnDM1Dm3q2K/xS5h+xdhjnndhsrkwm9U9oyA2wxzSXFL+pdfj2fOdRwuR5nW0J2NFrq3kJjkRmpO/Genq1UW+TEknIWAb6VzJJJA244K/H8cnSx2+nSNZO3bbo6Ys228ruV9A8m6DhxmS+bihN3ttQ==";

    fn b64(text: &str) -> Vec<u8> {
        base64::engine::general_purpose::STANDARD.decode(text).unwrap()
    }

    #[test]
    fn bcrypt_matches_go_srp_vectors() {
        assert_eq!(
            bcrypt(b"test!!!", "PTTsDBs/mlLnSk6VmtFghe"),
            "$2y$10$PTTsDBs/mlLnSk6VmtFgheNSiK/lSwtJsrBLLDK3kZYI7193nInqy"
        );
        assert_eq!(
            bcrypt(b"test!!!", "/.3KXCwRnsrxURMGxN7.R."),
            "$2y$10$/.3KXCwRnsrxURMGxN7.R.GLpVq0zyBbI9wgS0wB2U/g2btx1RYoy"
        );
        let salt: Vec<u8> = (0..16).collect();
        assert_eq!(salted_key_pass(b"pw", &salt).len(), 31);
    }

    /// go-srp's `TestSRPauth`, which seeds math/rand with 42: the secret below is what
    /// that seed gives (`crypto/rand.Int` over it, run with go-srp v0.0.7), and the
    /// proofs are the test's published ones.
    #[test]
    fn proofs_match_go_srp_test_srp_auth() {
        let signed = format!(
            "-----BEGIN PGP SIGNED MESSAGE-----\nHash: SHA256\n\n{MODULUS}\n-----BEGIN PGP SIGNATURE-----\nVersion: ProtonMail\n\nx\n-----END PGP SIGNATURE-----"
        );
        let modulus = modulus_of(&signed).unwrap();
        assert_eq!(modulus, b64(MODULUS));
        let eph = b64("l13IQSVFBEV0ZZREuRQ4ZgP6OpGiIfIjbSDYQG3Yp39FkT2B/k3n1ZhwqrAdy+qvPPFq/le0b7UDtayoX4aOTJihoRvifas8Hr3icd9nAHqd0TUBbkZkT6Iy6UpzmirCXQtEhvGQIdOLuwvy+vZWh24G2ahBM75dAqwkP961EJMh67/I5PA5hJdQZjdPT5luCyVa7BS1d9ZdmuR0/VCjUOdJbYjgtIH7BQoZs+KacjhUN8gybu+fsycvTK3eC+9mCN2Y6GdsuCMuR3pFB0RF9eKae7cA6RbJfF1bjm0nNfWLXzgKguKBOeF3GEAsnCgK68q82/pq9etiUDizUlUBcA==");
        let secret = BigUint::parse_bytes(
            b"538c7f96b164bf1b97bb9f4bb472e89f5b1484f25209c9d9343e92ba09dd9d52dfd79b4d76429b617a0c9f9f0d3ba55b0cc0d6144c888535841acbe0709b0758083f61d375bc02b41df4f91929e18fda9e6f82e54e748e81e79e4bbd6fe34cdcba843ee8d63e8c4ffe1cebea546d8fac13dd1aac04ce2ea2877c5579cfa2c78e1b0bafae881b82a751108a42ed3c903caa43465a78620616978aed0ce3c6c4f3ae7bc3e0495b5712fefdbe0c102887e100dacd2d885f692cb607da00a11c1c7071e796a2dc2dc25a5b74b2e129705e273f05c92326828e2b056e3817658e1061498947fdf344410ed4c116023fa8e3576b6fed27ff8974bac0cafd9ad05692b1",
            16,
        )
        .unwrap();
        let salt = b64("yKlc5/CvObfoiw==");
        assert_eq!(
            hash_password(b"abc123", &salt, &modulus),
            b64("UHwyXL718YTYWiTYh8/LPLSBa7DgezL5QJXZEgzEeeomD0Mf62Zu6qNkQj0z/sDxf3fIGTn0U8gk4jWCBLMnmY5jPwvTX63SyvmWpF6LiMhWvz6bUZh+5z/hGQiI/YdRE4DW+2PXAmnGQJZ1MgTouDnLLavGuvSrC0qzzI37QNUDyAtFWwrXwbNqhpWUa1PQDJ+gHpQjnCXa7dVlpj5TQbDvPEZ9JrA7c37TopINerrBmU/jhcPUFp8EHwsScuRxxDBXIkYBP1gIVTtJSTPLXWKJlrdqNzHuJXxOrpeOiDjuIDFhWK2v/gXbyfwp53ph80VPd3eRPQWpCkO3iqKWUg==")
        );
        let fixed = proofs_with(b"abc123", &salt, &modulus, &eph, Some(secret)).unwrap();
        assert_eq!(
            fixed.client_ephemeral,
            b64("mK+eSMosfZO/Cs5s+vcbjpsN7F8UAObwlKKnCy/z9FpoMRM2PfTe5ywLBgffmLYaapPq7XOxaqaj08kcZLHcM1fIA2JQZZTKPnESN1qAQztJ3/YHMI0op6yBgzx9803OjIznjCD2B3XBSMOHIG4oG0UwocsIX32hiMnYlMMkt8NGrityPlnmEbxpRna3fu9LEZ+v0uo6PjKCrO7+9E3uaMi64HadXBfyx2raBFFwA+yh7FvE7U+hl3AJclEre4d8pmfhMdxXze1soJI8fMuqaa07rY0r0rF5mLLTuqTIGRFkU1qG9loq9+IMsSwgkt1P3ghW63JK7Y6LWdDy0d6cAg==")
        );
        assert_eq!(
            fixed.client_proof,
            b64("Qb+1+jEqHRqpJ3nEJX2FEj0kXgCIWHngO0eT4R2Idkwke/ceCIUmQa0RfTYU53ybO1AVergtb7N0W/3bathdHT9FAHhy0vDGQDg/yPnuUneqV76NuU+pQHnO83gcjmZjDq/zvRRSD7dtIORRK97xhdR9W9bG5XRGr2c9Zev40YVcXgUiNUG/0zHSKQfEhUpMKxdauKtGC+dZnZzU6xaU0qvulYEsraawurRf0b1VXwohM6KE52Fj5xlS2FWZ3Mg0WIOC5KW5ziI6QirEUDK2pH/Rxvu4HcW9aMuppUmHk9Bm6kdg99o3vl0G7OgmEI7y6iyEYmXqH44XGORJ2sDMxQ==")
        );
        assert_eq!(
            fixed.expected_server_proof,
            b64("SLCSIClioSAtozauZZzcJuVPyY+MjnxfJSgEe9y6RafgjlPqnhQTZclRKPGsEhxVyWan7PIzhL+frPyZNaE1QaV5zbqz1yf9RXpGyTjZwU3FuVCJpkhp6iiCK3Wd2SemxawFXC06dgAdJ7I3HKvfkXeMANOUUh5ofjnJtXg42OGp4x1lKoFcH+IbB/CvRNQCmRTyhOiBJmZyUFwxHXLT/h+PlD0XSehcyybIIBIsscQ7ZPVPxQw4BqlqoYzTjjXPJxLxeQUQm2g9bPzT+izuR0VOPDtjt+dXrWny90k2nzS0Bs2YvNIqbJn1aQwFZr42p/O1I9n5S3mYtMgGk/7b1g==")
        );
        // A random secret still gives an A in range and a proof of the right length.
        let random = proofs(b"abc123", &salt, &modulus, &eph).unwrap();
        assert_eq!((random.client_ephemeral.len(), random.client_proof.len()), (256, 256));
        assert_ne!(random.client_ephemeral, fixed.client_ephemeral);
    }

    #[test]
    fn a_modulus_that_is_not_a_safe_prime_is_refused() {
        let signed = format!(
            "-----BEGIN PGP SIGNED MESSAGE-----\nHash: SHA256\n\n{}\n-----BEGIN PGP SIGNATURE-----\n\nx\n-----END PGP SIGNATURE-----",
            base64::engine::general_purpose::STANDARD.encode([7u8; 256])
        );
        let modulus = modulus_of(&signed).unwrap();
        assert!(proofs(b"pw", &[0; 10], &modulus, &[3; 256]).is_none());
        // The real modulus with its top bit flipped is no prime either.
        let mut broken = b64(MODULUS);
        broken[0] ^= 0x08;
        assert!(!valid_modulus(&to_int(&broken)));
        assert!(valid_modulus(&to_int(&b64(MODULUS))));
        assert!(modulus_of("no blank line").is_none());
    }

    #[test]
    fn dot_slash_round_trips() {
        let bytes: Vec<u8> = (0..16).map(|i| i * 17).collect();
        let text = dot_slash(&bytes);
        assert_eq!(text.len(), 22);
        assert_eq!(from_dot_slash(&text, 16), bytes);
    }
}
