//! Little-endian bytes, which is every integer SMB and NTLM have.

/// A reply too short for a field it should hold. Reads are checked because the bytes
/// come from the server: a short reply must be an error, never a panic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Malformed;

/// A little-endian writer. Positions are absolute so a length or an offset can be
/// patched in after the thing it describes has been written.
#[derive(Debug, Default)]
pub(crate) struct LeWriter(Vec<u8>);

impl LeWriter {
    pub(crate) fn new() -> Self {
        Self(Vec::with_capacity(128))
    }

    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    pub(crate) fn u8(&mut self, v: u8) -> &mut Self {
        self.0.push(v);
        self
    }

    pub(crate) fn u16(&mut self, v: u16) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }

    pub(crate) fn u32(&mut self, v: u32) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }

    pub(crate) fn u64(&mut self, v: u64) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }

    pub(crate) fn bytes(&mut self, b: &[u8]) -> &mut Self {
        self.0.extend_from_slice(b);
        self
    }

    pub(crate) fn zeros(&mut self, n: usize) -> &mut Self {
        self.0.resize(self.0.len() + n, 0);
        self
    }

    /// Pads with zeros to a multiple of `n`.
    pub(crate) fn align(&mut self, n: usize) -> &mut Self {
        while !self.0.len().is_multiple_of(n) {
            self.0.push(0);
        }
        self
    }

    pub(crate) fn put_u32(&mut self, at: usize, v: u32) {
        if let Some(slot) = self.0.get_mut(at..at + 4) {
            slot.copy_from_slice(&v.to_le_bytes());
        }
    }

    pub(crate) fn into_vec(self) -> Vec<u8> {
        self.0
    }
}

/// Checked little-endian reads.
pub(crate) trait Le {
    fn slice_at(&self, at: usize, len: usize) -> Result<&[u8], Malformed>;

    fn u8_at(&self, at: usize) -> Result<u8, Malformed> {
        Ok(self.slice_at(at, 1)?[0])
    }

    fn u16_at(&self, at: usize) -> Result<u16, Malformed> {
        let b = self.slice_at(at, 2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32_at(&self, at: usize) -> Result<u32, Malformed> {
        let b = self.slice_at(at, 4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn u64_at(&self, at: usize) -> Result<u64, Malformed> {
        Ok(u64::from(self.u32_at(at)?) | (u64::from(self.u32_at(at + 4)?) << 32))
    }
}

impl Le for [u8] {
    fn slice_at(&self, at: usize, len: usize) -> Result<&[u8], Malformed> {
        at.checked_add(len).and_then(|end| self.get(at..end)).ok_or(Malformed)
    }
}

/// UTF-16LE, which every SMB and NTLM string is.
pub(crate) fn utf16(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

pub(crate) fn from_utf16(b: &[u8]) -> String {
    let units: Vec<u16> = b.as_chunks::<2>().0.iter().map(|pair| u16::from_le_bytes(*pair)).collect();
    String::from_utf16_lossy(&units)
}

/// Windows FILETIME now: 100 ns ticks since 1601.
pub(crate) fn filetime_now() -> u64 {
    const EPOCH_DIFF_MS: u64 = 11_644_473_600_000;
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(0));
    (ms + EPOCH_DIFF_MS) * 10_000
}
