//! YubiKey HMAC-SHA1 challenge-response, as KeePassXC and others use it. The 32-byte challenge
//! is padded to 64 bytes the same way, so a slot set up for those tools needs no changes.

use wmpass_core::format::{FactorKind, FactorOutput};
use zeroize::Zeroizing;

use crate::transport::ApduDevice;
use crate::{HwError, Result};

const CHALLENGE_SIZE: usize = 64;
const RESPONSE_SIZE: usize = 20;
const OTP_AID: [u8; 7] = [0xa0, 0x00, 0x00, 0x05, 0x27, 0x20, 0x01];
const CRC_OK_RESIDUAL: u16 = 0xf0b8;
const WRITE_FLAG: u8 = 0x80;
const RESPONSE_PENDING: u8 = 0x40;
const REPORT_DATA: usize = 7;
const FRAME_SIZE: usize = 70;
const RESPONSE_FRAME: usize = RESPONSE_SIZE + 2;

/// A new challenge for a slot.
pub fn enroll(slot: u8) -> Result<FactorKind> {
    if slot != 1 && slot != 2 {
        return Err(HwError::SlotNotConfigured);
    }
    let challenge = wmpass_core::crypto::random_array().map_err(|_| HwError::Crypto)?;
    Ok(FactorKind::YubiKey { challenge, slot })
}

fn parts(factor: &FactorKind) -> Result<([u8; 32], u8)> {
    match factor {
        FactorKind::YubiKey { challenge, slot } => Ok((*challenge, *slot)),
        _ => Err(HwError::Protocol),
    }
}

fn padded(challenge: &[u8; 32]) -> [u8; CHALLENGE_SIZE] {
    let mut out = [32u8; CHALLENGE_SIZE];
    out[..32].copy_from_slice(challenge);
    out
}

fn slot_command(slot: u8) -> Result<u8> {
    match slot {
        1 => Ok(0x30),
        2 => Ok(0x38),
        _ => Err(HwError::SlotNotConfigured),
    }
}

/// Over NFC.
pub fn derive_nfc(device: &mut dyn ApduDevice, factor: &FactorKind) -> Result<FactorOutput> {
    let (challenge, slot) = parts(factor)?;
    let mut select = vec![0x00, 0xa4, 0x04, 0x00, u8::try_from(OTP_AID.len()).map_err(|_| HwError::Protocol)?];
    select.extend_from_slice(&OTP_AID);
    let answer = device.transceive(&select)?;
    if !answer.ends_with(&[0x90, 0x00]) {
        return Err(HwError::Unsupported);
    }
    let padded = padded(&challenge);
    let mut apdu = vec![0x00, 0x01, slot_command(slot)?, 0x00, u8::try_from(CHALLENGE_SIZE).map_err(|_| HwError::Protocol)?];
    apdu.extend_from_slice(&padded);
    let mut answer = device.transceive(&apdu)?;
    if !answer.ends_with(&[0x90, 0x00]) {
        return Err(HwError::SlotNotConfigured);
    }
    answer.truncate(answer.len() - 2);
    if answer.len() != RESPONSE_SIZE {
        return Err(HwError::Protocol);
    }
    Ok(Zeroizing::new(answer))
}

/// A YubiKey's OTP interface over USB: 8-byte HID feature reports.
pub trait OtpHid {
    /// Reads a feature report.
    fn get_report(&mut self) -> Result<[u8; 8]>;
    /// Writes a feature report.
    fn set_report(&mut self, report: &[u8; 8]) -> Result<()>;
    /// Waits a moment between polls.
    fn pause(&mut self);
}

/// The checksum the key uses: CRC-16/ISO-13239.
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xffff;
    for byte in data {
        crc ^= u16::from(*byte);
        for _ in 0..8 {
            let low = crc & 1;
            crc >>= 1;
            if low != 0 {
                crc ^= 0x8408;
            }
        }
    }
    crc
}

fn build_frame(challenge: &[u8; CHALLENGE_SIZE], command: u8) -> [u8; FRAME_SIZE] {
    let mut frame = [0u8; FRAME_SIZE];
    frame[..CHALLENGE_SIZE].copy_from_slice(challenge);
    frame[CHALLENGE_SIZE] = command;
    let crc = !crc16(challenge);
    frame[CHALLENGE_SIZE + 1..CHALLENGE_SIZE + 3].copy_from_slice(&crc.to_le_bytes());
    frame
}

/// Over USB. Polls while the key waits for a touch, up to `polls` reads.
pub fn derive_usb(device: &mut dyn OtpHid, factor: &FactorKind, polls: usize) -> Result<FactorOutput> {
    let (challenge, slot) = parts(factor)?;
    let frame = build_frame(&padded(&challenge), slot_command(slot)?);

    // Clear anything left from an earlier command: an all-zero report at sequence 0x8f resets it.
    let mut reset = [0u8; 8];
    reset[7] = 0x8f;
    device.set_report(&reset)?;

    for (sequence, block) in frame.chunks(REPORT_DATA).enumerate() {
        // Blocks of zeros in the middle are skipped, as the protocol allows, but the first
        // and last are always sent.
        let edge = sequence == 0 || sequence == FRAME_SIZE / REPORT_DATA - 1;
        if !edge && block.iter().all(|byte| *byte == 0) {
            continue;
        }
        wait_ready(device, polls)?;
        let mut report = [0u8; 8];
        report[..block.len()].copy_from_slice(block);
        report[7] = WRITE_FLAG | u8::try_from(sequence).map_err(|_| HwError::Protocol)?;
        device.set_report(&report)?;
    }
    read_response(device, polls)
}

fn wait_ready(device: &mut dyn OtpHid, polls: usize) -> Result<()> {
    for _ in 0..polls {
        if device.get_report()?[7] & WRITE_FLAG == 0 {
            return Ok(());
        }
        device.pause();
    }
    Err(HwError::Transport)
}

fn read_response(device: &mut dyn OtpHid, polls: usize) -> Result<FactorOutput> {
    let mut out = Zeroizing::new(Vec::with_capacity(RESPONSE_FRAME + REPORT_DATA));
    let mut expected = 0u8;
    for _ in 0..polls {
        let report = device.get_report()?;
        let flags = report[7];
        if flags & RESPONSE_PENDING != 0 {
            if flags & 0x1f != expected {
                return Err(HwError::Protocol);
            }
            expected += 1;
            out.extend_from_slice(&report[..REPORT_DATA]);
            if out.len() >= RESPONSE_FRAME {
                let mut reset = [0u8; 8];
                reset[7] = 0x8f;
                device.set_report(&reset)?;
                if crc16(&out[..RESPONSE_FRAME]) != CRC_OK_RESIDUAL {
                    return Err(HwError::Crypto);
                }
                out.truncate(RESPONSE_SIZE);
                return Ok(out);
            }
        } else {
            // Waiting for a touch, or not ready yet: the same answer either way.
            device.pause();
        }
    }
    Err(HwError::NotConfirmed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hmac::{Hmac, KeyInit, Mac};
    use sha1::Sha1;

    fn hmac(key: &[u8; 20], message: &[u8]) -> [u8; 20] {
        let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(key).unwrap();
        mac.update(message);
        let mut out = [0u8; 20];
        out.copy_from_slice(&mac.finalize().into_bytes());
        out
    }

    /// Strips the padding the way a key in "less than 64 bytes" mode does.
    fn unpadded(challenge: &[u8]) -> &[u8] {
        let last = *challenge.last().unwrap();
        let mut end = challenge.len();
        while end > 0 && challenge[end - 1] == last {
            end -= 1;
        }
        &challenge[..end]
    }

    /// Follows the published YubiKey frame format.
    struct Key {
        secret: [u8; 20],
        received: [u8; FRAME_SIZE],
        answer: Vec<u8>,
        cursor: usize,
        touch_polls: usize,
    }

    impl OtpHid for Key {
        fn get_report(&mut self) -> Result<[u8; 8]> {
            let mut report = [0u8; 8];
            if self.answer.is_empty() {
                return Ok(report);
            }
            if self.touch_polls > 0 {
                self.touch_polls -= 1;
                report[7] = 0x20 | 10;
                return Ok(report);
            }
            if self.cursor * REPORT_DATA < self.answer.len() {
                let start = self.cursor * REPORT_DATA;
                let end = (start + REPORT_DATA).min(self.answer.len());
                report[..end - start].copy_from_slice(&self.answer[start..end]);
                report[7] = RESPONSE_PENDING | u8::try_from(self.cursor).unwrap();
                self.cursor += 1;
            }
            Ok(report)
        }

        fn set_report(&mut self, report: &[u8; 8]) -> Result<()> {
            if report[7] == 0x8f {
                self.answer.clear();
                self.cursor = 0;
                return Ok(());
            }
            let sequence = usize::from(report[7] & 0x1f);
            self.received[sequence * REPORT_DATA..sequence * REPORT_DATA + REPORT_DATA].copy_from_slice(&report[..REPORT_DATA]);
            if sequence == FRAME_SIZE / REPORT_DATA - 1 {
                let crc = u16::from_le_bytes([self.received[65], self.received[66]]);
                assert_eq!(crc, !crc16(&self.received[..64]), "bad checksum in the frame");
                let response = hmac(&self.secret, unpadded(&self.received[..64]));
                let mut answer = response.to_vec();
                let crc = !crc16(&response);
                answer.extend_from_slice(&crc.to_le_bytes());
                self.answer = answer;
            }
            Ok(())
        }

        fn pause(&mut self) {}
    }

    fn key() -> Key {
        Key { secret: [0x0b; 20], received: [0; FRAME_SIZE], answer: Vec::new(), cursor: 0, touch_polls: 3 }
    }

    #[test]
    fn checksum_matches_the_residual_rule() {
        let data = b"\x01\x02\x03";
        let mut framed = data.to_vec();
        framed.extend_from_slice(&(!crc16(data)).to_le_bytes());
        assert_eq!(crc16(&framed), CRC_OK_RESIDUAL);
    }

    #[test]
    fn usb_exchange_gives_hmac_sha1_of_the_challenge() {
        let factor = FactorKind::YubiKey { challenge: [0x11; 32], slot: 2 };
        let mut device = key();
        let output = derive_usb(&mut device, &factor, 100).unwrap();
        assert_eq!(&output[..], hmac(&[0x0b; 20], &[0x11; 32]));
        assert_eq!(device.received[64], 0x38, "slot two");
    }

    #[test]
    fn a_key_that_is_not_touched_times_out() {
        let factor = FactorKind::YubiKey { challenge: [0x11; 32], slot: 1 };
        let mut device = key();
        device.touch_polls = 1000;
        assert_eq!(derive_usb(&mut device, &factor, 10).err(), Some(HwError::NotConfirmed));
    }

    struct Tag {
        answers: Vec<Vec<u8>>,
        sent: Vec<Vec<u8>>,
    }

    impl ApduDevice for Tag {
        fn transceive(&mut self, apdu: &[u8]) -> Result<Vec<u8>> {
            self.sent.push(apdu.to_vec());
            Ok(self.answers.remove(0))
        }
    }

    #[test]
    fn nfc_exchange_uses_the_otp_applet() {
        let mut reply = vec![7u8; 20];
        reply.extend_from_slice(&[0x90, 0x00]);
        let mut tag = Tag { answers: vec![vec![0x90, 0x00], reply], sent: Vec::new() };
        let factor = FactorKind::YubiKey { challenge: [1; 32], slot: 1 };
        assert_eq!(&derive_nfc(&mut tag, &factor).unwrap()[..], &[7u8; 20]);
        assert_eq!(tag.sent[0][5..], OTP_AID);
        assert_eq!(tag.sent[1][..5], [0x00, 0x01, 0x30, 0x00, 64]);
        assert_eq!(tag.sent[1][5 + 32], 32, "padded to 64");

        let mut empty = Tag { answers: vec![vec![0x90, 0x00], vec![0x6a, 0x80]], sent: Vec::new() };
        assert_eq!(derive_nfc(&mut empty, &factor).err(), Some(HwError::SlotNotConfigured));
    }

    #[test]
    fn slots_outside_one_and_two_are_refused() {
        assert_eq!(enroll(3).err(), Some(HwError::SlotNotConfigured));
        assert!(matches!(enroll(2).unwrap(), FactorKind::YubiKey { slot: 2, .. }));
    }
}
