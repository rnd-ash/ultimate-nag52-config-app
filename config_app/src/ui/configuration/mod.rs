use std::{borrow::BorrowMut, sync::Arc};

use crate::{window::PageAction};
use backend::{
    diag::{DataState, Nag52Diag}, ecu_diagnostics::kwp2000::{KwpSessionType, ResetType},
};
use chrono::Datelike;
use config_app_macros::include_base64;
use eframe::egui::{Ui, mutex::RwLock};
use eframe::egui::{self, *};
use packed_struct::{PackedStructSlice, PackingError};
use strum::IntoEnumIterator;

use self::cfg_structs::{
    BoardType, DefaultProfile, EgsCanType, EngineType, IOPinConfig, MosfetPurpose, ShifterStyle,
    TcmCoreConfig, TcmEfuseConfig,
};

pub mod cfg_structs;
pub mod egs_config;
pub struct ConfigPage {
    nag: Nag52Diag,
    scn: Arc<RwLock<DataState<(TcmCoreConfig, bool)>>>,
    efuse: Arc<RwLock<DataState<(bool, TcmEfuseConfig)>>>,
    show_final_warning: bool,
}

impl ConfigPage {

    pub fn query(nag: Nag52Diag, scn: Option<Arc<RwLock<DataState<(TcmCoreConfig, bool)>>>>, efuse: Option<Arc<RwLock<DataState<(bool, TcmEfuseConfig)>>>>) {
        if let Some(scn) = scn.as_ref() {
            *scn.write() = DataState::Unint;
        }
        if let Some(efuse) = efuse.as_ref() {
            *efuse.write() = DataState::Unint;
        }
        std::thread::spawn(move|| {
            let _ = nag.with_kwp(|server| {
                if let Some(scn) = scn.as_ref() {
                    match server.kwp_read_custom_local_identifier(0xFE) {
                        Ok(res) => {
                            match TcmCoreConfig::unpack_from_slice(&res) {
                                Ok(res) => {
                                    *scn.write() = DataState::LoadOk((res, false));
                                },
                                Err(e) => {
                                    if e == PackingError::InvalidValue {
                                        let new_cfg = TcmCoreConfig::default();
                                        *scn.write() = DataState::LoadOk((new_cfg, true));
                                    } else {
                                        *scn.write() = DataState::LoadErr(format!("TCU Config unpack error: {e}"));
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            *scn.write() = DataState::LoadErr(format!("TCU config request error: {e}"));
                        }
                    }
                }
                if let Some(efuse) = efuse.as_ref() {
                    match server.kwp_read_custom_local_identifier(0xFD) {
                        Ok(res) => {
                            match TcmEfuseConfig::unpack_from_slice(&res) {
                                Ok(tmp) => {
                                    let req_set = tmp.board_ver == BoardType::Unknown;
                                    *efuse.write() = DataState::LoadOk((req_set, tmp));
                                },
                                Err(e) => {
                                    *efuse.write() = DataState::LoadErr(format!("TCU EFUSE unpack error: {e}"));
                                }
                            }
                        }
                        Err(e) => {
                            *efuse.write() = DataState::LoadErr(format!("TCU EFUSE request error: {e}"));
                        }
                    }
                }
                Ok(())
            });
        });
    }

    pub fn write_scn(nag: Nag52Diag, new_scn: TcmCoreConfig, scn: Arc<RwLock<DataState<(TcmCoreConfig, bool)>>>) {
        *scn.write() = DataState::Unint;
        std::thread::spawn(move|| {
            match {
                let mut x: Vec<u8> = vec![0x3B, 0xFE];
                x.extend_from_slice(&new_scn.clone().pack_to_vec().unwrap());
                nag.with_kwp(|server| {
                    server.kwp_set_session(KwpSessionType::Reprogramming.into())?;
                    server.send_byte_array_with_response(&x, None)?;
                    server.kwp_reset_ecu(ResetType::PowerOnReset.into())?;
                    Ok(())
                })
            } {
                Ok(_) => {
                    Self::query(nag, Some(scn), None);
                },
                Err(e) => {
                    *scn.write() = DataState::LoadErr(format!("Error writing efuse: {e}"));
                }
            }
        });
    }

    pub fn write_efuse(
        nag: Nag52Diag, 
        new_efuse: TcmEfuseConfig,
        scn: Arc<RwLock<DataState<(TcmCoreConfig, bool)>>>, 
        efuse: Arc<RwLock<DataState<(bool, TcmEfuseConfig)>>>

    ) {
        *scn.write() = DataState::Unint;
        *efuse.write() = DataState::Unint;
        std::thread::spawn(move|| {
            let mut x = vec![0x3Bu8, 0xFD];
            x.extend_from_slice(&new_efuse.pack_to_vec().unwrap());
            match nag.with_kwp(|server| {
                server.kwp_set_session(KwpSessionType::Reprogramming.into())?;
                server.send_byte_array_with_response(&x, None)?;
                server.kwp_reset_ecu(ResetType::PowerOnReset.into())?;
                Ok(())
            }) {
                Ok(_) => {
                    Self::query(nag, Some(scn), Some(efuse));
                },
                Err(e) => {
                    *efuse.write() = DataState::LoadErr(format!("Error writing efuse: {e}"));
                }
            }
        });
    }

    pub fn new(nag: Nag52Diag) -> Self {
        // Try and read both efuse and config on launch

        let scn = Arc::new(RwLock::new(DataState::Unint));
        let scn_t = scn.clone();
        let efuse = Arc::new(RwLock::new(DataState::Unint));
        let efuse_t = efuse.clone();

        let nag_t = nag.clone();
        Self::query(nag_t, Some(scn_t), Some(efuse_t));
        Self {
            nag,
            scn,
            efuse,
            show_final_warning: false,
        }
    }
}

impl crate::window::InterfacePage for ConfigPage {
    fn make_ui(&mut self, ui: &mut Ui) -> PageAction {
        let mut efuse_now = self.efuse.read().clone();
        let mut config_now = self.scn.read().clone();

        let board_ver = efuse_now.data()
            .map(|(unset, x)| if *unset { BoardType::Unknown } else {x.board_ver})
            .unwrap_or(BoardType::Unknown);

        ui.heading("Core Vehicle config");
        ui.separator();
        ui.hyperlink_to("See getting started for more info", include_base64!("aHR0cDovL2RvY3MudWx0aW1hdGUtbmFnNTIubmV0L2VuL2dldHRpbmdzdGFydGVkI2l2ZS1yZWNlaXZlZC1hbi1hc3NlbWJsZWQtdGN1"));
        ui.hyperlink_to("See Mercedes VIN lookup table for your car configuration", include_base64!("aHR0cDovL2RvY3MudWx0aW1hdGUtbmFnNTIubmV0L2VuL2dldHRpbmdzdGFydGVkL2NvbmZpZ3VyYXRpb24vVklOTGlzdA"));
        let mut can_apply = true;
            match config_now.borrow_mut() {
                DataState::LoadOk((data, was_reset)) => {
                    if *was_reset {
                        ui.colored_label(Color32::RED, "TCM Config was corrupt and thus reset");
                    }
                    ui.add_enabled_ui(BoardType::Unknown != board_ver, |ui| {
                    egui::Grid::new("DGS").striped(true).show(ui, |ui| {
                    let mut curr_profile = data.default_profile;
                    ui.strong("Default drive profile");
                    egui::ComboBox::new("profile", "")
                        .selected_text(curr_profile.to_string())
                        .width(100.0)
                        .show_ui(ui, |cb_ui| {
                            for dev in DefaultProfile::iter() {
                                cb_ui.selectable_value(
                                    &mut curr_profile,
                                    dev.clone(),
                                    dev.to_string(),
                                );
                            }
                            data.default_profile = curr_profile
                        });
                    ui.end_row();

                    ui.strong("Differential ratio")
                        .on_hover_text("
Used to calculate rear output shaft speed. Consult the wiki.

CAUTION: Some cars have multiple ratios available. Higher ratio is
typically for USA only
                    ");
                    ui.add(DragValue::new(&mut data.diff_ratio).speed(0)
                        .custom_formatter(|v, _| format!("{:.2}", v / 1000.0))
                        .custom_parser(|s| {
                            s.parse::<f64>().ok().map(|x| x * 1000.0)
                        })
                        .speed(0)
                    );
                    ui.end_row();
                    ui.strong("Wheel circumferance").on_hover_text("Used to calculate vehicle speed");
                    ui.add(DragValue::new(&mut data.wheel_circumference)
                        .speed(0)
                        .suffix("mm")
                        .max_decimals(0)
                    );
                    ui.end_row();

                    let mut engine = data.engine_type;
                    ui.strong("Engine type");
                    egui::ComboBox::new("engine_type", "")
                        .width(100.0)
                        .selected_text(engine.to_string())
                        .show_ui(ui, |cb_ui| {
                            let profiles = vec![EngineType::Diesel, EngineType::Petrol];
                            for dev in profiles {
                                cb_ui.selectable_value(&mut engine, dev.clone(), dev.to_string());
                            }
                            data.engine_type = engine
                        });
                    ui.end_row();

                    let rpm_mut = match data.engine_type {
                        EngineType::Diesel => &mut data.red_line_dieselrpm,
                        EngineType::Petrol => &mut data.red_line_petrolrpm
                    };
                    ui.strong("Engine redline RPM").on_hover_text("The maximum engine speed allowed");
                    ui.add(DragValue::new(rpm_mut)
                        .range(3000..=10000)
                        .speed(0)
                        .suffix("RPM")
                        .max_decimals(0)
                    );
                    ui.end_row();

                    let mut x = data.is_four_matic == 1;
                    ui.strong("Four matic");
                    ui.checkbox(&mut x, "");
                    data.is_four_matic = x as u8;
                    ui.end_row();

                    if data.is_four_matic == 1 {
                        ui.strong("Transfer case high ratio");
                        ui.add(DragValue::new(&mut data.transfer_case_high_ratio).speed(0)
                        .custom_formatter(|v, _| format!("{:.2}", v / 1000.0))
                        .custom_parser(|s| {
                            s.parse::<f64>().ok().map(|x| x * 1000.0)
                        })
                        .speed(0)
                    );
                        ui.end_row();
                        ui.strong("Transfer case low ratio");
                        ui.add(DragValue::new(&mut data.transfer_case_low_ratio).speed(0)
                        .custom_formatter(|v, _| format!("{:.2}", v / 1000.0))
                        .custom_parser(|s| {
                            s.parse::<f64>().ok().map(|x| x * 1000.0)
                        })
                        .speed(0)
                    );
                        ui.end_row();
                    }

                    ui.strong("Engine Inertia").on_hover_text("Consult the wiki");
                    ui.add(DragValue::new(&mut data.engine_drag_torque).speed(0)
                        .custom_formatter(|v, _| format!("{:.1}", v / 10.0))
                        .suffix("Nm")
                        .custom_parser(|s| {
                            s.parse::<f64>().ok().map(|x| x * 10.0)
                        })
                        .speed(0)
                    );
                    ui.end_row();

                    ui.strong("EGS CAN Layer").on_hover_text(
"This is the CAN Layer (NOT EGS VERSION) the car uses. In 90% of cases, you
can use the one that matches your original EGS version, but some cars have
newer EGS TCU's running older CAN layers. Consult the wiki for more information"
                    );
                    let mut can = data.egs_can_type;
                    egui::ComboBox::new("can_layer","")
                        .width(100.0)
                        .selected_text(can.to_string())
                        .show_ui(ui, |cb_ui| {
                            let layers = match board_ver {
                                BoardType::Unknown | BoardType::V11 => {
                                    vec![EgsCanType::Unknown, EgsCanType::Egs52, EgsCanType::Egs53]
                                }
                                _ => EgsCanType::iter().collect()
                            };
                            for layer in layers {
                                cb_ui.selectable_value(&mut can, layer.clone(), layer.to_string());
                            }
                            data.egs_can_type = can
                        });
                    ui.end_row();
                    let mut x = data.jeep_chrysler;
                    ui.strong("Vehicle is a Jeep/Chrysler car").on_hover_text(
"Jeep / Chrysler cars require special IO handling"
                    );
                    ui.checkbox(&mut x, "");
                    data.jeep_chrysler = x;
                    ui.end_row();

                    if board_ver == BoardType::V12 || board_ver == BoardType::V13 {
                        // 1.2 or 1.3 config
                        ui.strong("Shifter style");
                        let mut ss = data.shifter_style;
                        egui::ComboBox::new("shifter_style", "")
                            .width(200.0)
                            .selected_text(ss.to_string())
                            .show_ui(ui, |cb_ui| {
                                for o in ShifterStyle::iter() {
                                    cb_ui.selectable_value(&mut ss, o.clone(), o.to_string());
                                }
                                data.shifter_style = ss
                            });
                        ui.end_row();
                    }

                    if board_ver == BoardType::V13 {
                        // Only v1.3 config
                        ui.strong("GPIO usage");
                        let mut ss = data.io_0_usage;
                        egui::ComboBox::new("gpio_usage", "")
                            .width(200.0)
                            .selected_text(ss.to_string())
                            .show_ui(ui, |cb_ui| {
                                cb_ui.selectable_value(&mut ss, IOPinConfig::NotConnected, IOPinConfig::NotConnected.to_string());
                                cb_ui.selectable_value(&mut ss, IOPinConfig::Input, IOPinConfig::Input.to_string());
                                cb_ui.selectable_value(&mut ss, IOPinConfig::Output, IOPinConfig::Output.to_string());
                                if board_ver == BoardType::V13 {
                                    cb_ui.selectable_value(&mut ss, IOPinConfig::TCCMod13, IOPinConfig::TCCMod13.to_string());
                                }
                                data.io_0_usage = ss
                            });
                        ui.end_row();

                        if data.io_0_usage == IOPinConfig::Input {
                            ui.strong("Input sensor pulses/rev");
                            ui.add(DragValue::new(&mut data.input_sensor_pulses_per_rev)
                                .range(0..=0xFF)
                                .speed(0)
                                .suffix("/rev")
                                .max_decimals(0)
                            );
                            ui.end_row();
                        } else if data.io_0_usage == IOPinConfig::Output {
                            ui.strong("Pulse width (μs) per kmh");
                            ui.add(DragValue::new(&mut data.output_pulse_width_per_kmh)
                                .range(0..=0xFF)
                                .speed(0)
                                .suffix("μs/kmh")
                                .max_decimals(0)
                            );
                            ui.end_row();
                        } else if data.io_0_usage == IOPinConfig::TCCMod13 {
                            // TODO
                        }
                        ui.strong("General MOSFET usage");
                        let mut ss = data.mosfet_purpose;
                        egui::ComboBox::new("mosfet_purpose", "")
                            .width(200.0)
                            .selected_text(ss.to_string())
                            .show_ui(ui, |cb_ui| {
                                for o in MosfetPurpose::iter() {
                                    cb_ui.selectable_value(&mut ss, o.clone(), o.to_string());
                                }
                                data.mosfet_purpose = ss
                            });
                        ui.end_row();
                    }
                    *self.scn.write() = DataState::LoadOk((data.clone(), *was_reset));
                });

                if data.diff_ratio == 0 {
                    ui.colored_label(Color32::RED, "Diff ratio cannot be 0");
                    can_apply = false;
                }
                if data.engine_drag_torque == 0 {
                    ui.colored_label(Color32::RED, "Engine Inertia cannot be 0");
                    can_apply = false;
                }
                if data.wheel_circumference == 0 {
                    ui.colored_label(Color32::RED, "Tyre size cannot be 0");
                    can_apply = false;
                }
                if can_apply {
                     ui.colored_label(Color32::GREEN, "Vehicle configuration OK");
                }

                ui.add_enabled_ui(can_apply, |ui| {
                    if ui.button("Apply configuration").clicked() {
                        Self::write_scn(self.nag.clone(), data.clone(), self.scn.clone());
                    }
                });
            });
        },
        DataState::Unint => {
            ui.spinner();
        },
        DataState::LoadErr(err) => {
            ui.colored_label(Color32::RED, err);
            if ui.button("Try again").clicked() {
                Self::query(self.nag.clone(), Some(self.scn.clone()), None);
            }
        },
    }

    ui.add_space(20.0);
    ui.heading("EFUSE");
    ui.separator();
    match efuse_now.borrow_mut() {
        DataState::LoadOk((req_set, fuse)) => {
            if *req_set {
                ui.label("You have a freshly made board. You must set the board version");
                ui.strong("Caution: You can only do this once");
                egui::ComboBox::new("board-ver-sel", "Choose board variant")
                    .width(100.0)
                    .selected_text(fuse.board_ver.to_string())
                    .show_ui(ui, |cb_ui| {
                        let profiles = vec![BoardType::V11, BoardType::V12, BoardType::V13];
                        for dev in profiles.iter() {
                            cb_ui.selectable_value(&mut fuse.board_ver, dev.clone(), dev.to_string()).on_hover_ui(|ui| {
                                if let Some(imgs) = dev.image_source() {
                                    ui.horizontal(|row| {
                                        for img in imgs {
                                            row.add(Image::new(img));
                                        }
                                    });
                                }
                            });
                        }
                    });
                *self.efuse.write() = DataState::LoadOk((*req_set, fuse.clone()));
                ui.add_enabled_ui(*req_set && fuse.board_ver != BoardType::Unknown, |ui| {
                    if ui.button("Write EFUSE configuration").clicked() {
                        self.show_final_warning = true;
                    }
                });
            } else {
                // Just show table of current values
                ui.label("Configured EFUSE Data (Read only)");
                Grid::new("efuse-g").striped(true)
                .show(ui, |ui| {
                    ui.strong("PCB Version");
                    ui.label(format!(
                        "{}",
                        fuse.board_ver
                    ));
                    ui.end_row();

                    ui.strong("Production date");
                    ui.label(format!(
                        "{}/{}/20{}",
                        fuse.manf_day, fuse.manf_month, fuse.manf_year
                    ));
                    ui.end_row();
                });

            }
        },
        DataState::Unint => {
            ui.spinner();
        },
        DataState::LoadErr(e) => {
            ui.colored_label(Color32::RED, e);
            if ui.button("Try again").clicked() {
                Self::query(self.nag.clone(), None, Some(self.efuse.clone()));
            }
        },
    }

        let mut tmp = self.show_final_warning;

        let ss = ui.ctx().content_rect();
        let reload = false;
        egui::Window::new("ARE YOU SURE?")
            .open(&mut self.show_final_warning)
            .fixed_pos(Pos2::new(ss.size().x / 2.0, ss.size().y / 2.0))
            .show(ui.ctx(), |win| {
                win.label("EFUSE CONFIGURATION CANNOT BE UN-DONE");
                win.label(
                    "Please double check and ensure you have selected the right board variant!",
                );
                win.horizontal(|row| {
                    if row.button("Take me back").clicked() {
                        tmp = false;
                    }
                    if row.button("Yes, I am sure!").clicked() {
                        let (_, mut efuse) = efuse_now.data().unwrap().clone();
                        let date = chrono::Utc::now().date_naive();
                        efuse.manf_day = date.day() as u8;
                        efuse.manf_week = date.iso_week().week() as u8;
                        efuse.manf_month = date.month() as u8;
                        efuse.manf_year = (date.year() - 2000) as u8;
                        Self::write_efuse(self.nag.clone(), efuse, self.scn.clone(), self.efuse.clone());
                        tmp = false;
                    }
                })
            });
        if reload {
            *self = Self::new(self.nag.clone());
        }
        self.show_final_warning = tmp;
        PageAction::None
    }

    fn should_show_statusbar(&self) -> bool {
        true
    }
}
