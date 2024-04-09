use std::{
    borrow::BorrowMut,
    sync::{Arc, Mutex}, ops::RemAssign,
};

use crate::window::PageAction;
use backend::{
    diag::Nag52Diag, ecu_diagnostics::kwp2000::{ResetType, KwpSessionType},
};
use chrono::{Datelike, Weekday};
use config_app_macros::include_base64;
use eframe::egui::Ui;
use eframe::egui::{self, *};
use egui_extras::RetainedImage;
use image::{DynamicImage, ImageFormat};
use packed_struct::PackedStructSlice;
use strum::IntoEnumIterator;

use self::cfg_structs::{
    BoardType, DefaultProfile, EgsCanType, EngineType, IOPinConfig, MosfetPurpose, ShifterStyle,
    TcmCoreConfig, TcmEfuseConfig,
};

use super::{StatusText};

pub mod cfg_structs;
pub mod egs_config;
pub struct ConfigPage {
    nag: Nag52Diag,
    status: StatusText,
    scn: Option<TcmCoreConfig>,
    efuse: Option<TcmEfuseConfig>,
    show_efuse: bool,
    show_final_warning: bool,
}

impl ConfigPage {
    pub fn new(nag: Nag52Diag) -> Self {
        Self {
            nag,
            status: StatusText::Ok("".into()),
            scn: None,
            efuse: None,
            show_efuse: false,
            show_final_warning: false,
        }
    }
}

impl crate::window::InterfacePage for ConfigPage {
    fn make_ui(&mut self, ui: &mut Ui, frame: &eframe::Frame) -> PageAction {
        ui.heading("TCM Configuration");
        let mut action = PageAction::None;
        if ui.button("Read Configuration").clicked() {
            let _ = self.nag.with_kwp(|server| {
                match server.kwp_read_custom_local_identifier(0xFE) {
                    Ok(res) => {
                        match TcmCoreConfig::unpack_from_slice(&res) {
                            Ok(res) => {
                                self.status = StatusText::Ok(format!("Read OK!"));
                                self.scn = Some(res)
                            },
                            Err(_) => self.status = StatusText::Err(format!("TCM Config size is invalid. Maybe you have mismatched TCU firmware and config app version?"))
                        }
                    }
                    Err(e) => {
                        self.status =
                            StatusText::Err(format!("Error reading TCM configuration: {}", e))
                    }
                }
                match server.kwp_read_custom_local_identifier(0xFD) {
                    Ok(res) => {
                        match TcmEfuseConfig::unpack_from_slice(&res) {
                            Ok(tmp) => {
                                if tmp.board_ver == BoardType::Unknown {
                                    self.show_efuse = true;
                                }
                                self.efuse = Some(tmp);
                            },
                            Err(_) => self.status = StatusText::Err(format!("TCM EFUSE size is invalid. Maybe you have mismatched TCU firmware and config app version?"))
                        }
                    }
                    Err(e) => {
                        self.status =
                            StatusText::Err(format!("Error reading TCM EFUSE configuration: {}", e))
                    }
                }
                Ok(())
            });
        }

        let board_ver = self
            .efuse
            .clone()
            .map(|x| x.board_ver)
            .unwrap_or(BoardType::Unknown);
        if let Some(scn) = self.scn.borrow_mut() {

            ui.hyperlink_to("See getting started for more info", include_base64!("aHR0cDovL2RvY3MudWx0aW1hdGUtbmFnNTIubmV0L2VuL2dldHRpbmdzdGFydGVkI2l2ZS1yZWNlaXZlZC1hbi1hc3NlbWJsZWQtdGN1"));
            ui.hyperlink_to("See Mercedes VIN lookup table for your car configuration", include_base64!("aHR0cDovL2RvY3MudWx0aW1hdGUtbmFnNTIubmV0L2VuL2dldHRpbmdzdGFydGVkL2NvbmZpZ3VyYXRpb24vVklOTGlzdA"));

            egui::Grid::new("DGS").striped(true).show(ui, |ui| {
                let mut x = scn.is_large_nag == 1;
                ui.label("Using large 722.6");
                ui.checkbox(&mut x, "");
                scn.is_large_nag = x as u8;
                ui.end_row();

                let mut curr_profile = scn.default_profile;
                ui.label("Default drive profile");
                egui::ComboBox::from_id_source("profile")
                    .width(100.0)
                    .selected_text(format!("{:?}", curr_profile))
                    .show_ui(ui, |cb_ui| {
                        for dev in DefaultProfile::iter() {
                            cb_ui.selectable_value(
                                &mut curr_profile,
                                dev.clone(),
                                format!("{:?}", dev),
                            );
                        }
                        scn.default_profile = curr_profile
                    });
                ui.end_row();

                ui.label("Differential ratio");
                ui.add(DragValue::new(&mut scn.diff_ratio).speed(0)
                    .custom_formatter(|v, _| format!("{:.2}", v / 1000.0))
                    .custom_parser(|s| {
                        s.parse::<f64>().ok().map(|x| x * 1000.0)
                    })
                    .speed(0)
                );
                ui.end_row();
                ui.label("Wheel circumferance");
                ui.add(DragValue::new(&mut scn.wheel_circumference)
                    .speed(0)
                    .suffix("mm")
                    .max_decimals(0)
                );
                ui.end_row();

                let mut engine = scn.engine_type;
                ui.label("Engine type");
                egui::ComboBox::from_id_source("engine_type")
                    .width(100.0)
                    .selected_text(format!("{:?}", engine))
                    .show_ui(ui, |cb_ui| {
                        let profiles = vec![EngineType::Diesel, EngineType::Petrol];
                        for dev in profiles {
                            cb_ui.selectable_value(&mut engine, dev.clone(), format!("{:?}", dev));
                        }
                        scn.engine_type = engine
                    });
                ui.end_row();

                let rpm_mut = match scn.engine_type {
                    EngineType::Diesel => &mut scn.red_line_dieselrpm,
                    EngineType::Petrol => &mut scn.red_line_petrolrpm
                };
                ui.label("Engine redline RPM");
                ui.add(DragValue::new(rpm_mut)
                    .clamp_range(3000..=10000)
                    .speed(0)
                    .suffix("RPM")
                    .max_decimals(0)
                );
                ui.end_row();

                let mut x = scn.is_four_matic == 1;
                ui.label("Four matic");
                ui.checkbox(&mut x, "");
                scn.is_four_matic = (x as u8);
                ui.end_row();

                if scn.is_four_matic == 1 {
                    ui.label("Transfer case high ratio");
                    ui.add(DragValue::new(&mut scn.transfer_case_high_ratio).speed(0)
                    .custom_formatter(|v, _| format!("{:.2}", v / 1000.0))
                    .custom_parser(|s| {
                        s.parse::<f64>().ok().map(|x| x * 1000.0)
                    })
                    .speed(0)
                );
                    ui.end_row();
                    ui.label("Transfer case low ratio");
                    ui.add(DragValue::new(&mut scn.transfer_case_low_ratio).speed(0)
                    .custom_formatter(|v, _| format!("{:.2}", v / 1000.0))
                    .custom_parser(|s| {
                        s.parse::<f64>().ok().map(|x| x * 1000.0)
                    })
                    .speed(0)
                );
                    ui.end_row();
                }

                ui.label("Engine drag torque");
                ui.add(DragValue::new(&mut scn.engine_drag_torque).speed(0)
                    .custom_formatter(|v, _| format!("{:.1}", v / 10.0))
                    .suffix("Nm")
                    .custom_parser(|s| {
                        s.parse::<f64>().ok().map(|x| x * 10.0)
                    })
                    .speed(0)
                );
                ui.end_row();

                ui.label("EGS CAN Layer: ");
                let mut can = scn.egs_can_type;
                egui::ComboBox::from_id_source("can_layer")
                    .width(100.0)
                    .selected_text(format!("{:?}", can))
                    .show_ui(ui, |cb_ui| {
                        let layers = match board_ver {
                            BoardType::Unknown | BoardType::V11 => {
                                vec![EgsCanType::UNKNOWN, EgsCanType::EGS52, EgsCanType::EGS53]
                            }
                            _ => EgsCanType::iter().collect()
                        };
                        for layer in layers {
                            cb_ui.selectable_value(&mut can, layer.clone(), format!("{:?}", layer));
                        }
                        scn.egs_can_type = can
                    });
                ui.end_row();
                if can == EgsCanType::CUSTOM_ECU {
                    ui.strong("Custom ECU CAN is experimental! - It requires implementation on the ECU Side");
                    ui.hyperlink_to("Read more", "https://docs.ultimate-nag52.net/en/advanced/custom-can");
                    ui.end_row();
                }

                if board_ver == BoardType::V12 || board_ver == BoardType::V13 {
                    // 1.2 or 1.3 config
                    ui.label("Shifter style: ");
                    let mut ss = scn.shifter_style;
                    egui::ComboBox::from_id_source("shifter_style")
                        .width(200.0)
                        .selected_text(format!("{:?}", ss))
                        .show_ui(ui, |cb_ui| {
                            for o in ShifterStyle::iter() {
                                cb_ui.selectable_value(&mut ss, o.clone(), format!("{:?}", o));
                            }
                            scn.shifter_style = ss
                        });
                    ui.end_row();
                }

                if board_ver == BoardType::V13 {
                    // Only v1.3 config
                    ui.label("GPIO usage: ");
                    let mut ss = scn.io_0_usage;
                    egui::ComboBox::from_id_source("gpio_usage")
                        .width(200.0)
                        .selected_text(format!("{:?}", ss))
                        .show_ui(ui, |cb_ui| {
                            for o in IOPinConfig::iter() {
                                cb_ui.selectable_value(&mut ss, o.clone(), format!("{:?}", o));
                            }
                            scn.io_0_usage = ss
                        });
                    ui.end_row();

                    if scn.io_0_usage == IOPinConfig::Input {
                        ui.label("Input sensor pulses/rev");
                        ui.add(DragValue::new(&mut scn.input_sensor_pulses_per_rev)
                            .clamp_range(0..=0xFF)
                            .speed(0)
                            .suffix("/rev")
                            .max_decimals(0)
                        );
                        ui.end_row();
                    } else if scn.io_0_usage == IOPinConfig::Output {
                        ui.label("Pulse width (μs) per kmh");
                        ui.add(DragValue::new(&mut scn.output_pulse_width_per_kmh)
                            .clamp_range(0..=0xFF)
                            .speed(0)
                            .suffix("μs/kmh")
                            .max_decimals(0)
                        );
                        ui.end_row();
                    }
                    ui.label("General MOSFET usage: ");
                    let mut ss = scn.mosfet_purpose;
                    egui::ComboBox::from_id_source("mosfet_purpose")
                        .width(200.0)
                        .selected_text(format!("{:?}", ss))
                        .show_ui(ui, |cb_ui| {
                            for o in MosfetPurpose::iter() {
                                cb_ui.selectable_value(&mut ss, o.clone(), format!("{:?}", o));
                            }
                            scn.mosfet_purpose = ss
                        });
                    ui.end_row();
                }
            });

            if ui.button("Write SCN configuration").clicked() {
                match {
                    let mut x: Vec<u8> = vec![0x3B, 0xFE];
                    x.extend_from_slice(&scn.clone().pack_to_vec().unwrap());
                    self.nag.with_kwp(|server| {
                        server.kwp_set_session(KwpSessionType::Reprogramming.into())?;
                        server.send_byte_array_with_response(&x)?;
                        server.kwp_reset_ecu(ResetType::PowerOnReset.into())?;
                        Ok(())
                    })
                } {
                    Ok(_) => {
                        action = PageAction::SendNotification { text: "Configuration applied!".into(), kind: egui_toast::ToastKind::Success }
                    },
                    Err(e) => {
                        action = PageAction::SendNotification { 
                            text: format!("Configuration failed to apply: {e}"), 
                            kind: egui_toast::ToastKind::Error 
                        }
                    }
                }
            }
            ui.strong("What to do next?");
            if ui.button("Configure EGS compatibility data").clicked() {
                action = PageAction::Add(Box::new(
                    egs_config::EgsConfigPage::new(self.nag.clone())
                ))
            }
        }

        if let Some(efuse) = self.efuse.borrow_mut() {
            if self.show_efuse {
                ui.heading("EFUSE CONFIG");
                ui.label("IMPORTANT! This can only be set once! Be careful!");
                ui.spacing();
                let mut ver = efuse.board_ver;
                ui.label("Choose board variant: ");
                egui::ComboBox::from_id_source("board_ver")
                    .width(100.0)
                    .selected_text(format!("{:?}", efuse.board_ver))
                    .show_ui(ui, |cb_ui| {
                        let profiles = vec![BoardType::V11, BoardType::V12, BoardType::V13];
                        for dev in profiles.iter() {
                            cb_ui.selectable_value(&mut ver, dev.clone(), dev.to_string()).on_hover_ui(|ui| {
                                if let Some(img) = dev.image_source() {
                                    ui.add(Image::new(img));
                                }
                            });
                        }
                        efuse.board_ver = ver
                    });
            }
            if self.show_efuse && efuse.board_ver != BoardType::Unknown {
                if ui.button("Write EFUSE configuration").clicked() {
                    self.show_final_warning = true;
                }
            }
        }

        let mut tmp = self.show_final_warning;

        let ss = ui.ctx().input(|x| x.screen_rect());
        let mut reload = false;
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
                        let mut efuse = self.efuse.clone().unwrap();
                        let date = chrono::Utc::now().date_naive();
                        efuse.manf_day = date.day() as u8;
                        efuse.manf_week = date.iso_week().week() as u8;
                        efuse.manf_month = date.month() as u8;
                        efuse.manf_year = (date.year() - 2000) as u8;

                        let mut x = vec![0x3Bu8, 0xFD];
                        x.extend_from_slice(&efuse.pack_to_vec().unwrap());
                        match self.nag.with_kwp(|server| {
                            server.kwp_set_session(KwpSessionType::Reprogramming.into())?;
                            server.send_byte_array_with_response(&x)?;
                            server.kwp_reset_ecu(ResetType::PowerOnReset.into())?;
                            Ok(())
                        }) {
                            Ok(_) => {
                                action = PageAction::SendNotification { text: "EFUSE applied!".into(), kind: egui_toast::ToastKind::Success }
                            },
                            Err(e) => {
                                action = PageAction::SendNotification { 
                                    text: format!("EFUSE failed to apply: {e}"), 
                                    kind: egui_toast::ToastKind::Error 
                                }
                            }
                        }
                        tmp = false;
                    }
                })
            });
        if reload {
            *self = Self::new(self.nag.clone());
        }
        self.show_final_warning = tmp;

        ui.add(self.status.clone());
        action
    }

    fn get_title(&self) -> &'static str {
        "Configuration"
    }

    fn should_show_statusbar(&self) -> bool {
        true
    }
}
