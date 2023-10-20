//! Read data by local identifier data structures
//! Based on diag_data.h in TCM source code
//!
use backend::ecu_diagnostics::dynamic_diag::DynamicDiagSession;
use backend::ecu_diagnostics::{DiagError, DiagServerResult};
use eframe::egui::{self, Color32, InnerResponse, RichText, Ui};
use eframe::wgpu::Color;
use packed_struct::PackedStructSlice;
use packed_struct::prelude::{PackedStruct, PrimitiveEnum_u8};

pub const RLI_QUERY_INTERVAL: u64 = 100;
pub const RLI_PLOT_INTERVAL: u64 = 1000/60;

#[repr(u8)]
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Debug)]
pub enum RecordIdents {
    GearboxSensors = 0x20,
    SolenoidStatus = 0x21,
    CanDataDump = 0x22,
    SysUsage = 0x23,
    PressureStatus = 0x25,
    SSData = 0x27,
    ClutchSpeeds = 0x30,
    ClutchVelocities = 0x31,
}


pub(crate) fn read_struct<T>(c: &[u8]) -> DiagServerResult<T>
where
    T: PackedStruct,
{
    T::unpack_from_slice(&c).map_err(|e| DiagError::InvalidResponseLength)
}

impl RecordIdents {
    pub fn query_ecu(
        &self,
        server: &DynamicDiagSession,
    ) -> DiagServerResult<LocalRecordData> {
        let resp = server.kwp_read_custom_local_identifier(*self as u8)?;
        match self {
            Self::GearboxSensors => Ok(LocalRecordData::Sensors(read_struct(&resp)?)),
            Self::SolenoidStatus => Ok(LocalRecordData::Solenoids(read_struct(&resp)?)),
            Self::CanDataDump => Ok(LocalRecordData::Canbus(read_struct(&resp)?)),
            Self::SysUsage => Ok(LocalRecordData::SysUsage(read_struct(&resp)?)),
            Self::PressureStatus => Ok(LocalRecordData::Pressures(read_struct(&resp)?)),
            Self::SSData => Ok(LocalRecordData::ShiftMonitorLive(read_struct(&resp)?)),
            Self::ClutchSpeeds => Ok(LocalRecordData::ClutchSpeeds(read_struct(&resp)?)),
            Self::ClutchVelocities => Ok(LocalRecordData::ClutchVelocities(read_struct(&resp)?))
        }
    }
}

#[derive(Debug, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub enum LocalRecordData {
    Sensors(DataGearboxSensors),
    Solenoids(DataSolenoids),
    Canbus(DataCanDump),
    SysUsage(DataSysUsage),
    Pressures(DataPressures),
    ShiftMonitorLive(DataShiftManager),
    ClutchSpeeds(DataClutchSpeeds),
    ClutchVelocities(DataShiftClutchVelocity),
}

impl LocalRecordData {
    pub fn to_table(&self, ui: &mut Ui) -> InnerResponse<()> {
        match &self {
            LocalRecordData::Sensors(s) => s.to_table(ui),
            LocalRecordData::Solenoids(s) => s.to_table(ui),
            LocalRecordData::Canbus(s) => s.to_table(ui),
            LocalRecordData::SysUsage(s) => s.to_table(ui),
            LocalRecordData::Pressures(s) => s.to_table(ui),
            LocalRecordData::ShiftMonitorLive(s) => s.to_table(ui),
            LocalRecordData::ClutchSpeeds(s) => s.to_table(ui),
            LocalRecordData::ClutchVelocities(s) => s.to_table(ui),
            _ => egui::Grid::new("DGS").striped(true).show(ui, |ui| {}),
        }
    }

    pub fn get_chart_data(&self) -> Vec<ChartData> {
        match &self {
            LocalRecordData::Sensors(s) => s.to_chart_data(),
            LocalRecordData::Solenoids(s) => s.to_chart_data(),
            LocalRecordData::Canbus(s) => s.to_chart_data(),
            LocalRecordData::SysUsage(s) => s.to_chart_data(),
            LocalRecordData::Pressures(s) => s.to_chart_data(),
            LocalRecordData::ShiftMonitorLive(s) => s.to_chart_data(),
            LocalRecordData::ClutchSpeeds(s) => s.to_chart_data(),
            LocalRecordData::ClutchVelocities(s) => s.to_chart_data(),
            _ => vec![],
        }
    }
}

#[derive(Debug, Clone, Ord, PartialOrd, Eq, PartialEq, PackedStruct)]
#[packed_struct(endian="lsb")]
pub struct DataPressures {
    pub ss_flag: u8,
    pub shift_req_pressure: u16,
    pub modulating_req_pressure: u16,
    pub working_pressure: u16,
    pub inlet_pressure: u16,
    pub corrected_spc_pressure: u16,
    pub corrected_mpc_pressure: u16,
    pub tcc_pressure: u16
}

impl DataPressures {
    pub fn to_table(&self, ui: &mut Ui) -> InnerResponse<()> {
        egui::Grid::new("DGS").striped(true).show(ui, |ui| {
            ui.label("Req. Shift pressure");
            ui.label(if self.shift_req_pressure == u16::MAX {
                make_text("ERROR", true)
            } else {
                make_text(format!("{} mBar", self.shift_req_pressure), false)
            });
            ui.end_row();

            ui.label("Req. Modulating pressure");
            ui.label(if self.modulating_req_pressure == u16::MAX {
                make_text("ERROR", true)
            } else {
                make_text(format!("{} mBar", self.modulating_req_pressure), false)
            });
            ui.end_row();

            ui.label("Req. Torque converter pressure");
            ui.label(if self.tcc_pressure == u16::MAX {
                make_text("ERROR", true)
            } else {
                make_text(format!("{} mBar", self.tcc_pressure), false)
            });
            ui.end_row();

            ui.label("Corrected shift pressure");
            ui.label(if self.corrected_spc_pressure == u16::MAX {
                make_text("ERROR", true)
            } else {
                make_text(format!("{} mBar", self.corrected_spc_pressure), false)
            });
            ui.end_row();

            ui.label("Corrected modulating pressure");
            ui.label(if self.corrected_mpc_pressure == u16::MAX {
                make_text("ERROR", true)
            } else {
                make_text(format!("{} mBar", self.corrected_mpc_pressure), false)
            });
            ui.end_row();

            ui.label("Calc. Solenoid inlet pressure");
            ui.label(if self.inlet_pressure == u16::MAX {
                make_text("ERROR", true)
            } else {
                make_text(format!("{} mBar", self.inlet_pressure), false)
            });
            ui.end_row();

            ui.label("Calc. Working pressure");
            ui.label(if self.working_pressure == u16::MAX {
                make_text("ERROR", true)
            } else {
                make_text(format!("{} mBar", self.working_pressure), false)
            });
            ui.end_row();

            ui.label("Active shift circuits");
            ui.label(if self.ss_flag == 0 {
                make_text("None", false)
            } else {
                let mut s: Vec<&'static str> = Vec::new();
                if (self.ss_flag & (1 << 0)) != 0 {
                    s.push("1-2");
                }
                if (self.ss_flag & (1 << 1)) != 0 {
                    s.push("2-3");
                }
                if (self.ss_flag & (1 << 2)) != 0 {
                    s.push("3-4");
                }
                make_text(format!("{:?}", s), false)
            });
            ui.end_row();
        })
    }

    pub fn to_chart_data(&self) -> Vec<ChartData> {
        vec![ChartData::new(
            "Gearbox Pressures (In and calc)".into(),
            vec![
                ("Calc. working pressure", self.working_pressure as f32, Some("mBar"), Color32::from_rgb(217,38,28)),
                ("Calc. inlet pressure", self.inlet_pressure as f32, Some("mBar"), Color32::from_rgb(0, 145, 64)),
                ("Req. modulating pressure", self.modulating_req_pressure as f32, Some("mBar"), Color32::from_rgb(255,245,0)),
                ("Req. shift pressure", self.shift_req_pressure as f32, Some("mBar"), Color32::from_rgb(0,148,222)),
                ("Req. TCC pressure", self.tcc_pressure as f32, Some("mBar"), Color32::from_rgb(232,120,23)),
            ],
            None
        ),
        ChartData::new(
            "Gearbox Pressures (Output)".into(),
            vec![
                ("Corrected modulating pressure", self.corrected_mpc_pressure as f32, Some("mBar"), Color32::from_rgb(255,245,0)),
                ("Corrected shift pressure", self.corrected_spc_pressure as f32, Some("mBar"), Color32::from_rgb(0,148,222)),
            ],
            None
        )]
    }
}

#[derive(Debug, Clone, Ord, PartialOrd, Eq, PartialEq, PackedStruct)]
#[packed_struct(endian="lsb")]
pub struct DataGearboxSensors {
    pub n2_rpm: u16,
    pub n3_rpm: u16,
    pub calculated_rpm: u16,
    pub calc_ratio: u16,
    pub v_batt: u16,
    pub atf_temp_c: u32,
    pub parking_lock: u8,
    pub output_rpm: u16
}

fn make_text<T: Into<String>>(t: T, e: bool) -> egui::RichText {
    let mut s = RichText::new(t);
    if e {
        s = s.color(Color32::from_rgb(255, 0, 0))
    }
    s
}

impl DataGearboxSensors {
    pub fn to_table(&self, ui: &mut Ui) -> InnerResponse<()> {
        egui::Grid::new("DGS").striped(true).show(ui, |ui| {
            ui.label("N2 Pulse counter")
                .on_hover_text("Raw counter value for PCNT for N2 hall effect RPM sensor");
            ui.label(if self.n2_rpm == u16::MAX {
                make_text("ERROR", true)
            } else {
                make_text(format!("{} pulses/min", self.n2_rpm), false)
            });
            ui.end_row();

            ui.label("N3 Pulse counter")
                .on_hover_text("Raw counter value for PCNT for N3 hall effect RPM sensor");
            ui.label(if self.n3_rpm == u16::MAX {
                make_text("ERROR", true)
            } else {
                make_text(format!("{} pulses/min", self.n3_rpm), false)
            });
            ui.end_row();

            ui.label("Calculated input RPM")
                .on_hover_text("Calculated input shaft RPM based on N2 and N3 raw values");
            ui.label(if self.calculated_rpm == u16::MAX {
                make_text("ERROR", true)
            } else {
                make_text(format!("{} RPM", self.calculated_rpm), false)
            });
            ui.end_row();

            ui.label("Calculated output RPM")
                .on_hover_text("Calculated output RPM. Either based on GPIO pin, or CAN Data");
            ui.label(if self.output_rpm == u16::MAX {
                make_text("ERROR", true)
            } else {
                make_text(format!("{} RPM", self.output_rpm), false)
            });
            ui.end_row();

            ui.label("Calculated ratio")
                .on_hover_text("Calculated gear ratio");
            ui.label(if self.calculated_rpm == u16::MAX {
                make_text("ERROR", true)
            } else {
                make_text(format!("{:.2}", self.calc_ratio as f32 / 100.0), false)
            });
            ui.end_row();

            ui.label("Battery voltage");
            ui.label(if self.v_batt == u16::MAX {
                make_text("ERROR", true)
            } else {
                make_text(format!("{:.1} V", self.v_batt as f32 / 1000.0), false)
            });
            ui.end_row();

            ui.label("ATF Oil temperature\n(Only when parking lock off)");
            ui.label(if self.parking_lock != 0x00 {
                make_text("Cannot read\nParking lock engaged", true)
            } else {
                make_text(format!("{} *C", self.atf_temp_c as i32), false)
            });
            ui.end_row();

            ui.label("Parking lock");
            ui.label(if self.parking_lock == 0x00 {
                make_text("No", false)
            } else {
                make_text("Yes", false)
            });
            ui.end_row();
        })
    }

    pub fn to_chart_data(&self) -> Vec<ChartData> {
        vec![ChartData::new(
            "RPM sensors".into(),
            vec![
                ("N2 raw", self.n2_rpm as f32, Some("RPM"), Color32::from_rgb(0, 0, 255)),
                ("N3 raw", self.n3_rpm as f32, Some("RPM"), Color32::from_rgb(0, 255, 0)),
                ("Calculated RPM", self.calculated_rpm as f32, Some("RPM"), Color32::from_rgb(255, 0, 0)),
            ],
            Some((0.0, 0.0)),
        )]
    }
}

#[derive(Debug, Clone)]
pub struct ChartData {
    /// Min, Max
    pub bounds: Option<(f32, f32)>,
    pub group_name: String,
    pub data: Vec<(String, f32, Option<&'static str>, Color32)>, // Data field name, data field value, data field unit
}

impl ChartData {
    pub fn new<T: Into<String>>(
        group_name: String,
        data: Vec<(T, f32, Option<&'static str>, Color32)>,
        bounds: Option<(f32, f32)>
    ) -> Self {
        Self {
            bounds,
            group_name,
            data: data
                .into_iter()
                .map(|(n, v, u, c)| (n.into(), v, u.map(|x| x.into()), c))
                .collect(),
        }
    }
}

#[derive(Debug, Default, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, PackedStruct)]
#[packed_struct(endian="lsb")]
pub struct DataSolenoids {
    pub spc_pwm: u16,
    pub mpc_pwm: u16,
    pub tcc_pwm: u16,
    pub y3_pwm: u16,
    pub y4_pwm: u16,
    pub y5_pwm: u16,
    pub spc_current: u16,
    pub mpc_current: u16,
    pub tcc_current: u16,
    pub targ_spc_current: u16,
    pub targ_mpc_current: u16,
    pub adjustment_spc: u16,
    pub adjustment_mpc: u16,
    pub y3_current: u16,
    pub y4_current: u16,
    pub y5_current: u16,
}

impl DataSolenoids {
    pub fn to_table(&self, ui: &mut Ui) -> InnerResponse<()> {
        egui::Grid::new("DGS").striped(true).show(ui, |ui| {
            ui.label("MPC Solenoid Driver");
            ui.label(format!(
                "PWM {:>4}/4096, Trim: {:.2}%",
                self.mpc_pwm,
                (self.adjustment_mpc as f32 / 10.0) - 100.0,
            ));
            ui.end_row();

            ui.label("MPC Solenoid Target / actual current");
            ui.label(format!(
                "{} mA/{} mA",
                self.targ_mpc_current,
                self.mpc_current,
            ));
            ui.end_row();

            ui.label("SPC Solenoid Driver");
            ui.label(format!(
                "PWM {:>4}/4096, Trim: {:.2}%",
                self.spc_pwm,
                (self.adjustment_spc as f32 / 10.0) - 100.0,
            ));
            ui.end_row();

            ui.label("MPC Solenoid Target / actual current");
            ui.label(format!(
                "{} mA/{} mA",
                self.targ_spc_current,
                self.spc_current,
            ));
            ui.end_row();

            ui.label("TCC Solenoid");
            ui.label(format!(
                "PWM {:>4}/4096, Read current {} mA",
                self.tcc_pwm,
                self.tcc_current
            ));
            ui.end_row();

            ui.label("Y3 shift Solenoid");
            ui.label(format!(
                "PWM {:>4}/4096, Read {} mA",
                self.y3_pwm,
                self.y3_current
            ));
            ui.end_row();

            ui.label("Y4 shift Solenoid");
            ui.label(format!(
                "PWM {:>4}/4096, Read {} mA",
                self.y4_pwm,
                self.y4_current
            ));
            ui.end_row();

            ui.label("Y5 shift Solenoid");
            ui.label(format!(
                "PWM {:>4}/4096, Read {} mA",
                self.y5_pwm,
                self.y5_current
            ));
            ui.end_row();

            ui.label("Total current consumption");
            ui.label(format!(
                "{} mA",
                self.y5_current as u32
                    + self.y4_current as u32
                    + self.y3_current as u32
                    + self.mpc_current as u32
                    + self.spc_current as u32
                    + self.tcc_current as u32
            ));
            ui.end_row();
        })
    }

    pub fn to_chart_data(&self) -> Vec<ChartData> {
        vec![
            ChartData::new(
                "Solenoid PWM".into(),
                vec![
                    ("MPC Solenoid", self.mpc_pwm as f32, None, Color32::from_rgb(255,245,0)),
                    ("SPC Solenoid", self.spc_pwm as f32, None, Color32::from_rgb(0,148,222)),
                    ("TCC Solenoid", self.tcc_pwm as f32, None, Color32::from_rgb(232,120,23)),
                    ("Y3 Solenoid", self.y3_pwm as f32, None, Color32::from_rgb(0, 0, 128)),
                    ("Y4 Solenoid", self.y4_pwm as f32, None, Color32::from_rgb(0, 0, 192)),
                    ("Y5 Solenoid", self.y5_pwm as f32, None, Color32::from_rgb(0, 0, 255)),
                ],
                Some((0.0, 4096.0)),
            ),
            ChartData::new(
                "Solenoid Current (Recorded)".into(),
                vec![
                    ("MPC Solenoid", self.mpc_current as f32, Some("mA"), Color32::from_rgb(255,245,0)),
                    ("SPC Solenoid", self.spc_current as f32, Some("mA"), Color32::from_rgb(0,148,222)),
                    ("TCC Solenoid", self.tcc_current as f32, Some("mA"), Color32::from_rgb(232,120,23)),
                    ("Y3 Solenoid", self.y3_current as f32, Some("mA"), Color32::from_rgb(0, 0, 128)),
                    ("Y4 Solenoid", self.y4_current as f32, Some("mA"), Color32::from_rgb(0, 0, 192)),
                    ("Y5 Solenoid", self.y5_current as f32, Some("mA"), Color32::from_rgb(0, 0, 255)),
                ],
                Some((0.0, 6600.0)),
            ),
            ChartData::new(
                "Constant current solenoid trim".into(),
                vec![
                    ("MPC Solenoid", (self.adjustment_mpc as f32 / 10.0) - 100.0, Some("%"), Color32::from_rgb(255,245,0)),
                    ("SPC Solenoid", (self.adjustment_spc as f32 / 10.0) - 100.0, Some("%"), Color32::from_rgb(0,148,222)),
                ],
                Some((-100.0, 100.0)),
            )
        ]
    }
}


#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, PrimitiveEnum_u8)]
pub enum TorqueReqCtrlType {
    None = 0,
    NormalSpeed = 1,
    FastAsPossible = 2,
    BackToDriverDemand = 3
}

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, PrimitiveEnum_u8)]
pub enum TorqueReqBounds {
    LessThan = 0,
    MoreThan = 1,
    Exact = 2
}

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, PrimitiveEnum_u8)]
pub enum PaddlePosition {
    None = 0,
    Plus = 1,
    Minus = 2,
    PlusAndMinus = 3,
    SNV = 0xFF,
}

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, PrimitiveEnum_u8)]
pub enum ShifterPosition {
    Park = 0,
    ParkReverse = 1,
    Reverse = 2,
    ReverseNeutral = 3,
    Neutral = 4,
    NeutralDrive = 5,
    Drive = 6,
    Plus = 7,
    Minus = 8,
    Four = 9,
    Three = 10,
    Two = 11,
    One = 12,
    SNV = 0xFF,
}

#[derive(Debug, Clone, Ord, PartialOrd, Eq, PartialEq, PackedStruct)]
#[packed_struct(endian="lsb")]
pub struct DataCanDump {
    pub pedal_position: u8,
    pub min_torque_ms: u16,
    pub max_torque_ms: u16,
    pub static_torque: u16,
    pub driver_torque: u16,
    pub left_rear_rpm: u16,
    pub right_rear_rpm: u16,
    pub shift_profile_pressed: u8,
    #[packed_field(size_bytes="1", ty="enum")]
    pub selector_position: ShifterPosition,
    #[packed_field(size_bytes="1", ty="enum")]
    pub paddle_position: PaddlePosition,
    pub engine_rpm: u16,
    pub fuel_flow: u16,
    pub egs_req_torque: u16,
    #[packed_field(size_bytes="1", ty="enum")]
    pub egs_torque_req_ctrl_type: TorqueReqCtrlType,
    #[packed_field(size_bytes="1", ty="enum")]
    pub egs_torque_req_bounds: TorqueReqBounds,
    pub engine_iat_temp: i16,
    pub engine_oil_temp: i16,
    pub engine_coolant_temp: i16
}

impl DataCanDump {
    pub fn to_table(&self, ui: &mut Ui) -> InnerResponse<()> {
        egui::Grid::new("DGS").striped(true).show(ui, |ui| {
            ui.label("Accelerator pedal position");
            ui.label(if self.pedal_position == u8::MAX {
                make_text("Signal not available", true)
            } else {
                make_text(
                    format!("{:.1} %", self.pedal_position as f32 / 250.0 * 100.0),
                    false,
                )
            });
            ui.end_row();

            ui.label("Engine RPM");
            ui.label(if self.engine_rpm == u16::MAX {
                make_text("Signal not available", true)
            } else {
                make_text(format!("{} RPM", self.engine_rpm as f32), false)
            });
            ui.end_row();

            ui.label("Engine minimum torque");
            ui.label(if self.min_torque_ms == u16::MAX {
                make_text("Signal not available", true)
            } else {
                make_text(
                    format!("{:.1} Nm", self.min_torque_ms as f32 / 4.0 - 500.0),
                    false,
                )
            });
            ui.end_row();

            ui.label("Engine maximum torque");
            ui.label(if self.max_torque_ms == u16::MAX {
                make_text("Signal not available", true)
            } else {
                make_text(
                    format!("{:.1} Nm", self.max_torque_ms as f32 / 4.0 - 500.0),
                    false,
                )
            });
            ui.end_row();

            ui.label("Engine static torque");
            ui.label(if self.static_torque == u16::MAX {
                make_text("Signal not available", true)
            } else {
                make_text(
                    format!("{:.1} Nm", self.static_torque as f32 / 4.0 - 500.0),
                    false,
                )
            });
            ui.end_row();

            ui.label("Driver req torque");
            ui.label(if self.driver_torque == u16::MAX {
                make_text("Signal not available", true)
            } else {
                make_text(
                    format!("{:.1} Nm", self.driver_torque as f32 / 4.0 - 500.0),
                    false,
                )
            });
            ui.end_row();

            ui.label("Rear right wheel speed");
            ui.label(if self.right_rear_rpm == u16::MAX {
                make_text("Signal not available", true)
            } else {
                make_text(
                    format!("{:.1} RPM", self.right_rear_rpm as f32 / 2.0),
                    false,
                )
            });
            ui.end_row();

            ui.label("Rear left wheel speed");
            ui.label(if self.left_rear_rpm == u16::MAX {
                make_text("Signal not available", true)
            } else {
                make_text(
                    format!("{:.1} RPM", self.left_rear_rpm as f32 / 2.0),
                    false,
                )
            });
            ui.end_row();

            ui.label("Gear selector position");
            ui.label(if self.selector_position == ShifterPosition::SNV {
                make_text("Signal not available", true)
            } else {
                make_text(format!("{:?}", self.selector_position), false)
            });
            ui.end_row();

            ui.label("Shift paddle position");
            ui.label(if self.paddle_position == PaddlePosition::SNV {
                make_text("Signal not available", true)
            } else {
                make_text(format!("{:?}", self.paddle_position), false)
            });
            ui.end_row();

            ui.label("Fuel flow");
            ui.label(format!("{} ul/s", self.fuel_flow));
            ui.end_row();

            ui.label("Torque request");
            if self.egs_torque_req_ctrl_type == TorqueReqCtrlType::None {
                ui.label("None");
            } else {
                ui.label(format!("{} Nm ({:?})", self.egs_req_torque as f32 / 4.0 - 500.0, self.egs_torque_req_ctrl_type));
                ui.end_row();
                ui.label(format!("({:?})", self.egs_torque_req_bounds));
            }
            ui.end_row();
            
            ui.label("Engine intake air temp");
            ui.label(if self.engine_iat_temp == core::i16::MAX {
                make_text("Signal not available", true)
            } else {
                make_text(format!("{}C", self.engine_iat_temp), false)
            });
            ui.end_row();

            ui.label("Engine coolant temp");
            ui.label(if self.engine_coolant_temp == core::i16::MAX {
                make_text("Signal not available", true)
            } else {
                make_text(format!("{}C", self.engine_coolant_temp), false)
            });
            ui.end_row();

            ui.label("Engine oil temp");
            ui.label(if self.engine_oil_temp == core::i16::MAX {
                make_text("Signal not available", true)
            } else {
                make_text(format!("{}C", self.engine_oil_temp), false)
            });
            ui.end_row();
        })
    }

    pub fn to_chart_data(&self) -> Vec<ChartData> {
        let min = if self.min_torque_ms == u16::MAX {
            0.0
        } else {
            self.min_torque_ms as f32 / 4.0 - 500.0
        };
        let sta = if self.static_torque == u16::MAX {
            0.0
        } else {
            self.static_torque as f32 / 4.0 - 500.0
        };
        let drv = if self.driver_torque == u16::MAX {
            0.0
        } else {
            self.driver_torque as f32 / 4.0 - 500.0
        };
        let egs = if self.egs_req_torque == u16::MAX || self.egs_torque_req_ctrl_type == TorqueReqCtrlType::None {
            0.0
        } else {
            self.egs_req_torque as f32 / 4.0 - 500.0
        };
        vec![ChartData::new(
            "Torque data".into(),
            vec![
                ("Min trq", min, Some("Nm"), Color32::from_rgb(0, 0, 255)),
                ("Static trq", sta, Some("Nm"), Color32::from_rgb(0, 255, 0)),
                ("Demanded trq", drv, Some("Nm"), Color32::from_rgb(0, 255, 255)),
                ("EGS Requested trq", egs, Some("Nm"), Color32::from_rgb(255, 0, 0))
            ],
            None,
        ),
        ChartData::new(
            "Fuel usage".into(),
            vec![
                ("Fuel flow", self.fuel_flow as f32, Some("ul/sec"), Color32::from_rgb(255, 0, 0)),
            ],
            None,
        ),
        ChartData::new(
            "Wheel speeds".into(),
            vec![
                ("Rear left wheel", self.left_rear_rpm as f32, Some("RPM"), Color32::from_rgb(0, 255, 0)),
                ("Rear right wheel", self.right_rear_rpm as f32, Some("RPM"), Color32::from_rgb(0, 0, 255)),
            ],
            None,
        )]
    }
}

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, PackedStruct)]
#[packed_struct(endian="lsb")]
pub struct DataSysUsage {
    core1_usage: u16,
    core2_usage: u16,
    free_ram: u32,
    total_ram: u32,
    free_psram: u32,
    total_psram: u32,
    num_tasks: u32,
}

impl DataSysUsage {
    pub fn to_table(&self, ui: &mut Ui) -> InnerResponse<()> {
        let r_f = self.free_ram as f32;
        let r_t = self.total_ram as f32;
        let p_f = self.free_psram as f32;
        let p_t = self.total_psram as f32;

        let used_ram_perc = 100f32 * (r_t - r_f) / r_t;
        let used_psram_perc = 100f32 * (p_t - p_f) / p_t;

        egui::Grid::new("DGS").striped(true).show(ui, |ui| {
            ui.label("Core 1 usage");
            ui.label(format!("{:.1} %", self.core1_usage as f32 / 10.0));
            ui.end_row();

            ui.label("Core 2 usage");
            ui.label(format!("{:.1} %", self.core2_usage as f32 / 10.0));
            ui.end_row();

            ui.label("Free internal RAM");
            ui.label(format!(
                "{:.1} Kb ({:.1}% Used)",
                self.free_ram as f32 / 1024.0,
                used_ram_perc
            ));
            ui.end_row();

            ui.label("Free PSRAM");
            ui.label(format!(
                "{:.1} Kb ({:.1}% Used)",
                self.free_psram as f32 / 1024.0,
                used_psram_perc
            ));
            ui.end_row();

            ui.label("Num. OS Tasks");
            ui.label(format!("{}", self.num_tasks));
            ui.end_row();
        })
    }

    pub fn to_chart_data(&self) -> Vec<ChartData> {
        let r_f = self.free_ram as f32;
        let r_t = self.total_ram as f32;
        let p_f = self.free_psram as f32;
        let p_t = self.total_psram as f32;
        let used_ram_perc = 100f32 * (r_t - r_f) / r_t;
        let used_psram_perc = 100f32 * (p_t - p_f) / p_t;
        vec![ChartData::new(
            "CPU Usage".into(),
            vec![
                ("Core 1", self.core1_usage as f32 / 10.0, Some("%"), Color32::from_rgb(0, 255, 128)),
                ("Core 2", self.core2_usage as f32 / 10.0, Some("%"), Color32::from_rgb(255, 0, 128)),
            ],
            Some((0.0, 100.0)),
        ),
        ChartData::new(
            "Mem Usage".into(),
            vec![
                ("IRAM", used_ram_perc, Some("%"), Color32::from_rgb(0, 255, 0)),
                ("PSRAM", used_psram_perc, Some("%"), Color32::from_rgb(0, 0, 255)),
            ],
            Some((0.0, 100.0))
        ),
        ChartData::new(
            "OS Task count".into(),
            vec![
                ("Count", self.num_tasks as f32, None, Color32::from_rgb(0, 255, 0)),
            ],
            None
        )]
    }
}

#[repr(u8)]
pub enum ShiftIdx {
    NoShift = 0,
    OneTwo = 1,
    TwoThree = 2,
    ThreeFour = 3,
    FourFive = 4,
    FiveFour = 5,
    FourThree = 6,
    ThreeTwo = 7,
    TwoOne = 8,
    Unknown = 0xFF,
}

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, PackedStruct)]
#[packed_struct(endian="lsb")]
pub struct DataShiftManager {
    pub spc_pressure_mbar: u16,
    pub mpc_pressure_mbar: u16,
    pub tcc_pressure_mbar: u16,
    pub shift_solenoid_pos: u8,
    pub input_rpm: u16,
    pub engine_rpm: u16,
    pub output_rpm: u16,
    pub engine_torque: i16,
    pub input_torque: i16,
    pub req_engine_torque: i16,
    pub atf_temp: u8,
    pub shift_idx: u8,
}

impl DataShiftManager {
    pub fn to_table(&self, ui: &mut Ui) -> InnerResponse<()> {
        egui::Grid::new("SM").striped(true).show(ui, |ui| {
            ui.label("SPC Pressure");
            ui.label(format!("{} mBar", self.spc_pressure_mbar));
            ui.end_row();

            ui.label("MPC pressure");
            ui.label(format!("{} mBar", self.mpc_pressure_mbar));
            ui.end_row();

            ui.label("TCC pressure");
            ui.label(format!("{} mBar", self.tcc_pressure_mbar));
            ui.end_row();

            ui.label("Shift solenoid pos");
            ui.label(format!("{}/255", self.shift_solenoid_pos));
            ui.end_row();

            ui.label("Input shaft speed");
            ui.label(format!("{} RPM", self.input_rpm));
            ui.end_row();

            ui.label("Engine speed");
            ui.label(format!("{} RPM", self.engine_rpm));
            ui.end_row();

            ui.label("Output shaft speed");
            ui.label(format!("{} RPM", self.output_rpm));
            ui.end_row();

            ui.label("Shift state");
            ui.label(match self.shift_idx {
                0 => "None",
                1 => "1 -> 2",
                2 => "2 -> 3",
                3 => "3 -> 4",
                4 => "4 -> 5",
                5 => "5 -> 4",
                6 => "4 -> 3",
                7 => "3 -> 2",
                8 => "2 -> 1",
                _ => "UNKNOWN",
            });
            ui.end_row();
        })
    }

    pub fn to_chart_data(&self) -> Vec<ChartData> {
        vec![ChartData::new(
            "RPMs".into(),
            vec![
                ("Input speed", self.input_rpm as f32, Some("RPM"), Color32::from_rgb(255, 0, 0)),
                ("Engine speed", self.engine_rpm as f32, Some("RPM"), Color32::from_rgb(0, 255, 0)),
            ],
            None,
        ),
        ChartData::new(
            "Solenoid pressures".into(),
            vec![
                ("Modulating pressure", self.mpc_pressure_mbar as f32, Some("mBar"), Color32::from_rgb(255,245,0)),
                ("Shift pressure", self.spc_pressure_mbar as f32, Some("mBar"), Color32::from_rgb(0,148,222)),
                ("TCC pressure", self.tcc_pressure_mbar as f32, Some("mBar"), Color32::from_rgb(232,120,23)),
            ],
            None,
        ),
        ChartData::new(
            "Torque data".into(),
            vec![
                ("Static torque", self.engine_torque as f32, Some("Nm"), Color32::from_rgb(0, 0, 255)),
                ("Input torque (calc)", self.input_torque as f32, Some("Nm"), Color32::from_rgb(0, 255, 0)),
                ("EGS Req torque", if self.req_engine_torque == i16::MAX { 0.0 } else { self.req_engine_torque as f32 }, Some("Nm"), Color32::from_rgb(255, 0, 0)),
            ],
            None,
        )]
    }
}

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, PackedStruct)]
#[packed_struct(endian="lsb")]
pub struct DataClutchSpeeds {
    k1: i16,
    k2: i16,
    k3: i16,
    b1: i16,
    b2: i16,
    b3: i16,
}

impl DataClutchSpeeds {
    pub fn to_table(&self, ui: &mut Ui) -> InnerResponse<()> {
        egui::Grid::new("SM").striped(true).show(ui, |ui| {

            ui.label("K1 speed");
            ui.label(format!("{} RPM", self.k1));
            ui.end_row();

            ui.label("K2 speed");
            ui.label(format!("{} RPM", self.k2));
            ui.end_row();

            ui.label("K3 speed");
            ui.label(format!("{} RPM", self.k3));
            ui.end_row();

            ui.label("B1 speed");
            ui.label(format!("{} RPM", self.b1));
            ui.end_row();

            ui.label("B2 speed");
            ui.label(format!("{} RPM", self.b2));
            ui.end_row();

            ui.label("B3 speed");
            ui.label(format!("{} RPM", self.b3));
            ui.end_row();
        })
    }

    pub fn to_chart_data(&self) -> Vec<ChartData> {
        vec![ChartData::new(
            "RPMs".into(),
            vec![
                ("K1", self.k1 as f32, Some("RPM"), Color32::from_rgb(0, 64, 0)),
                ("K2", self.k2 as f32, Some("RPM"), Color32::from_rgb(0, 128, 0)),
                ("K3", self.k3 as f32, Some("RPM"), Color32::from_rgb(0, 255, 0)),
                ("B1", self.b1 as f32, Some("RPM"), Color32::from_rgb(255, 0, 0)),
                ("B2", self.b2 as f32, Some("RPM"), Color32::from_rgb(255, 128, 0)),
                ("B3", self.b3 as f32, Some("RPM"), Color32::from_rgb(255, 255, 0)),
            ],
            None,
        )]
    }
}

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, PackedStruct)]
#[packed_struct(endian="lsb")]
pub struct DataShiftClutchVelocity {
    on_vel: i16,
    off_vel: i16
}

impl DataShiftClutchVelocity {
    pub fn to_table(&self, ui: &mut Ui) -> InnerResponse<()> {
        egui::Grid::new("SM").striped(true).show(ui, |ui| {

            ui.label("On clutch acceleration");
            ui.label(format!("{} RPM/100ms", self.on_vel));
            ui.end_row();

            ui.label("Off clutch acceleration");
            ui.label(format!("{} RPM/100ms", self.off_vel));
            ui.end_row();
        })
    }

    pub fn to_chart_data(&self) -> Vec<ChartData> {
        vec![ChartData::new(
            "Velocities".into(),
            vec![
                ("On clutch", self.on_vel as f32, Some("RPM/100 msec"), Color32::from_rgb(255, 0, 255)),
                ("Off clutch", self.off_vel as f32, Some("RPM/100 msec"), Color32::from_rgb(0, 255, 255)),
            ],
            None,
        )]
    }
}
