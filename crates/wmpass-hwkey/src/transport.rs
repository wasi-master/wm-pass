//! Moving CTAP2 messages over USB HID and over NFC.

use crate::{HwError, Result};

/// Carries one CTAP2 message: a command byte and CBOR, answered by a status byte and CBOR.
pub trait Ctap2Transport {
    /// Sends and waits. May block for as long as the key waits for a touch.
    fn exchange(&mut self, request: &[u8]) -> Result<Vec<u8>>;
}

/// A raw USB HID device with 64-byte reports.
pub trait HidDevice {
    /// Writes one report.
    fn write(&mut self, report: &[u8; 64]) -> Result<()>;
    /// Reads one report. An error means nothing came within a sensible time.
    fn read(&mut self) -> Result<[u8; 64]>;
}

const BROADCAST: u32 = 0xffff_ffff;
const CMD_INIT: u8 = 0x86;
const CMD_CBOR: u8 = 0x90;
const CMD_KEEPALIVE: u8 = 0xbb;
const CMD_ERROR: u8 = 0xbf;
const INIT_DATA: usize = 57;
const CONT_DATA: usize = 59;
/// CTAPHID allows 7609 bytes in a message.
const MAX_MESSAGE: usize = 7609;

/// CTAPHID framing over a HID device.
#[derive(Debug)]
pub struct CtapHid<D> {
    device: D,
    channel: u32,
}

/// Splits a message into reports.
pub fn hid_frames(channel: u32, command: u8, payload: &[u8]) -> Result<Vec<[u8; 64]>> {
    if payload.len() > MAX_MESSAGE {
        return Err(HwError::Protocol);
    }
    let mut frames = Vec::new();
    let mut first = [0u8; 64];
    first[..4].copy_from_slice(&channel.to_be_bytes());
    first[4] = command;
    first[5..7].copy_from_slice(&u16::try_from(payload.len()).map_err(|_| HwError::Protocol)?.to_be_bytes());
    let head = payload.len().min(INIT_DATA);
    first[7..7 + head].copy_from_slice(&payload[..head]);
    frames.push(first);
    for (sequence, chunk) in payload[head..].chunks(CONT_DATA).enumerate() {
        let mut frame = [0u8; 64];
        frame[..4].copy_from_slice(&channel.to_be_bytes());
        frame[4] = u8::try_from(sequence).map_err(|_| HwError::Protocol)?;
        frame[5..5 + chunk.len()].copy_from_slice(chunk);
        frames.push(frame);
    }
    Ok(frames)
}

impl<D: HidDevice> CtapHid<D> {
    /// Opens a channel on the device.
    pub fn open(mut device: D, nonce: [u8; 8]) -> Result<Self> {
        for frame in hid_frames(BROADCAST, CMD_INIT, &nonce)? {
            device.write(&frame)?;
        }
        let mut this = Self { device, channel: BROADCAST };
        loop {
            let (command, answer) = this.read_message()?;
            if command != CMD_INIT {
                return Err(HwError::Protocol);
            }
            // Answers to other programs' requests can arrive on the broadcast channel too, and
            // the nonce tells them apart: one with another nonce is skipped, not an error.
            if answer.get(..8) != Some(&nonce[..]) {
                continue;
            }
            let channel = answer.get(8..12).ok_or(HwError::Protocol)?;
            this.channel = u32::from_be_bytes([channel[0], channel[1], channel[2], channel[3]]);
            return Ok(this);
        }
    }

    fn read_message(&mut self) -> Result<(u8, Vec<u8>)> {
        loop {
            let frame = self.device.read()?;
            if frame[..4] != self.channel.to_be_bytes() {
                continue;
            }
            let command = frame[4];
            if command == CMD_KEEPALIVE {
                continue;
            }
            if command == CMD_ERROR {
                return Err(HwError::Protocol);
            }
            if command & 0x80 == 0 {
                return Err(HwError::Protocol);
            }
            let total = usize::from(u16::from_be_bytes([frame[5], frame[6]]));
            if total > MAX_MESSAGE {
                return Err(HwError::Protocol);
            }
            let mut out = Vec::with_capacity(total);
            out.extend_from_slice(&frame[7..7 + total.min(INIT_DATA)]);
            let mut expected = 0u8;
            while out.len() < total {
                let next = self.device.read()?;
                // Another program's traffic on its own channel can come between the parts.
                if next[..4] != self.channel.to_be_bytes() {
                    continue;
                }
                if next[4] != expected {
                    return Err(HwError::Protocol);
                }
                expected = expected.wrapping_add(1);
                let take = (total - out.len()).min(CONT_DATA);
                out.extend_from_slice(&next[5..5 + take]);
            }
            return Ok((command, out));
        }
    }
}

impl<D: HidDevice> Ctap2Transport for CtapHid<D> {
    fn exchange(&mut self, request: &[u8]) -> Result<Vec<u8>> {
        for frame in hid_frames(self.channel, CMD_CBOR, request)? {
            self.device.write(&frame)?;
        }
        let (command, answer) = self.read_message()?;
        if command != CMD_CBOR {
            return Err(HwError::Protocol);
        }
        Ok(answer)
    }
}

/// Sends one APDU to an NFC tag. On Android, this wraps `IsoDep.transceive`.
pub trait ApduDevice {
    /// Sends and returns the answer with its two status bytes.
    fn transceive(&mut self, apdu: &[u8]) -> Result<Vec<u8>>;
}

const FIDO_AID: [u8; 8] = [0xa0, 0x00, 0x00, 0x06, 0x47, 0x2f, 0x00, 0x01];
/// Bytes of a command in one short APDU.
const APDU_CHUNK: usize = 250;
/// Far more than any CTAP2 answer a key sends over NFC.
const MAX_NFC_ANSWER: usize = 64 * 1024;

/// CTAP2 over NFC.
#[derive(Debug)]
pub struct CtapNfc<A> {
    device: A,
}

fn split_status(mut answer: Vec<u8>) -> Result<(Vec<u8>, u8, u8)> {
    if answer.len() < 2 {
        return Err(HwError::Protocol);
    }
    let low = answer.pop().ok_or(HwError::Protocol)?;
    let high = answer.pop().ok_or(HwError::Protocol)?;
    Ok((answer, high, low))
}

impl<A: ApduDevice> CtapNfc<A> {
    /// Selects the FIDO applet.
    pub fn open(mut device: A) -> Result<Self> {
        let mut select = vec![0x00, 0xa4, 0x04, 0x00, u8::try_from(FIDO_AID.len()).map_err(|_| HwError::Protocol)?];
        select.extend_from_slice(&FIDO_AID);
        let (_, high, low) = split_status(device.transceive(&select)?)?;
        if (high, low) != (0x90, 0x00) {
            return Err(HwError::Unsupported);
        }
        Ok(Self { device })
    }
}

impl<A: ApduDevice> Ctap2Transport for CtapNfc<A> {
    fn exchange(&mut self, request: &[u8]) -> Result<Vec<u8>> {
        let mut pieces = request.chunks(APDU_CHUNK).peekable();
        let mut answer = None;
        while let Some(piece) = pieces.next() {
            let last = pieces.peek().is_none();
            // Bit 0x10 of the class byte says more of this command follows.
            let class = if last { 0x80 } else { 0x90 };
            let mut apdu = vec![class, 0x10, 0x00, 0x00, u8::try_from(piece.len()).map_err(|_| HwError::Protocol)?];
            apdu.extend_from_slice(piece);
            if last {
                apdu.push(0x00);
            }
            answer = Some(self.device.transceive(&apdu)?);
        }
        let (mut out, mut high, mut low) = split_status(answer.ok_or(HwError::Protocol)?)?;
        // 0x61 means more of the answer waits, and is fetched with GET RESPONSE. A key that
        // never stops saying so is not followed past any real answer's size.
        while high == 0x61 {
            if out.len() > MAX_NFC_ANSWER {
                return Err(HwError::Protocol);
            }
            let (more, h, l) = split_status(self.device.transceive(&[0x80, 0xc0, 0x00, 0x00, low])?)?;
            out.extend_from_slice(&more);
            (high, low) = (h, l);
        }
        if (high, low) != (0x90, 0x00) {
            return Err(HwError::Transport);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    #[derive(Default)]
    struct Loop {
        written: Vec<[u8; 64]>,
        incoming: VecDeque<[u8; 64]>,
    }

    impl HidDevice for Loop {
        fn write(&mut self, report: &[u8; 64]) -> Result<()> {
            self.written.push(*report);
            Ok(())
        }
        fn read(&mut self) -> Result<[u8; 64]> {
            self.incoming.pop_front().ok_or(HwError::Transport)
        }
    }

    #[test]
    fn long_messages_split_into_reports_and_join_again() {
        let nonce = [1, 2, 3, 4, 5, 6, 7, 8];
        let mut device = Loop::default();
        let mut init = nonce.to_vec();
        init.extend_from_slice(&[0, 0, 0, 9]);
        init.extend_from_slice(&[0; 5]);
        // A keepalive and another program's traffic on the way are skipped.
        device.incoming.extend(hid_frames(0x1234, CMD_INIT, &[9; 17]).unwrap());
        device.incoming.extend(hid_frames(BROADCAST, CMD_INIT, &init).unwrap());
        let payload: Vec<u8> = (0..200u32).map(|n| n as u8).collect();
        device.incoming.extend(hid_frames(9, CMD_KEEPALIVE, &[1]).unwrap());
        device.incoming.extend(hid_frames(9, CMD_CBOR, &payload).unwrap());
        let mut hid = CtapHid::open(device, nonce).unwrap();
        assert_eq!(hid.channel, 9);
        let request = vec![7u8; 100];
        assert_eq!(hid.exchange(&request).unwrap(), payload);
        let sent = &hid.device.written[1..];
        assert_eq!(sent.len(), 2, "57 bytes in the first report, the rest in the second");
        assert_eq!(&sent[0][..7], &[0, 0, 0, 9, CMD_CBOR, 0, 100]);
        assert_eq!(&sent[1][..5], &[0, 0, 0, 9, 0]);
    }

    #[test]
    fn another_programs_traffic_is_skipped() {
        let nonce = [1, 2, 3, 4, 5, 6, 7, 8];
        let mut device = Loop::default();
        // Another program's INIT answer, with its own nonce, comes first on the broadcast channel.
        let mut theirs = vec![9u8; 8];
        theirs.extend_from_slice(&[0, 0, 0, 5]);
        theirs.extend_from_slice(&[0; 5]);
        device.incoming.extend(hid_frames(BROADCAST, CMD_INIT, &theirs).unwrap());
        let mut ours = nonce.to_vec();
        ours.extend_from_slice(&[0, 0, 0, 9]);
        ours.extend_from_slice(&[0; 5]);
        device.incoming.extend(hid_frames(BROADCAST, CMD_INIT, &ours).unwrap());
        // And a report on that other channel lands between the parts of our answer.
        let payload = vec![3u8; 100];
        let mut frames = hid_frames(9, CMD_CBOR, &payload).unwrap();
        frames.insert(1, hid_frames(5, CMD_CBOR, &[7; 70]).unwrap()[1]);
        device.incoming.extend(frames);
        let mut hid = CtapHid::open(device, nonce).unwrap();
        assert_eq!(hid.channel, 9, "the channel from the answer with our nonce");
        assert_eq!(hid.exchange(&[1]).unwrap(), payload);
    }

    #[test]
    fn an_endless_nfc_answer_is_cut_off() {
        let mut answers = VecDeque::from([vec![0x90, 0x00]]);
        for _ in 0..400 {
            let mut more = vec![0u8; 256];
            more.extend_from_slice(&[0x61, 0x00]);
            answers.push_back(more);
        }
        let mut nfc = CtapNfc::open(Tag { sent: Vec::new(), answers }).unwrap();
        assert_eq!(nfc.exchange(&[4]).err(), Some(HwError::Protocol));
    }

    #[test]
    fn hostile_lengths_are_refused() {
        let mut device = Loop::default();
        let mut frame = [0u8; 64];
        frame[..4].copy_from_slice(&BROADCAST.to_be_bytes());
        frame[4] = CMD_INIT;
        frame[5] = 0xff;
        frame[6] = 0xff;
        device.incoming.push_back(frame);
        assert_eq!(CtapHid::open(device, [0; 8]).err(), Some(HwError::Protocol));
    }

    struct Tag {
        sent: Vec<Vec<u8>>,
        answers: VecDeque<Vec<u8>>,
    }

    impl ApduDevice for Tag {
        fn transceive(&mut self, apdu: &[u8]) -> Result<Vec<u8>> {
            self.sent.push(apdu.to_vec());
            self.answers.pop_front().ok_or(HwError::Transport)
        }
    }

    #[test]
    fn nfc_chains_commands_and_fetches_long_answers() {
        let answers = VecDeque::from([
            vec![0x90, 0x00],
            vec![0x90, 0x00],
            vec![1, 2, 3, 0x61, 0x02],
            vec![4, 5, 0x90, 0x00],
        ]);
        let tag = Tag { sent: Vec::new(), answers };
        let mut nfc = CtapNfc::open(tag).unwrap();
        let request = vec![0xaa; 300];
        assert_eq!(nfc.exchange(&request).unwrap(), vec![1, 2, 3, 4, 5]);
        let sent = &nfc.device.sent;
        assert_eq!(&sent[0][..4], &[0x00, 0xa4, 0x04, 0x00]);
        assert_eq!(sent[1][0], 0x90, "more follows");
        assert_eq!(sent[1][4], 250);
        assert_eq!(sent[2][0], 0x80, "last piece");
        assert_eq!(sent[2][4], 50);
        assert_eq!(sent[3][..2], [0x80, 0xc0]);
    }

    #[test]
    fn a_failed_select_means_no_fido_applet() {
        let tag = Tag { sent: Vec::new(), answers: VecDeque::from([vec![0x6a, 0x82]]) };
        assert_eq!(CtapNfc::open(tag).err(), Some(HwError::Unsupported));
    }
}
