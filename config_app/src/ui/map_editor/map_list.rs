use super::MapData;

pub(crate) const MAP_ARRAY: &[MapData] = &[
    MapData::new(
        0x01,
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
        //None
    ),
    MapData::new(
        0x02,
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
        //None
    ),
    MapData::new(
        0x03,
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
        //None
    ),
    MapData::new(
        0x04,
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
        //None
    ),
    MapData::new(
        0x05,
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
        //None
    ),
    MapData::new(
        0x06,
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
        //None
    ),
    MapData::new(
        0x09,
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
        //None
    ),
    MapData::new(
        0x0A,
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
        //None
    ),
    MapData::new(
        0x0B,
        "Clutch filling pressure",
        "%",
        "",
        "Input torque load",
        "Clutch",
        "filling pressure in millibar",
        "mBar",
        None,
        Some(&["K1", "K2", "K3", "B1", "B2", "B3"]),
        true
        //None
    ),
    MapData::new(
        0x10,
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
        //None
    ),
    MapData::new(
        0x11,
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
        //None
    ),
    MapData::new(
        0x12,
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
        //None
    ),
    MapData::new(
        0x13,
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
        //None
    ),
    MapData::new(
        0x14,
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
        //None
    ),
    MapData::new(
        0x15,
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
        //None
    ),
    MapData::new(
        0x16,
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
        //None
    ),
    MapData::new(
        0x17,
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
        //None
    ),
    MapData::new(
        0x18,
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
        //None
    ),
    MapData::new(
        0x19,
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
        //None
    ),
    MapData::new(
        0xA0,
        "Torque converter slip adapt map",
        "%",
        "",
        "Gear",
        "",
        "Slipping pressure(mBar)",
        "mBar",
        None,
        Some(&["D1", "D2", "D3", "D4", "D5"]),
        false
        //None
    ),
    MapData::new(
        0xA1,
        "Torque converter lock adapt map",
        "%",
        "",
        "Gear",
        "",
        "Locking pressure(mBar)",
        "mBar",
        None,
        Some(&["D1", "D2", "D3", "D4", "D5"]),
        false
        //None
    ),
];
