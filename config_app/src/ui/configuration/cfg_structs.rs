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

impl Default for TcmCoreConfig {
    fn default() -> Self {
        Self { 
            is_large_nag: 0, 
            diff_ratio: 1000, 
            wheel_circumference: 0, 
            is_four_matic: 0, 
            transfer_case_high_ratio: Default::default(), 
            transfer_case_low_ratio: Default::default(), 
            default_profile: DefaultProfile::Standard, 
            red_line_dieselrpm: 4500, 
            red_line_petrolrpm: 6000, 
            engine_type: EngineType::Petrol, 
            egs_can_type: EgsCanType::Unknown, 
            shifter_style: ShifterStyle::EwmCan, 
            io_0_usage: IOPinConfig::NotConnected, 
            input_sensor_pulses_per_rev: 0, 
            output_pulse_width_per_kmh: 0, 
            mosfet_purpose: MosfetPurpose::NotConnected, 
            throttle_max_open_angle: 89, 
            c_eng: 0, 
            engine_drag_torque: 400, 
            jeep_chrysler: false
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, PrimitiveEnum_u8, EnumIter)]
pub enum EgsCanType {
    Unknown = 0,
    Egs51 = 1,
    Egs52 = 2,
    Egs53 = 3,
    Hfm = 4,
    CustomEcu = 5
}

impl Display for EgsCanType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EgsCanType::Unknown => f.write_str("Unknown"),
            EgsCanType::Egs51 => f.write_str("EGS51"),
            EgsCanType::Egs52 => f.write_str("EGS52"),
            EgsCanType::Egs53 => f.write_str("EGS53"),
            EgsCanType::Hfm => f.write_str("HFM"),
            EgsCanType::CustomEcu => f.write_str("Custom ECU"),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, PrimitiveEnum_u8, EnumIter)]
pub enum ShifterStyle {
    EwmCan = 0,
    TRRS = 1,
    Slr = 2,
}

impl Display for ShifterStyle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShifterStyle::EwmCan => f.write_str("Tiptronic (+/-)"),
            ShifterStyle::TRRS => f.write_str("TRRS (4321)"),
            ShifterStyle::Slr => f.write_str("SLR Mclaren"),
        }
    }
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

impl Display for MosfetPurpose {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MosfetPurpose::NotConnected => f.write_str("Not connected"),
            MosfetPurpose::TorqueCutTrigger => f.write_str("Torque cut"),
            MosfetPurpose::B3BrakeSolenoid => f.write_str("Trans brake"),
        }
    }
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
}

impl Display for DefaultProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DefaultProfile::Standard => f.write_str("Standard"),
            DefaultProfile::Comfort => f.write_str("Comfort"),
            DefaultProfile::Winter => f.write_str("Winter"),
            DefaultProfile::Agility => f.write_str("Agility"),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, PrimitiveEnum_u8, EnumIter)]
pub enum EngineType {
    Diesel,
    Petrol,
}

impl Display for EngineType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EngineType::Diesel => f.write_str("Diesel"),
            EngineType::Petrol => f.write_str("petrol"),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, PrimitiveEnum_u8, EnumIter)]
pub enum BoardType {
    Unknown = 0,
    V11 = 1,
    V12 = 2,
    V13 = 3,
}

impl BoardType {
    pub fn image_source(&'_ self) -> Option<Vec<ImageSource<'_>>> {
        match self {
            BoardType::Unknown => None,
            BoardType::V11 => Some(vec![include_image!("../../../res/pcb_11.jpg")]),
            BoardType::V12 => Some(vec![include_image!("../../../res/pcb_12.jpg")]),
            BoardType::V13 => Some(vec![include_image!("../../../res/pcb_13.jpg"), include_image!("../../../res/pcb_13b.png")]),
        }   
    }
}

impl Display for BoardType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BoardType::Unknown => write!(f, "Unknown"),
            BoardType::V11 => write!(f, "V1.1 (12/12/21)"),
            BoardType::V12 => write!(f, "V1.2 (07/07/22)"),
            BoardType::V13 => write!(f, "V1.3/B (12/12/22 | 23/05/26)"),
        }
    }
}
