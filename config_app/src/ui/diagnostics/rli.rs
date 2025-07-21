//! Read data by local identifier data structures
//! Based on diag_data.h in TCM source code
//!
use std::fmt::Display;

use backend::ecu_diagnostics::dynamic_diag::DynamicDiagSession;
use backend::ecu_diagnostics::{DiagError, DiagServerResult};
use eframe::egui::{self, Color32, InnerResponse, RichText, ScrollArea, Ui, WidgetText};
use packed_struct::PackedStructSlice;
use packed_struct::prelude::{PackedStruct, PrimitiveEnum_u8};

pub const RLI_QUERY_INTERVAL: u64 = 100;
pub const RLI_PLOT_INTERVAL: u64 = 1000/60;

#[repr(u8)]
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Debug, strum_macros::VariantArray)]
pub enum RecordIdents {
    GearboxSensors = 0x20,
    SolenoidStatus = 0x21,
    CanDataDump = 0x22,
    SysUsage = 0x23,
    TccProgram = 0x24,
    PressureStatus = 0x25,
    SSData = 0x27,
    ClutchSpeeds = 0x30,
    ShiftingAlgoFeedback = 0x31,
}

impl ToString for RecordIdents {
    fn to_string(&self) -> String {
        match self {
            RecordIdents::GearboxSensors => "Gearbox sensors",
            RecordIdents::SolenoidStatus => "Solenoids",
            RecordIdents::CanDataDump => "CAN Data",
            RecordIdents::SysUsage => "System usage",
            RecordIdents::TccProgram => "TCC status",
            RecordIdents::PressureStatus => "Gearbox pressures",
            RecordIdents::SSData => "Shift info",
            RecordIdents::ClutchSpeeds => "Clutch speeds",
            RecordIdents::ShiftingAlgoFeedback => "Shift algorithm",
        }.to_string()
    }
}

pub(crate) fn read_struct<T>(c: &[u8]) -> DiagServerResult<T>
where
    T: PackedStruct,
{
    T::unpack_from_slice(&c).map_err(|_| DiagError::InvalidResponseLength)
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
            Self::TccProgram => Ok(LocalRecordData::TccProgram(read_struct(&resp)?)),
            Self::PressureStatus => Ok(LocalRecordData::Pressures(read_struct(&resp)?)),
            Self::SSData => Ok(LocalRecordData::ShiftMonitorLive(read_struct(&resp)?)),
            Self::ClutchSpeeds => Ok(LocalRecordData::ClutchSpeeds(read_struct(&resp)?)),
            Self::ShiftingAlgoFeedback => Ok(LocalRecordData::ShiftAlgoFeedback(read_struct(&resp)?))
        }
    }
}

#[derive(Debug, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub enum LocalRecordData {
    Sensors(DataGearboxSensors),
    Solenoids(DataSolenoids),
    Canbus(DataCanDump),
    SysUsage(DataSysUsage),
    TccProgram(TccProgramData),
    Pressures(DataPressures),
    ShiftMonitorLive(DataShiftManager),
    ClutchSpeeds(DataClutchSpeeds),
    ShiftAlgoFeedback(DataShiftAlgoFeedback),
}

fn make_row<T: Into<WidgetText>, X: Into<WidgetText>>(ui: &mut Ui, key: T, value: X) {
    ui.label(key);
    ui.label(value);
    ui.end_row();
}

pub enum DisplayErrorType {
    Error,
    SignalNotAvailable,
}

fn make_display_value<T: Eq + Display>(value: T, e_eq: T, e: DisplayErrorType, unit: Option<&'static str>) -> RichText {
    let text = if value == e_eq {
        match e {
            DisplayErrorType::Error => format!("ERROR"),
            DisplayErrorType::SignalNotAvailable => format!("Signal not avaiable"),
        }
    } else {
        match unit {
            Some(u) => format!("{value} {u}"),
            None => format!("{value}"),
        }
    };

    let mut ret = RichText::new(text);
    if value == e_eq {
        ret = ret.color(Color32::RED);
    }
    ret
}

impl LocalRecordData {
    pub fn to_table(&self, ui: &mut Ui) {
        ScrollArea::new([false, true])
            .max_height(ui.available_height())
            .auto_shrink(false)
            .show(ui, |ui| {
            egui::Grid::new("DGS").striped(true).show(ui, |ui| {
                match &self {
                    LocalRecordData::Sensors(s) => {
                        ui.label("N2 Pulse counter")
                            .on_hover_text("Raw counter value for PCNT for N2 hall effect RPM sensor");
                        ui.label(make_display_value(s.n2_rpm, u16::MAX, DisplayErrorType::Error, Some("RPM")));
                        ui.end_row();

                        ui.label("N3 Pulse counter")
                            .on_hover_text("Raw counter value for PCNT for N3 hall effect RPM sensor");
                        ui.label(make_display_value(s.n3_rpm, u16::MAX, DisplayErrorType::Error, Some("RPM")));
                        ui.end_row();

                        ui.label("Calculated input RPM")
                            .on_hover_text("Calculated input shaft RPM based on N2 and N3 raw values");
                        ui.label(make_display_value(s.calculated_rpm, u16::MAX, DisplayErrorType::Error, Some("RPM")));
                        ui.end_row();

                        ui.label("Physical output RPM\n(If fitted)")
                            .on_hover_text("Output RPM sensor if fitted to GPIO 23");
                        ui.label(make_display_value(s.output_rpm, u16::MAX, DisplayErrorType::Error, Some("RPM")));
                        ui.end_row();

                        ui.label("Observed ratio")
                            .on_hover_text("Calculated gear ratio");
                        ui.label(if s.calc_ratio == u16::MAX {
                            RichText::new("ERROR").color(Color32::RED)
                        } else {
                            RichText::new(format!("{:.2}", s.calc_ratio as f32 / 100.0))
                        });
                        ui.end_row();

                        ui.label("Target ratio")
                            .on_hover_text("Target gear ratio");
                        ui.label(if s.targ_ratio == u16::MAX {
                            RichText::new("ERROR").color(Color32::RED)
                        } else {
                            RichText::new(format!("{:.2}", s.targ_ratio as f32 / 100.0))
                        });
                        ui.end_row();

                        ui.label("Battery voltage");
                        ui.label(if s.v_batt == u16::MAX {
                            RichText::new("ERROR").color(Color32::RED)
                        } else {
                            RichText::new(format!("{:.1} V", s.v_batt as f32 / 1000.0))
                        });
                        ui.end_row();

                        ui.label("ATF Oil temperature\n(Only when parking lock off)");
                        ui.label(if s.parking_lock != 0x00 {
                            RichText::new("Cannot read\nParking lock engaged").color(Color32::RED)
                        } else {
                            RichText::new(format!("{} *C", s.atf_temp_c as i32))
                        });
                        ui.end_row();

                        ui.label("Parking lock");
                        ui.label(if s.parking_lock == 0x00 {"No"} else {"Yes"});
                        ui.end_row();
                    },
                    LocalRecordData::Solenoids(s) => {
                        ui.label("MPC Solenoid Driver");
                        ui.label(format!(
                            "PWM {:>4}/4096, Trim: {:.2}%",
                            s.mpc_pwm,
                            (s.adjustment_mpc as f32 / 10.0) - 100.0,
                        ));
                        ui.end_row();

                        ui.label("MPC Solenoid Target / actual current");
                        ui.label(format!(
                            "{} mA/{} mA",
                            s.targ_mpc_current,
                            s.mpc_current,
                        ));
                        ui.end_row();

                        ui.label("SPC Solenoid Driver");
                        ui.label(format!(
                            "PWM {:>4}/4096, Trim: {:.2}%",
                            s.spc_pwm,
                            (s.adjustment_spc as f32 / 10.0) - 100.0,
                        ));
                        ui.end_row();

                        ui.label("MPC Solenoid Target / actual current");
                        ui.label(format!(
                            "{} mA/{} mA",
                            s.targ_spc_current,
                            s.spc_current,
                        ));
                        ui.end_row();

                        ui.label("TCC Solenoid");
                        ui.label(format!(
                            "PWM {:>4}/4096, Read current {} mA",
                            s.tcc_pwm,
                            s.tcc_current
                        ));
                        ui.end_row();

                        ui.label("Y3 shift Solenoid");
                        ui.label(format!(
                            "PWM {:>4}/4096, Read {} mA",
                            s.y3_pwm,
                            s.y3_current
                        ));
                        ui.end_row();

                        ui.label("Y4 shift Solenoid");
                        ui.label(format!(
                            "PWM {:>4}/4096, Read {} mA",
                            s.y4_pwm,
                            s.y4_current
                        ));
                        ui.end_row();

                        ui.label("Y5 shift Solenoid");
                        ui.label(format!(
                            "PWM {:>4}/4096, Read {} mA",
                            s.y5_pwm,
                            s.y5_current
                        ));
                        ui.end_row();

                        ui.label("Total current consumption");
                        ui.label(format!(
                            "{} mA",
                            s.y5_current as u32
                                + s.y4_current as u32
                                + s.y3_current as u32
                                + s.mpc_current as u32
                                + s.spc_current as u32
                                + s.tcc_current as u32
                        ));
                        ui.end_row();
                    },
                    LocalRecordData::Canbus(s) => {
                        ui.label("Accelerator pedal position");
                        ui.label(if s.pedal_position == u8::MAX {
                            RichText::new("Signal not available").color(Color32::RED)
                        } else {
                            RichText::new(format!("{:.1} %", s.pedal_position as f32 / 250.0 * 100.0))
                        });
                        ui.end_row();

                        ui.label("Engine RPM");
                        ui.label(make_display_value(s.engine_rpm, u16::MAX, DisplayErrorType::SignalNotAvailable, Some("RPM")));
                        ui.end_row();

                        ui.label("Engine minimum torque");
                        ui.label(if s.min_torque_ms == u16::MAX {
                            RichText::new("Signal not available").color(Color32::RED)
                        } else {
                            RichText::new(format!("{:.1} Nm", s.min_torque_ms as f32 / 4.0 - 500.0))
                        });
                        ui.end_row();

                        ui.label("Engine maximum torque");
                        ui.label(if s.max_torque_ms == u16::MAX {
                            RichText::new("Signal not available").color(Color32::RED)
                        } else {
                            RichText::new(format!("{:.1} Nm", s.max_torque_ms as f32 / 4.0 - 500.0))
                        });
                        ui.end_row();

                        ui.label("Engine static torque");
                        ui.label(if s.static_torque == u16::MAX {
                            RichText::new("Signal not available").color(Color32::RED)
                        } else {
                            RichText::new(format!("{:.1} Nm", s.static_torque as f32 / 4.0 - 500.0))
                        });
                        ui.end_row();

                        ui.label("Driver req torque");
                        ui.label(if s.driver_torque == u16::MAX {
                            RichText::new("Signal not available").color(Color32::RED)
                        } else {
                            RichText::new(format!("{:.1} Nm", s.driver_torque as f32 / 4.0 - 500.0))
                        });
                        ui.end_row();

                        ui.label("Rear right wheel speed");
                        ui.label(make_display_value(s.right_rear_rpm, u16::MAX, DisplayErrorType::SignalNotAvailable, Some("RPM")));
                        ui.end_row();

                        ui.label("Rear left wheel speed");
                        ui.label(make_display_value(s.left_rear_rpm, u16::MAX, DisplayErrorType::SignalNotAvailable, Some("RPM")));
                        ui.end_row();

                        ui.label("Gear selector position");
                        ui.label(make_display_value(s.selector_position, ShifterPosition::SNV, DisplayErrorType::SignalNotAvailable, None));
                        ui.end_row();

                        ui.label("Shift profile input");
                        ui.label(make_display_value(s.profile_input_raw, DiagProfileInputState::SNV, DisplayErrorType::SignalNotAvailable, None));
                        ui.end_row();

                        ui.label("Shift paddle position");
                        ui.label(make_display_value(s.paddle_position, PaddlePosition::SNV, DisplayErrorType::Error, None));
                        ui.end_row();

                        ui.label("Fuel flow");
                        ui.label(format!("{} ul/s", s.fuel_flow));
                        ui.end_row();

                        ui.label("Torque request");
                        if s.egs_torque_req_ctrl_type == TorqueReqCtrlType::None {
                            ui.label("None");
                        } else {
                            ui.label(format!("{} Nm ({:?})", s.egs_req_torque as f32 / 4.0 - 500.0, s.egs_torque_req_ctrl_type));
                            ui.end_row();
                            ui.label(format!("({:?})", s.egs_torque_req_bounds));
                        }
                        ui.end_row();
                        
                        ui.label("Engine intake air temp");
                        ui.label(make_display_value(s.engine_iat_temp, i16::MAX, DisplayErrorType::SignalNotAvailable, Some("C")));
                        ui.end_row();

                        ui.label("Engine coolant temp");
                        ui.label(make_display_value(s.engine_coolant_temp, i16::MAX, DisplayErrorType::SignalNotAvailable, Some("C")));
                        ui.end_row();

                        ui.label("Engine oil temp");
                        ui.label(make_display_value(s.engine_oil_temp, i16::MAX, DisplayErrorType::SignalNotAvailable, Some("C")));
                        ui.end_row();
                    },
                    LocalRecordData::SysUsage(s) => {
                        let r_f = s.free_ram as f32;
                        let r_t = s.total_ram as f32;
                        let p_f = s.free_psram as f32;
                        let p_t = s.total_psram as f32;

                        let used_ram_perc = 100f32 * (r_t - r_f) / r_t;
                        let used_psram_perc = 100f32 * (p_t - p_f) / p_t;

                        make_row(ui, "Core 1 usage", format!("{:.1} %", s.core1_usage as f32 / 10.0));
                        make_row(ui, "Core 2 usage", format!("{:.1} %", s.core2_usage as f32 / 10.0));

                        make_row(ui, "Core IRAM", format!("{:.1} Kb ({:.1}% Used)", s.free_ram as f32 / 1024.0, used_ram_perc));
                        make_row(ui, "Core PSRAM", format!("{:.1} Kb ({:.1}% Used)", s.free_psram as f32 / 1024.0, used_psram_perc));

                        make_row(ui, "OS Task count", format!("{}", s.num_tasks));
                    },
                    LocalRecordData::Pressures(s) => {
                        ui.label("Req. Shift pressure");
                        ui.label(make_display_value(s.shift_req_pressure, u16::MAX, DisplayErrorType::Error, Some("mBar")));
                        ui.end_row();

                        ui.label("Req. Modulating pressure");
                        ui.label(make_display_value(s.modulating_req_pressure, u16::MAX, DisplayErrorType::Error, Some("mBar")));
                        ui.end_row();

                        ui.label("Req. Torque converter pressure");
                        ui.label(make_display_value(s.tcc_pressure, u16::MAX, DisplayErrorType::Error, Some("mBar")));
                        ui.end_row();

                        ui.label("Corrected shift pressure");
                        ui.label(make_display_value(s.corrected_spc_pressure, u16::MAX, DisplayErrorType::Error, Some("mBar")));
                        ui.end_row();

                        ui.label("Corrected modulating pressure");
                        ui.label(make_display_value(s.corrected_mpc_pressure, u16::MAX, DisplayErrorType::Error, Some("mBar")));
                        ui.end_row();

                        ui.label("Calc. Solenoid inlet pressure");
                        ui.label(make_display_value(s.inlet_pressure, u16::MAX, DisplayErrorType::Error, Some("mBar")));
                        ui.end_row();

                        ui.label("Calc. Working pressure");
                        ui.label(make_display_value(s.working_pressure, u16::MAX, DisplayErrorType::Error, Some("mBar")));
                        ui.end_row();

                        ui.label("Active shift circuits");
                        ui.label(if s.ss_flag == 0 {
                            "None".to_string()
                        } else {
                            let mut s_flags: Vec<&'static str> = Vec::new();
                            if (s.ss_flag & (1 << 0)) != 0 {
                                s_flags.push("1-2");
                            }
                            if (s.ss_flag & (1 << 1)) != 0 {
                                s_flags.push("2-3");
                            }
                            if (s.ss_flag & (1 << 2)) != 0 {
                                s_flags.push("3-4");
                            }
                            if (s.ss_flag & (1 << 3)) != 0 {
                                s_flags.push("4-5");
                            }
                            format!("{:?}", s_flags)
                        });
                        ui.end_row();
                    },
                    LocalRecordData::ShiftMonitorLive(s) => {

                        make_row(ui, "SPC Pressure", format!("{} mBar", s.spc_pressure_mbar));
                        make_row(ui, "MPC Pressure", format!("{} mBar", s.mpc_pressure_mbar));
                        make_row(ui, "TCC Pressure", format!("{} mBar", s.tcc_pressure_mbar));

                        make_row(ui, "Shift solenoid pos", format!("{}/255", s.shift_solenoid_pos));
                        make_row(ui, "Input shaft speed", format!("{} RPM", s.input_rpm));
                        make_row(ui, "Engine shaft speed", format!("{} RPM", s.engine_rpm));
                        make_row(ui, "output shaft speed", format!("{} RPM", s.output_rpm));

                        let targ = (s.targ_act_gear >> 4) & 0x0F;
                        let actual = s.targ_act_gear & 0x0F;

                        fn geartext(b: u8) -> &'static str {
                            match b {
                                1 => "1",
                                2 => "2",
                                3 => "3",
                                4 => "4",
                                5 => "5",
                                8 => "P",
                                9 => "N",
                                10 => "R1",
                                11 => "R2",
                                _ => "UNKNOWN"
                            }
                        }

                        let state_text = if targ == actual {
                            geartext(actual).to_string()
                        } else {
                            format!("{} -> {}", geartext(actual), geartext(targ))
                        };
                        make_row(ui, "Gear", state_text);

                        let profile = match s.profile_id {
                            0 => "(S)tandard",
                            1 => "(C)omfort",
                            2 => "(W)inter",
                            3 => "(A)gility",
                            4 => "(M)anual",
                            5 => "(R)ace",
                            6 => "(I)ndividual",
                            7 => "_Init",
                            _ => "UNKNOWN"
                        };

                        make_row(ui, "Profile", profile);
                    },
                    LocalRecordData::ClutchSpeeds(s) => {

                        make_row(ui, "K1 speed", format!("{} RPM", s.k1));
                        make_row(ui, "K2 speed", format!("{} RPM", s.k2));
                        make_row(ui, "K3 speed", format!("{} RPM", s.k3));

                        make_row(ui, "B1 speed", format!("{} RPM", s.b1));
                        make_row(ui, "B2 speed", format!("{} RPM", s.b2));
                        make_row(ui, "B3 speed", format!("{} RPM", s.b3));
                    },
                    LocalRecordData::ShiftAlgoFeedback(s) => {
                        if s.active != 0 {
                            make_row(ui, "Algo phase", format!("{}", s.shift_phase));
                            make_row(ui, "Algo subphase mod", format!("{}", s.subphase_mod));
                            make_row(ui, "Algo subphase shift", format!("{}", s.subphase_shift));

                            make_row(ui, "On clutch speed", format!("{}", s.s_on));
                            make_row(ui, "Off clutch speed", format!("{}", s.s_off));

                            make_row(ui, "On clutch pressure", format!("{}", s.p_on));
                            make_row(ui, "Off clutch pressure", format!("{}", s.p_off));
                        } else {
                            ui.strong("No shift active");
                        }
                    },
                    LocalRecordData::TccProgram(s) => {
                        make_row(ui, "Target state", tcc_state_to_name(s.targ_state));
                        make_row(ui, "Current state", tcc_state_to_name(s.current_state));

                        make_row(ui, "Target pressure", format!("{} mBar", s.target_pressure));
                        make_row(ui, "Current pressure", format!("{} mBar", s.current_pressure));

                        make_row(ui, "Slip filtered", format!("{} RPM", s.slip_filtered));
                        make_row(ui, "Slip now", format!("{} RPM", s.slip_now));

                        make_row(ui, "Engine output (Kw)", format!("{} Kw", s.engine_output_joule / 1000));
                        make_row(ui, "Tcc absorbed power (J)", format!("{} J", s.tcc_absorbed_joule));
                    },
                }
            })
        });
    }

    pub fn get_chart_data(&self) -> Vec<ChartData> {
        match &self {
            LocalRecordData::Sensors(s) => {
                let ratio_float = if s.calc_ratio == u16::MAX {
                    0.0
                } else {
                    s.calc_ratio as f32 / 100.0
                };
                let ratio_targ_float = if s.targ_ratio == u16::MAX {
                    0.0
                } else {
                    s.targ_ratio as f32 / 100.0
                };
                vec![ChartData::new(
                    "RPM sensors".into(),
                    vec![
                        ("N2 raw speed", s.n2_rpm as f32, Some("RPM"), Color32::from_rgb(0, 0, 255)),
                        ("N3 raw speed", s.n3_rpm as f32, Some("RPM"), Color32::from_rgb(0, 255, 0)),
                        ("Calculated input speed", s.calculated_rpm as f32, Some("RPM"), Color32::from_rgb(255, 0, 0)),
                        ("Calculated output speed", s.output_rpm as f32, Some("RPM"), Color32::from_rgb(255, 255, 0)),
                    ],
                    Some((0.0, 0.0)),
                ),
                ChartData::new(
                    "RPM sensors".into(),
                    vec![
                        ("Observed ratio", ratio_float as f32, None, Color32::from_rgb(0, 0, 255)),
                        ("Target ratio", ratio_targ_float as f32, None, Color32::from_rgb(0, 255, 0)),
                    ],
                    Some((0.0, 0.0)),
                )
                ]
            },
            LocalRecordData::Solenoids(s) => {
                vec![
                    ChartData::new(
                        "Solenoid PWM".into(),
                        vec![
                            ("MPC Solenoid PWM", s.mpc_pwm as f32, None, Color32::from_rgb(255,245,0)),
                            ("SPC Solenoid PWM", s.spc_pwm as f32, None, Color32::from_rgb(0,148,222)),
                            ("TCC Solenoid PWM", s.tcc_pwm as f32, None, Color32::from_rgb(232,120,23)),
                            ("Y3 Solenoid PWM", s.y3_pwm as f32, None, Color32::from_rgb(0, 0, 128)),
                            ("Y4 Solenoid PWM", s.y4_pwm as f32, None, Color32::from_rgb(0, 0, 192)),
                            ("Y5 Solenoid PWM", s.y5_pwm as f32, None, Color32::from_rgb(0, 0, 255)),
                        ],
                        Some((0.0, 4096.0)),
                    ),
                    ChartData::new(
                        "Solenoid Current (Recorded)".into(),
                        vec![
                            ("MPC Solenoid current", s.mpc_current as f32, Some("mA"), Color32::from_rgb(255,245,0)),
                            ("SPC Solenoid current", s.spc_current as f32, Some("mA"), Color32::from_rgb(0,148,222)),
                            ("TCC Solenoid current", s.tcc_current as f32, Some("mA"), Color32::from_rgb(232,120,23)),
                            ("Y3 Solenoid current", s.y3_current as f32, Some("mA"), Color32::from_rgb(0, 0, 128)),
                            ("Y4 Solenoid current", s.y4_current as f32, Some("mA"), Color32::from_rgb(0, 0, 192)),
                            ("Y5 Solenoid current", s.y5_current as f32, Some("mA"), Color32::from_rgb(0, 0, 255)),
                        ],
                        Some((0.0, 6600.0)),
                    ),
                    ChartData::new(
                        "Constant current solenoid trim".into(),
                        vec![
                            ("MPC Solenoid trim", (s.adjustment_mpc as f32 / 10.0) - 100.0, Some("%"), Color32::from_rgb(255,245,0)),
                            ("SPC Solenoid trim", (s.adjustment_spc as f32 / 10.0) - 100.0, Some("%"), Color32::from_rgb(0,148,222)),
                        ],
                        Some((-100.0, 100.0)),
                    )
                ]
            },
            LocalRecordData::Canbus(s) => {
                let min = if s.min_torque_ms == u16::MAX {
                    0.0
                } else {
                    s.min_torque_ms as f32 / 4.0 - 500.0
                };
                let sta = if s.static_torque == u16::MAX {
                    0.0
                } else {
                    s.static_torque as f32 / 4.0 - 500.0
                };
                let drv = if s.driver_torque == u16::MAX {
                    0.0
                } else {
                    s.driver_torque as f32 / 4.0 - 500.0
                };
                let egs = if s.egs_req_torque == u16::MAX || s.egs_torque_req_ctrl_type == TorqueReqCtrlType::None {
                    0.0
                } else {
                    s.egs_req_torque as f32 / 4.0 - 500.0
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
                        ("Fuel flow", s.fuel_flow as f32, Some("ul/sec"), Color32::from_rgb(255, 0, 0)),
                    ],
                    None,
                ),
                ChartData::new(
                    "Wheel speeds".into(),
                    vec![
                        ("Rear left wheel speed", s.left_rear_rpm as f32, Some("RPM"), Color32::from_rgb(0, 255, 0)),
                        ("Rear right wheel speed", s.right_rear_rpm as f32, Some("RPM"), Color32::from_rgb(0, 0, 255)),
                    ],
                    None,
                )]
            },
            LocalRecordData::SysUsage(s) => {
                let r_f = s.free_ram as f32;
                let r_t = s.total_ram as f32;
                let p_f = s.free_psram as f32;
                let p_t = s.total_psram as f32;
                let used_ram_perc = 100f32 * (r_t - r_f) / r_t;
                let used_psram_perc = 100f32 * (p_t - p_f) / p_t;
                vec![ChartData::new(
                    "CPU Usage".into(),
                    vec![
                        ("Core 1 usage", s.core1_usage as f32 / 10.0, Some("%"), Color32::from_rgb(0, 255, 128)),
                        ("Core 2 usage", s.core2_usage as f32 / 10.0, Some("%"), Color32::from_rgb(255, 0, 128)),
                    ],
                    Some((0.0, 100.0)),
                ),
                ChartData::new(
                    "Mem Usage".into(),
                    vec![
                        ("IRAM Usage", used_ram_perc, Some("%"), Color32::from_rgb(0, 255, 0)),
                        ("PSRAM Usage", used_psram_perc, Some("%"), Color32::from_rgb(0, 0, 255)),
                    ],
                    Some((0.0, 100.0))
                ),
                ChartData::new(
                    "OS Task count".into(),
                    vec![
                        ("OS Tasks", s.num_tasks as f32, None, Color32::from_rgb(0, 255, 0)),
                    ],
                    None
                )]
            },
            LocalRecordData::Pressures(s) => {
                vec![ChartData::new(
                    "Gearbox Pressures (In and calc)".into(),
                    vec![
                        ("Calc line pressure", s.working_pressure as f32, Some("mBar"), Color32::from_rgb(217,38,28)),
                        ("Calc inlet pressure", s.inlet_pressure as f32, Some("mBar"), Color32::from_rgb(0, 145, 64)),
                        ("Req modulating pressure", s.modulating_req_pressure as f32, Some("mBar"), Color32::from_rgb(255,245,0)),
                        ("Req shift pressure", s.shift_req_pressure as f32, Some("mBar"), Color32::from_rgb(0,148,222)),
                        ("Req TCC pressure", s.tcc_pressure as f32, Some("mBar"), Color32::from_rgb(232,120,23)),
                    ],
                    None
                ),
                ChartData::new(
                    "Gearbox Pressures (Output)".into(),
                    vec![
                        ("Corrected modulating pressure", s.corrected_mpc_pressure as f32, Some("mBar"), Color32::from_rgb(255,245,0)),
                        ("Corrected shift pressure", s.corrected_spc_pressure as f32, Some("mBar"), Color32::from_rgb(0,148,222)),
                    ],
                    None
                )]
            },
            LocalRecordData::ShiftMonitorLive(s) => {
                vec![ChartData::new(
                    "RPMs".into(),
                    vec![
                        ("Input speed", s.input_rpm as f32, Some("RPM"), Color32::from_rgb(255, 0, 0)),
                        ("Engine speed", s.engine_rpm as f32, Some("RPM"), Color32::from_rgb(0, 255, 0)),
                    ],
                    None,
                ),
                ChartData::new(
                    "Solenoid pressures".into(),
                    vec![
                        ("Modulating pressure", s.mpc_pressure_mbar as f32, Some("mBar"), Color32::from_rgb(255,245,0)),
                        ("Shift pressure", s.spc_pressure_mbar as f32, Some("mBar"), Color32::from_rgb(0,148,222)),
                        ("TCC pressure", s.tcc_pressure_mbar as f32, Some("mBar"), Color32::from_rgb(232,120,23)),
                    ],
                    None,
                ),
                ChartData::new(
                    "Torque data".into(),
                    vec![
                        ("Static torque", s.engine_torque as f32, Some("Nm"), Color32::from_rgb(0, 0, 255)),
                        ("Input torque (calc)", s.input_torque as f32, Some("Nm"), Color32::from_rgb(0, 255, 0)),
                        ("EGS Req torque", if s.req_engine_torque == i16::MAX { 0.0 } else { s.req_engine_torque as f32 }, Some("Nm"), Color32::from_rgb(255, 0, 0)),
                    ],
                    None,
                )]
            },
            LocalRecordData::ClutchSpeeds(s) => {
                vec![ChartData::new(
                    "RPMs".into(),
                    vec![
                        ("Clutch K1 speed", s.k1 as f32, Some("RPM"), Color32::from_rgb(0, 64, 0)),
                        ("Clutch K2 speed", s.k2 as f32, Some("RPM"), Color32::from_rgb(0, 128, 0)),
                        ("Clutch K3 speed", s.k3 as f32, Some("RPM"), Color32::from_rgb(0, 255, 0)),
                        ("Brake B1 speed", s.b1 as f32, Some("RPM"), Color32::from_rgb(255, 0, 0)),
                        ("Brake B2 speed", s.b2 as f32, Some("RPM"), Color32::from_rgb(255, 128, 0)),
                        ("Brake B3 speed", s.b3 as f32, Some("RPM"), Color32::from_rgb(255, 255, 0)),
                    ],
                    None,
                )]
            },
            LocalRecordData::ShiftAlgoFeedback(s) => {
                vec![ChartData::new(
                    "Clutch speeds".into(),
                    vec![
                        ("On clutch speed", s.s_on as f32, Some("RPM"), Color32::from_rgb(255, 0, 255)),
                        ("Off clutch speed", s.s_off as f32, Some("RPM"), Color32::from_rgb(0, 255, 255)),
                        ("Syncronize speed", s.sync_rpm as f32, Some("RPM"), Color32::from_rgb(255, 0, 0)),
                    ],
                    None),
                    ChartData::new(
                        "Pressures".into(),
                        vec![
                            ("On clutch pressure", s.p_on as f32, Some("mBar"), Color32::from_rgb(255, 0, 255)),
                            ("Off clutch pressure", s.p_off as f32, Some("mBar"), Color32::from_rgb(0, 255, 255)),
                        ],
                    None),
                    ChartData::new(
                        "Phase IDs".into(),
                        vec![
                            ("Sub-Shift phase", s.subphase_shift as f32 + (10.0 * s.shift_phase as f32), None, Color32::from_rgb(255, 0, 255)),
                            ("Sub-Mod phase", s.subphase_mod as f32 + (10.0 * s.subphase_mod as f32), None, Color32::from_rgb(0, 255, 255)),
                        ],
                    None),
                    ChartData::new(
                        "Torques".into(),
                        vec![
                            ("PID torque", s.pid_trq as f32, Some("Nm"), Color32::from_rgb(255, 0, 255)),
                            ("Adder torque", s.adder_trq as f32, Some("Nm"), Color32::from_rgb(255, 0, 255)),
                        ],
                    None),
                ]
            },
            LocalRecordData::TccProgram(s) => {
                vec![
                    ChartData::new(
                        "Clutch Slip".into(),
                        vec![
                            ("Filtered slip", s.slip_filtered as f32, Some("RPM"), Color32::from_rgb(255, 0, 0)),
                            ("Raw slip", s.slip_now as f32, Some("RPM"), Color32::from_rgb(0, 255, 0)),
                            ("Target slip", s.slip_target as f32, Some("RPM"), Color32::from_rgb(0, 0, 255)),
                        ],
                        None,
                    ),
                    ChartData::new(
                        "Pressures".into(),
                        vec![
                            ("Target press.", s.target_pressure as f32, Some("mBar"), Color32::from_rgb(0, 0, 255)),
                            ("Current press.", s.current_pressure as f32, Some("mBar"), Color32::from_rgb(0, 255, 0)),
                        ],
                        None,
                    ),
                    ChartData::new(
                        "Absorbed power".into(),
                        vec![
                            ("Absorbed power", s.tcc_absorbed_joule as f32, Some("J"), Color32::from_rgb(255, 0, 255)),
                        ],
                        None,
                    )
                ]
            },
        }
    }
}

fn tcc_state_to_name(i: u8) -> &'static str {
    match i {
        0 => "Open",
        1 => "Slipping",
        2 => "Closed",
        _ => "Unknown"
    }    
}

#[derive(Debug, Clone, Ord, PartialOrd, Eq, PartialEq, PackedStruct)]
#[packed_struct(endian="lsb")]
pub struct TccProgramData {
    current_pressure: u16,
    target_pressure: u16,
    slip_now: i16,
    slip_filtered: i16,
    slip_target: u16,
    pedal_now: u16,
    pedal_filtered: u16,
    // 0 - Open
    // 1 - Slip
    // 2 - Closed
    targ_state: u8,
    current_state: u8,
    // 0b1 - Open request 
    // 0b01 - Slip request
    can_request_bits: u8,
    engine_output_joule: u32,
    tcc_absorbed_joule: u32
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
    pub tcc_pressure: u16,
    pub on_clutch_pressure: u16,
    pub off_clutch_pressure: u16,
    pub overlap_mod: u16,
    pub overlap_shift: u16,
}

#[derive(Debug, Clone, Ord, PartialOrd, Eq, PartialEq, PackedStruct)]
#[packed_struct(endian="lsb")]
pub struct DataGearboxSensors {
    pub n2_rpm: u16,
    pub n3_rpm: u16,
    pub calculated_rpm: u16,
    pub calc_ratio: u16,
    pub targ_ratio: u16,
    pub v_batt: u16,
    pub atf_temp_c: u32,
    pub parking_lock: u8,
    pub output_rpm: u16
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

impl Display for PaddlePosition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{:?}", self))
    }
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

impl Display for ShifterPosition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{:?}", self))
    }
}

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, PrimitiveEnum_u8)]
pub enum DiagProfileInputState {
    None = 0,
	SwitchTop = 1,
	SwitchBottom = 2,
	ButtonPressed = 3,
	ButtonReleased = 4,
	SLRLeft = 5,
	SLRMiddle = 6,
	SLRRight = 7,
    SNV = 0xFF
}

impl Display for DiagProfileInputState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{:?}", self))
    }
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
    #[packed_field(size_bytes="1", ty="enum")]
    pub profile_input_raw: DiagProfileInputState,
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
    pub targ_act_gear: u8,
    pub profile_id: u8
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

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, PackedStruct)]
#[packed_struct(endian="lsb")]
pub struct DataShiftAlgoFeedback {
    active: u8,
    shift_phase: u8,
    subphase_shift: u8,
    subphase_mod: u8,
    sync_rpm: u16,
    pid_trq: i16,
    adder_trq: i16,
    p_on: u16,
    p_off: u16,
    s_off: i16,
    s_on: i16,
}