use std::fmt::{Display};

use ecu_diagnostics::{DiagServerResult, bcd_decode_slice};

use super::{Nag52Diag};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EgsMode {
    EGS51,
    EGS52,
    EGS53,
    Unknown(u16),
}

impl From<u16> for EgsMode {
    fn from(diag_var_code: u16) -> Self {
        match diag_var_code {
            0x0251 => Self::EGS51,
            0x0252 => Self::EGS52,
            0x0253 => Self::EGS53,
            _ => Self::Unknown(diag_var_code),
        }
    }
}

impl Display for EgsMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EgsMode::EGS51 => f.write_str("EGS51"),
            EgsMode::EGS52 => f.write_str("EGS52"),
            EgsMode::EGS53 => f.write_str("EGS53"),
            EgsMode::Unknown(x) => f.write_fmt(format_args!("Unknown(0x{:08X})", x)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PCBVersion {
    OnePointOne,
    OnePointTwo,
    OnePointThree,
    Unknown,
}

impl PCBVersion {
    fn from_date(w: u32, y: u32) -> Self {
        if w == 49 && y == 21 {
            Self::OnePointOne
        } else if w == 27 && y == 22 {
            Self::OnePointTwo
        } else if w == 49 && y == 22 {
            Self::OnePointThree
        } else {
            Self::Unknown
        }
    }
}

impl Display for PCBVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            PCBVersion::OnePointOne => "V1.1",
            PCBVersion::OnePointTwo => "V1.2",
            PCBVersion::OnePointThree => "V1.3",
            PCBVersion::Unknown => "V_NDEF",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct IdentData {
    pub egs_mode: EgsMode,
    pub board_ver: PCBVersion,

    pub manf_day: u32,
    pub manf_month: u32,
    pub manf_year: u32,

    pub hw_week: u32,
    pub hw_year: u32,

    pub sw_week: u32,
    pub sw_year: u32,
}

fn bcd_decode_to_int(u: u8) -> u32 {
    10 * (u as u32 / 16) + (u as u32 % 16)
}

impl Nag52Diag {
    pub fn query_ecu_data(&mut self) -> DiagServerResult<IdentData> {
        self.with_kwp(|k| {
            let ident = k.kwp_read_daimler_identification()?;
            Ok(IdentData {
                egs_mode: EgsMode::from(ident.diag_info.get_info_id()),
                board_ver: PCBVersion::from_date(
                    bcd_decode_to_int(ident.ecu_hw_build_week),
                    bcd_decode_to_int(ident.ecu_hw_build_year),
                ),
                manf_day: bcd_decode_to_int(ident.ecu_production_day),
                manf_month: bcd_decode_to_int(ident.ecu_production_month),
                manf_year: bcd_decode_to_int(ident.ecu_production_year),
                hw_week: bcd_decode_to_int(ident.ecu_hw_build_week),
                hw_year: bcd_decode_to_int(ident.ecu_hw_build_year),
                sw_week: bcd_decode_to_int(ident.ecu_sw_build_week),
                sw_year: bcd_decode_to_int(ident.ecu_sw_build_year),
            })
        })
    }

    pub fn get_ecu_sn(&mut self) -> DiagServerResult<String> {
        self.with_kwp(|k| {
            Ok(String::from_utf8(k.kwp_read_ecu_serial_number()?).unwrap())
        })
    }
}
