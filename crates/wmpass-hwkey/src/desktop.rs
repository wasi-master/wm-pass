//! USB on desktop through hidapi. Windows reserves raw FIDO HID access for administrators, so
//! there FIDO2 needs a different path, and only YubiKey challenge-response works this way.

use std::time::Duration;

use hidapi::{HidApi, HidDevice as RawDevice};

use crate::otp::OtpHid;
use crate::transport::{CtapHid, HidDevice};
use crate::{HwError, Result};

const FIDO_USAGE_PAGE: u16 = 0xf1d0;
const YUBICO: u16 = 0x1050;
const READ_TIMEOUT_MS: i32 = 30_000;

/// A FIDO device.
#[derive(Debug)]
pub struct DesktopFido(RawDevice);

impl HidDevice for DesktopFido {
    fn write(&mut self, report: &[u8; 64]) -> Result<()> {
        let mut framed = [0u8; 65];
        framed[1..].copy_from_slice(report);
        self.0.write(&framed).map(|_| ()).map_err(|_| HwError::Transport)
    }

    fn read(&mut self) -> Result<[u8; 64]> {
        let mut report = [0u8; 64];
        match self.0.read_timeout(&mut report, READ_TIMEOUT_MS) {
            Ok(64) => Ok(report),
            _ => Err(HwError::Transport),
        }
    }
}

/// Opens the first FIDO key found.
pub fn open_fido() -> Result<CtapHid<DesktopFido>> {
    let api = HidApi::new().map_err(|_| HwError::Transport)?;
    let info = api.device_list().find(|device| device.usage_page() == FIDO_USAGE_PAGE).ok_or(HwError::Transport)?;
    let device = info.open_device(&api).map_err(|_| HwError::Transport)?;
    let nonce = wmpass_core::crypto::random_array().map_err(|_| HwError::Crypto)?;
    CtapHid::open(DesktopFido(device), nonce)
}

/// A YubiKey's OTP interface.
#[derive(Debug)]
pub struct DesktopOtp(RawDevice);

impl OtpHid for DesktopOtp {
    fn get_report(&mut self) -> Result<[u8; 8]> {
        let mut raw = [0u8; 9];
        self.0.get_feature_report(&mut raw).map_err(|_| HwError::Transport)?;
        let mut report = [0u8; 8];
        report.copy_from_slice(&raw[1..]);
        Ok(report)
    }

    fn set_report(&mut self, report: &[u8; 8]) -> Result<()> {
        let mut raw = [0u8; 9];
        raw[1..].copy_from_slice(report);
        self.0.send_feature_report(&raw).map_err(|_| HwError::Transport)
    }

    fn pause(&mut self) {
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Opens the first YubiKey found.
pub fn open_otp() -> Result<DesktopOtp> {
    let api = HidApi::new().map_err(|_| HwError::Transport)?;
    let info = api
        .device_list()
        .find(|device| device.vendor_id() == YUBICO && device.interface_number() == 0 && device.usage_page() != FIDO_USAGE_PAGE)
        .ok_or(HwError::Transport)?;
    Ok(DesktopOtp(info.open_device(&api).map_err(|_| HwError::Transport)?))
}
