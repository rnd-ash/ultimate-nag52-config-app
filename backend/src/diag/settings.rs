use std::ptr::slice_from_raw_parts;

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use strum::IntoEnumIterator;
use strum_macros::EnumIter;

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

fn enum_to_str_list<T>(x: Vec<T>) -> Vec<String>
where T: Serialize + DeserializeOwned {
    let mut res = vec![];

    for entry in x {
        let e = serde_json::to_string(&entry).unwrap();
        res.push(e.replace("\"", ""));
    }
    res
}


pub trait TcuSettings: Copy + Clone + Serialize + DeserializeOwned
where
{
    fn wiki_url() -> Option<&'static str>;
    fn setting_name() -> &'static str;
    fn get_revision_name() -> &'static str;
    fn get_scn_id() -> u8;
    fn effect_immediate() -> bool {
        true
    }
    fn get_enum_entries(key: &str) -> Option<Vec<String>> {
        None
    }
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
    adapt_enable: bool,
    enable_d1: bool,
    enable_d2: bool,
    enable_d3: bool,
    enable_d4: bool,
    enable_d5: bool,
    prefill_pressure: u16,
    min_locking_rpm: u16,
    adapt_test_interval_ms: u16,
    tcc_stall_speed: u16,
    min_torque_adapt: u16,
    max_torque_adapt: u16,
    prefill_min_engine_rpm: u16,
    max_slip_max_adapt_trq: u16,
    min_slip_max_adapt_trq: u16,
    max_slip_min_adapt_trq: u16,
    min_slip_min_adapt_trq: u16,
    pressure_increase_step: u8,
    adapt_pressure_step: u8,
    pressure_multiplier_output_rpm: LinearInterpSettings,
    sailing_mode_active_rpm: u16,
    force_lock_min_output_rpm: u16,
    locking_pedal_pos_max: u8
}

impl TcuSettings for TccSettings {
    fn wiki_url() -> Option<&'static str> {
        Some("https://docs.ultimate-nag52.net/en/gettingstarted/configuration/settings/tcc#revision-a2-240423")
    }

    fn setting_name() -> &'static str {
        "TCC Settings"
    }

    fn get_revision_name() -> &'static str {
        "A3 (14/06/23)"
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
    min_upshift_end_rpm: u16,
    f_shown_if_flare: bool,
    debug_show_up_down_arrows_in_r: bool,
    torque_reduction_factor_input_torque: LinearInterpSettings,
    torque_reduction_factor_shift_speed: LinearInterpSettings,
    stationary_shift_hold_time: u16,
    shift_timeout_pulling: u16,
    shift_timeout_coasting: u16,
    smooth_shifting_spc_multi_too_slow: f32,
    smooth_shifting_spc_multi_too_fast: f32,
    upshift_trq_max_reduction_at: u16,
    downshift_trq_max_reduction_at: u16,
    spc_multi_overlap_shift_speed: LinearInterpSettings,
    spc_multi_overlap_zero_trq: f32,
    spc_multi_overlap_max_trq: f32,
    garage_shift_max_timeout_engine: u16,
}

impl TcuSettings for SbsSettings {
    fn wiki_url() -> Option<&'static str> {
        Some("https://docs.ultimate-nag52.net/en/gettingstarted/configuration/settings/ShiftProgramBasicSettings#revision-a0-260423")
    }

    fn setting_name() -> &'static str {
        "Shift program basic Settings"
    }

    fn get_revision_name() -> &'static str {
        "A2 (11/07/23)"
    }

    fn get_scn_id() -> u8 {
        0x03
    }
}

#[derive(Default, Debug, Copy, Clone, PartialEq, PartialOrd, Serialize, Deserialize)]
#[repr(C, packed)]
pub struct GearboxInfo {
    max_torque: u16,
    ratio_1: f32,
    ratio_2: f32,
    ratio_3: f32,
    ratio_4: f32,
    ratio_5: f32,
    ratio_r1: f32,
    ratio_r2: f32,
    power_loss_1: u8,
    power_loss_2: u8,
    power_loss_3: u8,
    power_loss_4: u8,
    power_loss_5: u8,
    power_loss_r1: u8,
    power_loss_r2: u8,
}

#[derive(Default, Debug, Copy, Clone, PartialEq, PartialOrd, Serialize, Deserialize)]
#[repr(C, packed)]
pub struct NagSettings {
    max_drift_1: u8,
    max_drift_2: u8,
    max_drift_3: u8,
    max_drift_4: u8,
    max_drift_5: u8,
    max_drift_r1: u8,
    max_drift_r2: u8,
    small_nag: GearboxInfo,
    large_nag: GearboxInfo,
}

impl TcuSettings for NagSettings {
    fn wiki_url() -> Option<&'static str> {
        None
    }

    fn setting_name() -> &'static str {
        "NAG Settings"
    }

    fn get_revision_name() -> &'static str {
        "A0 (28/04/23)"
    }

    fn get_scn_id() -> u8 {
        0x04
    }

    fn effect_immediate() -> bool {
        false
    }
}

#[derive(Default, Debug, Copy, Clone, PartialEq, PartialOrd, Serialize, Deserialize)]
#[repr(C, packed)]
pub struct PrmSettings {
    max_spc_pressure: u16,
    max_mpc_pressure: u16,
    max_line_pressure: u16,
    engine_rpm_pressure_multi: LinearInterpSettings,
    k1_pressure_multi: f32,
    shift_solenoid_pwm_reduction_time: u16,
}

impl TcuSettings for PrmSettings {
    fn wiki_url() -> Option<&'static str> {
        Some("https://docs.ultimate-nag52.net/en/gettingstarted/configuration/settings/prm#revision-a1-140623")
    }

    fn setting_name() -> &'static str {
        "Pressure Manager Settings"
    }

    fn get_revision_name() -> &'static str {
        "A1 (14/06/23)"
    }

    fn get_scn_id() -> u8 {
        0x05
    }

    fn effect_immediate() -> bool {
        true
    }
}

#[derive(Default, Debug, Copy, Clone, PartialEq, PartialOrd, Serialize, Deserialize)]
#[repr(C, packed)]
pub struct AdpSettings {
    min_atf_temp: i16,
    max_atf_temp: i16,
    min_input_rpm: u16,
    max_input_rpm: u16,
    prefill_adapt_k1: bool,
    prefill_adapt_k2: bool,
    prefill_adapt_k3: bool,
    prefill_adapt_b1: bool,
    prefill_adapt_b2: bool,
    prefill_max_pressure_delta: u16,
    prefill_max_time_delta: u16,
}

impl TcuSettings for AdpSettings {
    fn wiki_url() -> Option<&'static str> {
        Some("https://docs.ultimate-nag52.net/en/gettingstarted/configuration/settings/adp#revision-a1-140623")
    }

    fn setting_name() -> &'static str {
        "Adaptation Manager Settings"
    }

    fn get_revision_name() -> &'static str {
        "A1 (14/06/23)"
    }

    fn get_scn_id() -> u8 {
        0x06
    }

    fn effect_immediate() -> bool {
        true
    }
}

#[derive(EnumIter, Debug, Copy, Clone, PartialEq, PartialOrd, Serialize, Deserialize)]
#[repr(u8)]
enum EwmSelectorType {
    None = 0,
    Button = 1,
    Switch = 2
}

impl Default for EwmSelectorType {
    fn default() -> Self {
        Self::Button
    }
}


#[derive(EnumIter, Debug, Copy, Clone, PartialEq, PartialOrd, Serialize, Deserialize)]
#[repr(u8)]

enum AutoProfile {
    Sport = 0,
    Comfort = 1,
    Agility = 2,
    Winter = 3
}

impl Default for AutoProfile {
    fn default() -> Self {
        Self::Sport
    }
}

#[derive(Default, Debug, Copy, Clone, PartialEq, PartialOrd, Serialize, Deserialize)]
#[repr(C, packed)]
pub struct EtsSettings {
    trrs_has_profile_selector: bool,
    ewm_selector_type: EwmSelectorType,
    profile_idx_top: AutoProfile,
    profile_idx_bottom: AutoProfile
}

impl TcuSettings for EtsSettings {
    fn wiki_url() -> Option<&'static str> {
        Some("https://docs.ultimate-nag52.net/en/gettingstarted/configuration/settings/ets#revision-a1-200623")
    }

    fn setting_name() -> &'static str {
        "Electronic gear selector settings"
    }

    fn get_revision_name() -> &'static str {
        "A1 (20/06/23)"
    }

    fn get_scn_id() -> u8 {
        0x07
    }

    fn effect_immediate() -> bool {
        true
    }

    fn get_enum_entries(key: &str) -> Option<Vec<String>> {
        match key {
            "ewm_selector_type" => Some(enum_to_str_list(EwmSelectorType::iter().collect())),
            "profile_idx_top" => Some(enum_to_str_list(AutoProfile::iter().collect())),
            "profile_idx_buttom" => Some(enum_to_str_list(AutoProfile::iter().collect())),
            _ => None
        }
    }
}