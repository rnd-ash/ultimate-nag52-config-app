use std::ptr::slice_from_raw_parts;

use serde::{Deserialize, Serialize, de::DeserializeOwned};

pub type UnpackResult<T> = std::result::Result<T, UnPackError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum UnPackError {
    WrongId { wanted: u8, real: u8 },
    InvalidLen { wanted: usize, len: usize },
}

impl ToString for UnPackError {
    fn to_string(&self) -> String {
        match self {
            UnPackError::WrongId { wanted, real } => format!("Wrong setting ID. Wanted 0x{:02X?}, got 0x{:02X?}", wanted, real),
            UnPackError::InvalidLen { wanted, len } => format!("Wrong response length. Wanted {} bytes, got {} bytes. Maybe a config app/firmware mismatch?", wanted, len),
        }
    }
}

pub fn unpack_settings<T>(settings_id: u8, raw: &[u8]) -> UnpackResult<T>
where
    T: Copy,
{
    if settings_id != raw[0] {
        Err(UnPackError::WrongId {
            wanted: settings_id,
            real: raw[0],
        })
    } else if raw.len() - 1 != std::mem::size_of::<T>() {
        Err(UnPackError::InvalidLen {
            wanted: std::mem::size_of::<T>(),
            len: raw.len() - 1,
        })
    } else {
        let ptr: *const T = raw[1..].as_ptr() as *const T;
        Ok(unsafe { *ptr })
    }
}

pub fn pack_settings<T>(settings_id: u8, settings: T) -> Vec<u8>
where
    T: Copy,
{
    let mut ret = vec![settings_id];
    let ptr = slice_from_raw_parts(
        (&settings as *const T) as *const u8,
        std::mem::size_of::<T>(),
    );
    ret.extend_from_slice(unsafe { &*ptr });
    ret
}

pub trait TcuSettings: Copy + Clone + Serialize + DeserializeOwned
where
    Self: Sized,
{
    fn wiki_url() -> Option<&'static str>;
    fn setting_name() -> &'static str;
    fn get_revision_name() -> &'static str;
    fn get_scn_id() -> u8;
}

#[derive(Default, Debug, Copy, Clone, PartialEq, PartialOrd, Serialize, Deserialize)]
#[repr(C, packed)]
pub struct LinearInterpSettings {
    pub new_min: f32,
    pub new_max: f32,
    pub raw_min: f32,
    pub raw_max: f32,
}

impl LinearInterpSettings {
    // Copied from TCU source lib/core/tcu_maths.cpp - Function scale_number()
    pub fn calc_with_value(&self, input: f32) -> f32 {
        let raw_limited = self.raw_min.max(input.min(self.raw_max));
        return (((self.new_max - self.new_min) * (raw_limited - self.raw_min))
            / (self.raw_max - self.raw_min))
            + self.new_min;
    }
}

#[derive(Default, Debug, Copy, Clone, PartialEq, PartialOrd, Serialize, Deserialize)]
#[repr(C, packed)]
pub struct TccSettings {
    pub adapt_enable: bool,
    pub enable_d1: bool,
    pub enable_d2: bool,
    pub enable_d3: bool,
    pub enable_d4: bool,
    pub enable_d5: bool,
    pub prefill_pressure: u16,
    pub lock_rpm_threshold: u16,
    pub min_locking_rpm: u16,
    pub adjust_interval_ms: u16,
    pub tcc_stall_speed: u16,
    pub min_torque_adapt: u16,
    pub max_torque_adapt: u16,
    pub prefill_min_engine_rpm: u16,
    pub base_pressure_offset_start_ramp: u16,
    pub pressure_increase_ramp_settings: LinearInterpSettings,
    pub adapt_pressure_inc: u8,
    pub adapt_lock_detect_time: u16,
    pub pulling_slip_rpm_low_threshold: u16,
    pub pulling_slip_rpm_high_threhold: u16,
    pub reaction_torque_multiplier: f32,
    pub trq_consider_coasting: u16,
    pub load_dampening: LinearInterpSettings,
    pub pressure_multiplier_output_rpm: LinearInterpSettings,
    pub max_allowed_bite_pressure: u16,
    pub max_allowed_pressure_longterm: u16,
}

impl TcuSettings for TccSettings {
    fn wiki_url() -> Option<&'static str> {
        Some("https://docs.ultimate-nag52.net/en/gettingstarted/configuration/settings/tcc#revision-a2-240423")
    }

    fn setting_name() -> &'static str {
        "TCC Settings"
    }

    fn get_revision_name() -> &'static str {
        "A2 (24/04/23)"
    }

    fn get_scn_id() -> u8 {
        0x01
    }
}

#[derive(Default, Debug, Copy, Clone, PartialEq, PartialOrd, Serialize, Deserialize)]
#[repr(C, packed)]
pub struct SolSettings {
    min_batt_power_on_test: u16,
    current_threshold_error: u16,
    cc_vref_solenoid: u16,
    cc_temp_coefficient_wires: f32,
    cc_reference_resistance: f32,
    cc_reference_temp: f32,
    cc_max_adjust_per_step: f32,
}

impl TcuSettings for SolSettings {
    fn wiki_url() -> Option<&'static str> {
        Some("https://docs.ultimate-nag52.net/en/gettingstarted/configuration/settings/SolenoidControlProgramSettings#revision-a0-260423")
    }

    fn setting_name() -> &'static str {
        "Solenoid subsystem Settings"
    }

    fn get_revision_name() -> &'static str {
        "A0 (26/04/23)"
    }

    fn get_scn_id() -> u8 {
        0x02
    }
}

#[derive(Default, Debug, Copy, Clone, PartialEq, PartialOrd, Serialize, Deserialize)]
#[repr(C, packed)]
pub  struct SbsSettings {
    shift_solenoid_pwm_reduction_time: u16,
    delta_rpm_flare_detect: u16,
    f_shown_if_flare: bool,
    torque_request_upshift: bool,
    torque_request_downshift: bool,
    upshift_use_driver_torque_as_input: bool,
    downshift_use_driver_torque_as_input: bool,
    torque_request_downramp_percent: u16,
    torque_request_hold_percent: u16,
    torque_reduction_factor_input_torque: LinearInterpSettings,
    torque_reduction_factor_shift_speed: LinearInterpSettings,
    min_spc_delta_mpc: u16,
    stationary_shift_hold_time: u16,
    shift_timeout_pulling: u16,
    shift_timeout_coasting: u16,
}

impl TcuSettings for SbsSettings {
    fn wiki_url() -> Option<&'static str> {
        Some("https://docs.ultimate-nag52.net/en/gettingstarted/configuration/settings/ShiftProgramBasicSettings#revision-a0-260423")
    }

    fn setting_name() -> &'static str {
        "Shift program basic Settings"
    }

    fn get_revision_name() -> &'static str {
        "A0 (26/04/23)"
    }

    fn get_scn_id() -> u8 {
        0x03
    }
}
