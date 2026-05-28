use crate::ui::map_editor::Map;

use super::MapData;

#[derive(Debug, Copy, Clone, PartialEq, PartialOrd, Hash, Eq, Ord)]
#[repr(u8)]
pub enum MapType {
    UpshiftA = 0x01,
    UpshiftC = 0x02,
    UpshiftS = 0x03,
    DnshiftA = 0x04,
    DnshiftC = 0x05,
    DnshiftS = 0x06,

    TccPwm   = 0x09,
    FillTime =  0x0A,
    FillPressure= 0x0B,
    LowFillPressure = 0x0C,

    UpshiftOverlapA  = 0x10,
    DnshiftOverlapA  = 0x11,
    UpshiftOverlapS  = 0x12,
    DnshiftOverlapS  = 0x13,
    UpshiftOverlapC  = 0x14,
    DnshiftOverlapC  = 0x15,
    UpshiftOverlapW  = 0x16,
    DnshiftOverlapW  = 0x17,
    UpshiftOverlapM  = 0x18,
    DnshiftOverlapM  = 0x19,

    HfmTrqMap = 0x20,
    HfmMafMap = 0x21,
    HfmMaxMap = 0x22,

    TccAdaptSlipMap = 0xA0,
    TccAdaptLockMap = 0xA1,

    ShiftAdaptFillTMap   = 0xA2,
    ShiftAdaptFillPMap   = 0xA3,
    ShiftAdaptTrqApplMap = 0xA4,
    ShiftAdaptTrqFreeMap = 0xA5,

    TccRpmSlipMap = 0xB0
}

pub(crate) const MAP_ARRAY: &[MapData] = &[
    MapData::new(
        MapType::UpshiftA,
        "Upshift (A)",
        "%",
        "",
        "Pedal position (%)",
        "Gear shift",
        "Upshift RPM threshold",
        "RPM",
        None,
        Some(&["1->2", "2->3", "3->4", "4->5"]),
        false
        
    ),
    MapData::new(
        MapType::UpshiftC,
        "Upshift (C)",
        "%",
        "",
        "Pedal position (%)",
        "Gear shift",
        "Upshift RPM threshold",
        "RPM",
        None,
        Some(&["1->2", "2->3", "3->4", "4->5"]),
        false
        
    ),
    MapData::new(
        MapType::UpshiftS,
        "Upshift (S)",
        "%",
        "",
        "Pedal position (%)",
        "Gear shift",
        "Upshift RPM threshold",
        "RPM",
        None,
        Some(&["1->2", "2->3", "3->4", "4->5"]),
        false
        
    ),
    MapData::new(
        MapType::DnshiftA,
        "Downshift (A)",
        "%",
        "",
        "Pedal position (%)",
        "Gear shift",
        "Downshift RPM threshold",
        "RPM",
        None,
        Some(&["2->1", "3->2", "4->3", "5->4"]),
        false
        
    ),
    MapData::new(
        MapType::DnshiftC,
        "Downshift (C)",
        "%",
        "",
        "Pedal position (%)",
        "Gear shift",
        "Downshift RPM threshold",
        "RPM",
        None,
        Some(&["2->1", "3->2", "4->3", "5->4"]),
        false
        
    ),
    MapData::new(
        MapType::DnshiftS,
        "Downshift (S)",
        "%",
        "",
        "Pedal position (%)",
        "Gear shift",
        "Downshift RPM threshold",
        "RPM",
        None,
        Some(&["2->1", "3->2", "4->3", "5->4"]),
        false
        
    ),
    MapData::new(
        MapType::TccPwm,
        "TCC solenoid Pwm",
        "mBar",
        "C",
        "Converter pressure",
        "ATF Temperature",
        "Solenoid PWM duty (4096 = 100% on)",
        "/4096",
        None,
        None,
        false
        
    ),
    MapData::new(
        MapType::FillTime,
        "Clutch filling time",
        "C",
        "",
        "ATF Temperature",
        "Clutch",
        "filling time in millseconds",
        "ms",
        None,
        Some(&["K1", "K2", "K3", "B1", "B2"]),
        true
        
    ).with_help("Duration of stage 1 of the filling process (Priming the clutch)."),
    MapData::new(
        MapType::FillPressure,
        "Clutch filling pressure",
        "",
        "",
        "",
        "",
        "Filling pressure in millibar",
        "mBar",
        None,
        Some(&["K1", "K2", "K3", "B1", "B2", "B3"]),
        true
        
    ).with_help("Clutch filling pressure for stage 1 of the filling process (Priming the clutch)."),
    MapData::new(
        MapType::LowFillPressure,
        "Clutch low filling pressure",
        "",
        "",
        "",
        "",
        "Filling pressure in millibar",
        "mBar",
        None,
        Some(&["K1", "K2", "K3", "B1", "B2", "B3"]),
        true
        
    ).with_help("Clutch filling pressure for stage 2 of the filling process (Tolorance clearning)."),
    MapData::new(
        MapType::UpshiftOverlapA,
        "Upshift overlap time (Agility)",
        "%",
        "RPM",
        "",
        "Pedal position",
        "Input speed",
        "ms",
        None,
        None,
        false
        
    ),
    MapData::new(
        MapType::DnshiftOverlapA,
        "Downshift overlap time (Agility)",
        "%",
        "RPM",
        "",
        "Pedal position",
        "Input speed",
        "ms",
        None,
        None,
        false
        
    ),
    MapData::new(
        MapType::UpshiftOverlapS,
        "Upshift overlap time (Standard)",
        "%",
        "RPM",
        "",
        "Pedal position",
        "Input speed",
        "ms",
        None,
        None,
        false
        
    ),
    MapData::new(
        MapType::DnshiftOverlapS,
        "Downshift overlap time (Standard)",
        "%",
        "RPM",
        "",
        "Pedal position",
        "Input speed",
        "ms",
        None,
        None,
        false
        
    ),
    MapData::new(
        MapType::UpshiftOverlapC,
        "Upshift overlap time (Comfort)",
        "%",
        "RPM",
        "",
        "Pedal position",
        "Input speed",
        "ms",
        None,
        None,
        false
        
    ),
    MapData::new(
        MapType::DnshiftOverlapC,
        "Downshift overlap time (Comfort)",
        "%",
        "RPM",
        "",
        "Pedal position",
        "Input speed",
        "ms",
        None,
        None,
        false
        
    ),
    MapData::new(
        MapType::UpshiftOverlapW,
        "Upshift overlap time (Winter)",
        "%",
        "RPM",
        "",
        "Pedal position",
        "Input speed",
        "ms",
        None,
        None,
        false
        
    ),
    MapData::new(
        MapType::DnshiftOverlapW,
        "Downshift overlap time (Winter)",
        "%",
        "RPM",
        "",
        "Pedal position",
        "Input speed",
        "ms",
        None,
        None,
        false
        
    ),
    MapData::new(
        MapType::UpshiftOverlapM,
        "Upshift overlap time (Manual)",
        "%",
        "RPM",
        "",
        "Pedal position",
        "Input speed",
        "ms",
        None,
        None,
        false
        
    ),
    MapData::new(
        MapType::DnshiftOverlapM,
        "Downshift overlap time (Manual)",
        "%",
        "RPM",
        "",
        "Pedal position",
        "Input speed",
        "ms",
        None,
        None,
        false
        
    ),
    MapData::new(
        MapType::TccAdaptSlipMap,
        "Torque converter slip adapt map",
        "%",
        "",
        "TCC Load %",
        "",
        "Slipping pressure(mBar)",
        "mBar",
        None,
        Some(&["D1", "D2", "D3", "D4", "D5"]),
        false
        
    ),
    MapData::new(
        MapType::TccAdaptLockMap,
        "Torque converter lock adapt map",
        "%",
        "",
        "TCC Load %",
        "",
        "Locking pressure(mBar)",
        "mBar",
        None,
        Some(&["D1", "D2", "D3", "D4", "D5"]),
        false
    ),
    MapData::new(
        MapType::ShiftAdaptFillTMap,
        "Shift adaptation fill time map",
        "",
        "",
        "Gear change",
        "",
        "Filling cycles offset (20ms per cycle)",
        "cycle(s)",
        Some(&["1->2", "2->3", "3->4", "4->5", "2->1", "3->2", "4->3", "5->4"]),
        None,
        false
    ),
    MapData::new(
        MapType::ShiftAdaptFillPMap,
        "Shift adaptation fill pressure map",
        "",
        "",
        "Gear change",
        "",
        "Filling pressure offset",
        "mBar",
        Some(&["1->2", "2->3", "3->4", "4->5", "2->1", "3->2", "4->3", "5->4"]),
        None,
        false
    ),
    MapData::new(
        MapType::ShiftAdaptTrqApplMap,
        "Shift adaptation applying clutch torque offset map",
        "",
        "",
        "Gear change",
        "",
        "Torque offset",
        "Nm",
        Some(&["1->2", "2->3", "3->4", "4->5", "2->1", "3->2", "4->3", "5->4"]),
        None,
        false
    ),
    MapData::new(
        MapType::ShiftAdaptTrqFreeMap,
        "Shift adaptation releasing clutch torque offset map",
        "",
        "",
        "Gear change",
        "",
        "Torque offset",
        "Nm",
        Some(&["1->2", "2->3", "3->4", "4->5", "2->1", "3->2", "4->3", "5->4"]),
        None,
        false
    ),

    MapData::new(
        MapType::HfmTrqMap,
        "HFM Engine max torque table",
        "RPM",
        "",
        "Engine speed",
        "",
        "Maximum Torque",
        "Nm",
        None,
        None,
        false
    ),

    MapData::new(
        MapType::HfmMafMap,
        "HFM Engine MAF table",
        "RPM",
        "",
        "Engine speed",
        "",
        "",
        "",
        None,
        None,
        false
    ),

    MapData::new(
        MapType::HfmMaxMap,
        "HFM Engine Max MAF table",
        "RPM",
        "",
        "Engine speed",
        "",
        "",
        "",
        None,
        None,
        false
    ),


    MapData::new(
        MapType::TccRpmSlipMap,
        "Torque converter slipping target",
        "%",
        "RPM",
        "Load",
        "Input speed",
        "Target slip",
        "RPM",
        None,
        None,
        false
    ),
];
