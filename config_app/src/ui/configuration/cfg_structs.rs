use std::fmt::Display;

use eframe::egui::{include_image, ImageSource};
use packed_struct::prelude::{PackedStruct, PrimitiveEnum_u8};
use strum_macros::EnumIter;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, PackedStruct)]
#[packed_struct(endian="lsb")]
pub struct TcmCoreConfig {
    pub is_large_nag: u8,
    pub diff_ratio: u16,
    pub wheel_circumference: u16,
    pub is_four_matic: u8,
    pub transfer_case_high_ratio: u16,
    pub transfer_case_low_ratio: u16,
    #[packed_field(size_bytes="1", ty="enum")]
    pub default_profile: DefaultProfile,
    pub red_line_dieselrpm: u16,
    pub red_line_petrolrpm: u16,
    #[packed_field(size_bytes="1", ty="enum")]
    pub engine_type: EngineType,
    #[packed_field(size_bytes="1", ty="enum")]
    pub egs_can_type: EgsCanType,
    // Only for V1,2 and newer PCBs
    #[packed_field(size_bytes="1", ty="enum")]
    pub shifter_style: ShifterStyle,
    // Only for V1.3 and newer PCBs
    #[packed_field(size_bytes="1", ty="enum")]
    pub io_0_usage: IOPinConfig,
    pub input_sensor_pulses_per_rev: u8,
    pub output_pulse_width_per_kmh: u8,
    #[packed_field(size_bytes="1", ty="enum")]
    pub mosfet_purpose: MosfetPurpose,
    // Only for HFM CAN mode
    pub throttle_max_open_angle: u8,
    // Value here is 1000x value ECU uses (Like diff ratio)
    pub c_eng: u16,
    // Value here is 10x value ECU uses
    pub engine_drag_torque: u16,
    #[packed_field(size_bytes="1")]
    pub jeep_chrysler: bool
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, PrimitiveEnum_u8, EnumIter)]
pub enum EgsCanType {
    UNKNOWN = 0,
    EGS51 = 1,
    EGS52 = 2,
    EGS53 = 3,
    HFM = 4,
    CUSTOM_ECU = 5
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, PrimitiveEnum_u8, EnumIter)]
pub enum ShifterStyle {
    EWM_CAN = 0,
    TRRS = 1,
    SLR_MCLAREN = 2,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, PrimitiveEnum_u8, EnumIter)]
pub enum IOPinConfig {
    NotConnected = 0,
    Input = 1,
    Output = 2,
    TCCMod13 = 3
}

impl ToString for IOPinConfig {
    fn to_string(&self) -> String {
        match self {
            IOPinConfig::NotConnected => "Not connected",
            IOPinConfig::Input => "Speed sensor input",
            IOPinConfig::Output => "Speedometer pulse output",
            IOPinConfig::TCCMod13 => "TCC Zener cutoff (With mod PCB)",
        }.into()
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, PrimitiveEnum_u8, EnumIter)]
pub enum MosfetPurpose {
    NotConnected = 0,
    TorqueCutTrigger = 1,
    B3BrakeSolenoid = 2,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, PackedStruct)]
pub struct TcmEfuseConfig {
    #[packed_field(size_bytes="1", ty="enum")]
    pub board_ver: BoardType,
    pub manf_day: u8,
    pub manf_week: u8,
    pub manf_month: u8,
    pub manf_year: u8,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, PrimitiveEnum_u8, EnumIter)]
pub enum DefaultProfile {
    Standard = 0,
    Comfort = 1,
    Winter = 2,
    Agility = 3,
    Manual = 4,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, PrimitiveEnum_u8, EnumIter)]
pub enum EngineType {
    Diesel,
    Petrol,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, PrimitiveEnum_u8, EnumIter)]
pub enum BoardType {
    Unknown = 0,
    V11 = 1,
    V12 = 2,
    V13 = 3,
    V14 = 4,
    V14HGS = 0xF4
}

impl BoardType {
    pub fn image_source(&self) -> Option<ImageSource> {
        match self {
            BoardType::Unknown => None,
            BoardType::V11 => Some(include_image!("../../../res/pcb_11.jpg")),
            BoardType::V12 => Some(include_image!("../../../res/pcb_12.jpg")),
            BoardType::V13 => Some(include_image!("../../../res/pcb_13.jpg")),
            BoardType::V14 => Some(include_image!("../../../res/pcb_13.jpg")),
            BoardType::V14HGS => Some(include_image!("../../../res/pcb_13.jpg")),
        }   
    }
}

impl Display for BoardType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BoardType::Unknown => write!(f, "Unknown"),
            BoardType::V11 => write!(f, "V1.1 (12/12/21)"),
            BoardType::V12 => write!(f, "V1.2 (07/07/22)"),
            BoardType::V13 => write!(f, "V1.3 (12/12/22)"),
            BoardType::V14 => write!(f, "V1.4 (13/05/24)"),
            BoardType::V14HGS => write!(f, "V1.4 (HGS) (13/05/24)"),
        }
    }
}

impl Into<String> for BoardType {
    fn into(self) -> String {
        format!("{}", self)
    }
}
