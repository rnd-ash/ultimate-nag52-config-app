use bitflags::{bitflags, Flags};
use ecu_diagnostics::{DiagServerResult, DiagError, kwp2000::KwpSessionType};

use super::Nag52Diag;

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct TcuDeviceMode: u16 {
        const NORMAL = 1 << 0;
        // Bit 1 ?
        const ROLLER = 1 << 2;
        const SLAVE = 1 << 3;
        const TEMPORARY_ERROR = 1 << 4;
        // Bit 5?
        const ERROR1 = 1 << 6;
        // Bit 7?
        // Bit 8?
        // Bit 9?
        // Bit 10?
        // Bit 11?
        // Bit 12?
        // Bit 13?
        // Bit 14?
        const CANLOGGER = 1 << 15;
    }
}

impl Nag52Diag {
    pub fn read_device_mode(&self) -> DiagServerResult<TcuDeviceMode> {
        let res = self.with_kwp(|kwp| {
            kwp.send_byte_array_with_response(&[0x30, 0x10, 0x01])
        })?;
        if res.len() != 5 {
            Err(DiagError::InvalidResponseLength)
        } else {
            let x: u16 = u16::from_be_bytes(res[3..].try_into().unwrap());
            Ok(TcuDeviceMode::from_bits_retain(x))
        }
    }

    pub fn set_device_mode(&self, mode: TcuDeviceMode, store_in_eeprom: bool) -> DiagServerResult<()> {
        let _ = self.with_kwp(|kwp| {
            let x = mode.bits();
            kwp.send_byte_array_with_response(&[
                0x30, 
                0x10, 
                if store_in_eeprom {0x08} else {0x07},
                ((x >> 8) & 0xFF) as u8,
                ((x >> 0) & 0xFF) as u8
            ])
        })?;
        Ok(())
    }

    pub fn return_mode_control_to_ecu(&self) -> DiagServerResult<()> {
        let _ = self.with_kwp(|kwp| {
            kwp.send_byte_array_with_response(&[0x30, 0x10, 0x00])
        })?;
        Ok(())
    }
}