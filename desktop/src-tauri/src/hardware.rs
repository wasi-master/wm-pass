//! Security keys by USB. A key's answer goes back to the page as a one-time value, the same as
//! the phone's, and joins the password in the session.

use serde_json::Value;
use wmpass_hwkey::desktop::{open_fido, open_otp};
use wmpass_hwkey::{HwError, otp};
use wmpass_session::{FactorView, Reply, SessionError, factor_of, factor_view};
use zeroize::Zeroizing;

fn failure(error: &HwError) -> SessionError {
    let code = match error {
        HwError::WrongKey => "wrong_key",
        HwError::NotConfirmed => "not_confirmed",
        HwError::Unsupported => "unsupported",
        HwError::PinRequired => "pin_required",
        HwError::PinBlocked => "pin_blocked",
        _ => "hardware",
    };
    let message = if matches!(error, HwError::Transport) {
        if cfg!(windows) {
            "No key answered. Windows lets only administrators reach FIDO2 keys directly.".to_owned()
        } else {
            "No key answered. Check that it is plugged in.".to_owned()
        }
    } else {
        error.to_string()
    };
    SessionError { code, message }
}

/// Enrols a key when no factor is given, then asks it for its secret.
pub fn tap(kind: &str, factor: &Value) -> Result<Reply, SessionError> {
    let wanted: Option<FactorView> = if factor.is_null() {
        None
    } else {
        Some(serde_json::from_value(factor.clone()).map_err(|_| SessionError { code: "invalid", message: "bad factor".to_owned() })?)
    };
    let existing = wanted.as_ref().map(factor_of).transpose()?;
    let run = || -> Result<_, HwError> {
        if kind == "fido2" {
            let mut transport = open_fido()?;
            let chosen = match existing.clone() {
                Some(chosen) => chosen,
                None => wmpass_hwkey::ctap::enroll(&mut transport)?,
            };
            let output = wmpass_hwkey::ctap::derive(&mut transport, &chosen)?;
            Ok((chosen, output))
        } else {
            let chosen = match existing.clone() {
                Some(chosen) => chosen,
                None => otp::enroll(2)?,
            };
            let mut device = open_otp()?;
            let output = otp::derive_usb(&mut device, &chosen, 150)?;
            Ok((chosen, output))
        }
    };
    let (chosen, output) = run().map_err(|e| failure(&e))?;
    Ok(Reply {
        json: serde_json::to_string(&factor_view(&chosen)).unwrap_or_default(),
        secret: Some(Zeroizing::new(output.to_vec())),
        blobs: Vec::new(),
    })
}
