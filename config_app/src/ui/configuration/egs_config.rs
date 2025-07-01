use std::{borrow::{Borrow, BorrowMut}, cmp::min, fs::File, io::{Read, Write}, mem::size_of, thread::JoinHandle};

use config_app_macros::include_base64;
use eframe::egui::{Color32, Grid, Label, RichText, ScrollArea, Window};
use egui_extras::Column;
use egui_toast::ToastKind;
use packed_struct::PackedStructSlice;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::window::{InterfacePage, PageAction};

use backend::{diag::{calibration::*, memory::MemoryRegion, Nag52Diag}, ecu_diagnostics::{kwp2000::KwpSessionType, DiagError, DiagServerResult}, serde_yaml};


const EGS_DB_BYTES: &[u8] = include_bytes!("../../../../egs_db.bin"); 

pub enum CalibrationSection {
    Hyraulic,
    Mechanical,
    TorqueConverter,
    ShiftAlgo
}

#[derive(Debug, Clone)]
pub struct EgsLinkedData {
    pub pn: String,
    pub gb: String,
    pub chassis: String,
    pub tcc: String,
    pub mech: String,
    pub hydr: String,
    pub shift_algo: String
}

pub struct EgsConfigPage {
    pub db: Result<CalibrationDatabase, String>,
    pub linked: Result<Vec<EgsLinkedData>, String>,
    pub viewing_cal: Option<EgsLinkedData>,
    pub calibration_contents: Result<Vec<u8>, String>,
    pub res: Option<JoinHandle<Result<Vec<u8>, String>>>,
    pub nag: Nag52Diag,
    pub gb_input: String,
    pub egs_pn: String,
    pub chassis_input: String,
    pub editing_cal: Option<CalibrationSection>
}

fn sign_and_crc(egs: &mut EgsStoredCalibration) {
    let egs_bytes = &egs.pack_to_vec().unwrap()[8..];
    // CRC check
    let mut crc: u16 = 0;
    for i in 0..egs_bytes.len() {
        let b = egs_bytes[i];
        crc = crc.wrapping_add(b as u16);
        crc = crc.wrapping_add(i as u16);
    }
    egs.crc = crc;
    egs.len = EgsStoredCalibration::packed_bytes_size(None).unwrap() as u16;
    println!("Size W {}", egs.len);
    egs.magic = 0xDEADBEEF;
}

impl EgsConfigPage {
    pub fn new(nag: Nag52Diag) -> Self {
        let db = match lz4_compression::decompress::decompress(EGS_DB_BYTES).map_err(|e| {
            match e {
                lz4_compression::decompress::Error::UnexpectedEnd => "LZ4 decompress failed. Unexpected End",
                lz4_compression::decompress::Error::InvalidDeduplicationOffset => "LZ4 decompress failed. Invalid deduplication offset",
            }.to_string()
        }) {
            Err(e) => Err(e),
            Ok(bytes) => {
                match bincode::serde::decode_from_slice::<CalibrationDatabase, _>(&bytes, bincode::config::legacy()) {
                    Ok((d, _)) => Ok(d),
                    Err(e) => {
                        Err(e.to_string())
                    }
                }
            }
        };

        let linked = match &db {
            Err(e) => Err(e.clone()),
            Ok(db) => {
                let mut ret: Vec<EgsLinkedData> = Vec::new();

                for egs in &db.egs_list {
                    for cal in &egs.chassis {
                        ret.push(EgsLinkedData {
                            pn: egs.pn.clone(),
                            gb: cal.gearbox.clone(),
                            chassis: cal.chassis.clone(),
                            tcc: cal.tcc_cfg.clone(),
                            mech: cal.mech_cfg.clone(),
                            hydr: cal.hydr_cfg.clone(),
                            shift_algo: cal.shift_algo_cfg.clone()
                        });
                    }
                }
                Ok(ret)
            }
        };


        let nag_c = nag.clone();
        let r = std::thread::spawn(move || {
            let len = EgsStoredCalibration::packed_bytes_size(None).unwrap() as u32;
            let mut i = 0;
            let mut res: Vec<u8> = Vec::new();
            let size = match nag_c.with_kwp(|kwp| {
                kwp.kwp_set_session(KwpSessionType::ExtendedDiagnostics.into())?;
                kwp.kwp_read_custom_local_identifier(0xFB)
            }) {
                Ok(res) => {
                    let size = u16::from_le_bytes(res.try_into().unwrap());
                    if size != len as u16 {
                        Err("Mismatch calibration size! Either your Firmware or Configuration app is out of date".to_string())
                    } else {
                        Ok(size)
                    }
                },
                Err(DiagError::ECUError { code, def }) => {
                    Err("TCU does not support calibration. Please update firmware".to_string())
                },
                Err(e) => {
                    Err(format!("Error trying to download flash contents. {e}"))
                }
            };
            if size.is_ok() {
                while i < len {
                    let read = min(0xFE, len - i);
                    match nag_c.read_memory(MemoryRegion::EgsCalibration, i, read as u8) {
                        Ok(c) => {
                            res.extend_from_slice(&c[1..]);
                        },
                        Err(e) => {
                            return Err(format!("Error downloading flash contents. {e}"));
                        }
                    }
                    i += read;
                }
                Ok(res)
            } else {
                Err(size.err().unwrap())
            }
        });

        Self {
            db,
            linked,
            viewing_cal: None,
            nag,
            calibration_contents: Ok(Vec::new()),
            res: Some(r),
            gb_input: String::default(),
            egs_pn: String::default(),
            chassis_input: String::default(),
            editing_cal: None
        }
    }

    pub fn write_calibration(nag: Nag52Diag, out_bytes: Vec<u8>) {
        std::thread::spawn(move|| {
            let mut written = 0;
            while written < out_bytes.len() {
                let block_size = min(250, out_bytes.len() - written);
                match nag.write_memory(MemoryRegion::EgsCalibration, written as u32, &out_bytes[written..written+block_size]) {
                    Ok(_) => {
                        println!("Write block OK!");
                        written += block_size as usize;
                    },
                    Err(e) => {
                        println!("Write block failed {e}");
                        break;
                    },
                }
            }
            if written == out_bytes.len() {
                println!("Write complete");
                nag.with_kwp(|kwp| kwp.kwp_reset_ecu(backend::ecu_diagnostics::kwp2000::ResetType::PowerOnReset));
            }
        });
    }
}

impl InterfacePage for EgsConfigPage {
    fn make_ui(&mut self, ui: &mut eframe::egui::Ui, frame: &eframe::Frame) -> crate::window::PageAction {
        let mut action = PageAction::None;
        let mut take = false;
        if let Some(h) = self.res.borrow_mut() {
            if h.is_finished() {
                take = true;
            } else {
                ui.strong("Please wait...");
                ui.spinner();
                return PageAction::None;
            }
        }
        if take {
            self.calibration_contents = self.res.take().unwrap().join().unwrap()
        }

        if let Err(e) = &self.calibration_contents {
            ui.vertical_centered(|ui| {
                ui.strong("Failed to initialize calibration");
                ui.label(e);
            });
        } else if let Err(e) = self.db.borrow() {
            ui.vertical_centered(|ui| {
                ui.strong("Database decode failed");
                ui.label(e)
            });
        } else {
            let flash = self.calibration_contents.as_mut().unwrap();
            let mut interpreted = EgsStoredCalibration::unpack_from_slice(&flash).unwrap();
            let db = self.db.as_ref().unwrap();
            // Show calibrations that are not valid
            ui.hyperlink_to("Watch tutorial video for help", include_base64!("aHR0cHM6Ly95b3V0dS5iZS9ENlZmNWlqekpndw"));
            ui.strong("Status of calibration data (Your TCU):");
            let mut error_counter = 0;
            match String::from_utf8(interpreted.hydr_cal_name.to_vec()) {
                Ok(name) => {
                    let parts_maybe = name.split(".").collect::<Vec<&str>>();
                    ui.horizontal(|row| {
                        if parts_maybe.len() == 2 && parts_maybe[0].starts_with("A") {
                            // MB calibration
                            row.colored_label(Color32::GREEN,  format!("Hydraulic calibration: '{}' from {}", parts_maybe[1], parts_maybe[0]));
                        } else {
                            row.colored_label(Color32::GREEN,  format!("Custom Hydraulic calibration in use: '{name}'"));
                        }
                        if row.button("Load/Save from file").clicked() {
                            self.editing_cal = Some(CalibrationSection::Hyraulic)
                        }
                    });
                },
                Err(_) => {
                    error_counter += 1;
                    ui.colored_label(Color32::RED, "Hydraulic calibration NOT FOUND");
                }
            };
            match String::from_utf8(interpreted.mech_cal_name.to_vec()) {
                Ok(name) => {
                    let parts_maybe = name.split(".").collect::<Vec<&str>>();
                    ui.horizontal(|row| {
                        if parts_maybe.len() == 2 && parts_maybe[0].starts_with("A") {
                            // MB calibration
                            row.colored_label(Color32::GREEN,  format!("Mechanical calibration: '{}' from {}", parts_maybe[1], parts_maybe[0]));
                        } else {
                            row.colored_label(Color32::GREEN,  format!("Custom mechanical calibration in use: '{name}'"));
                        }
                        if row.button("Load/Save from file").clicked() {
                            self.editing_cal = Some(CalibrationSection::Mechanical)
                        }
                    });
                },
                Err(_) => {
                    error_counter += 1;
                    ui.colored_label(Color32::RED, "Mechanical calibration NOT FOUND");
                }
            };
            match String::from_utf8(interpreted.tcc_cal_name.to_vec()) {
                Ok(name) => {
                    let parts_maybe = name.split(".").collect::<Vec<&str>>();
                    ui.horizontal(|row| {
                        if parts_maybe.len() == 2 && parts_maybe[0].starts_with("A") {
                            // MB calibration
                            row.colored_label(Color32::GREEN,  format!("TCC properties calibration: '{}' from {}", parts_maybe[1], parts_maybe[0]));;
                        } else {
                            row.colored_label(Color32::GREEN,  format!("Custom TCC properties calibration in use: '{name}'"));
                        }
                        if row.button("Load/Save from file").clicked() {
                            self.editing_cal = Some(CalibrationSection::TorqueConverter)
                        }
                    });
                },
                Err(_) => {
                    error_counter += 1;
                    ui.colored_label(Color32::RED, "TCC properties calibration NOT FOUND");
                }
            };

            match String::from_utf8(interpreted.shift_algo_cal_name.to_vec()) {
                Ok(name) => {
                    let parts_maybe = name.split(".").collect::<Vec<&str>>();
                    ui.horizontal(|row| {
                        if parts_maybe.len() == 2 && parts_maybe[0].starts_with("A") {
                            // MB calibration
                            row.colored_label(Color32::GREEN,  format!("Shift algo pack calibration: '{}' from {}", parts_maybe[1], parts_maybe[0]));;
                        } else {
                            row.colored_label(Color32::GREEN,  format!("Custom shift algo pack calibration in use: '{name}'"));
                        }
                        if row.button("Load/Save from file").clicked() {
                            self.editing_cal = Some(CalibrationSection::ShiftAlgo)
                        }
                    });
                },
                Err(_) => {
                    error_counter += 1;
                    ui.colored_label(Color32::RED, "Shift algo pack calibration NOT FOUND");
                }
            };

            if error_counter == 0 {
                // We can save!
                if ui.button("Apply calibrations").clicked() {
                    sign_and_crc(&mut interpreted);
                    interpreted.pack_to_slice(flash).unwrap();
                    Self::write_calibration(self.nag.clone(), flash.clone());
                }
            } else {
                ui.label("There are errors in the calibration data. Please correct the errors above.");
            }
            ui.separator();
            let l = self.linked.as_ref().unwrap();
            // Allow the user to filter by chassis and gearbox code
            ui.horizontal(|row| {
                row.strong("Filter by chassis");
                row.text_edit_singleline(&mut self.chassis_input);
            });
            ui.horizontal(|row| {
                row.strong("Filter by gearbox");
                row.text_edit_singleline(&mut self.gb_input);
            });
            ui.horizontal(|row| {
                row.strong("Filter by EGS PN.");
                row.text_edit_singleline(&mut self.egs_pn);
            });

            let linked_data: Vec<&EgsLinkedData> = l.iter().filter(|ld| {
                ld.gb.contains(&self.gb_input) && ld.chassis.contains(&self.chassis_input) && ld.pn.contains(&self.egs_pn)
            }).collect();

            // SCN Columns - [EGS PN, Chassis, GB Code, TCC, MECH, HYDR]
            egui_extras::TableBuilder::new(ui)
                .columns(Column::auto().at_least(100.0), 7)
                .column(Column::exact(100.0))
                .striped(true)
                .header(30.0, |mut header| {
                    header.col(|c| {c.add(Label::new(RichText::new("EGS PN").strong()));});
                    header.col(|c| {c.add(Label::new(RichText::new("Chassis").strong()));});
                    header.col(|c| {c.add(Label::new(RichText::new("Gearbox").strong()));});
                    header.col(|c| {c.add(Label::new(RichText::new("TCC property CAL").strong()));});
                    header.col(|c| {c.add(Label::new(RichText::new("Mechanical CAL").strong()));});
                    header.col(|c| {c.add(Label::new(RichText::new("Hydraulic CAL").strong()));});
                    header.col(|c| {c.add(Label::new(RichText::new("Shift algo CAL").strong()));});
                    header.col(|c| {c.add(Label::new(RichText::new("").strong()));});
                }).body(|body| {
                    body.rows(20.0, linked_data.len(), |mut row| {
                        let idx = row.index();
                        let cal = linked_data[idx];
                        row.col(|c| {c.add(Label::new(&cal.pn));});
                        row.col(|c| {c.add(Label::new(&cal.chassis));});
                        row.col(|c| {c.add(Label::new(&cal.gb));});
                        row.col(|c| {c.add(Label::new(&cal.tcc));});
                        row.col(|c| {c.add(Label::new(&cal.mech));});
                        row.col(|c| {c.add(Label::new(&cal.hydr));});
                        row.col(|c| {c.add(Label::new(&cal.shift_algo));});
                        row.col(|c| {
                            if c.button("Apply").clicked() {
                                let c: EgsLinkedData = cal.clone();
                                self.viewing_cal = Some(c)
                            }
                        });
                    });
                });
            if let Some(linked_data) = &self.viewing_cal {
                let mut open = true;

                let mech = db.mechanical_calibrations.iter().find(|x| {
                    x.valid_egs_pns.contains(&linked_data.pn) && x.name == linked_data.mech
                }).unwrap();

                let hydr = db.hydralic_calibrations.iter().find(|x| {
                    x.valid_egs_pns.contains(&linked_data.pn) && x.name == linked_data.hydr
                }).unwrap();

                let tcc = db.torqueconverter_calibrations.iter().find(|x| {
                    x.valid_egs_pns.contains(&linked_data.pn) && x.name == linked_data.tcc
                }).unwrap();

                let shift_algo = db.shift_algo_map_calibration.iter().find(|x| {
                    x.valid_egs_pns.contains(&linked_data.pn) && x.name == linked_data.shift_algo
                }).unwrap();

                Window::new("Explore calibrations")
                    .open(&mut open)
                    .show(ui.ctx(), |ui| {
                        let mut modified = false;
                        if ui.button("Use hydraulic calibration").clicked() {
                            interpreted.hydr_cal = hydr.data;
                            let name = format!("{}.{}", linked_data.pn, linked_data.hydr);
                            assert!(name.len() <= 16);
                            interpreted.hydr_cal_name.fill(0);
                            interpreted.hydr_cal_name[0..name.len()].copy_from_slice(name.as_bytes());
                            modified = true;
                        }
                        if ui.button("Use mechanical calibration").clicked() {
                            interpreted.mech_cal = mech.data;
                            let name = format!("{}.{}", linked_data.pn, linked_data.mech);
                            assert!(name.len() <= 16);
                            interpreted.mech_cal_name.fill(0);
                            interpreted.mech_cal_name[0..name.len()].copy_from_slice(name.as_bytes());
                            modified = true;
                        }
                        if ui.button("Use torque converter calibration").clicked() {
                            interpreted.tcc_cal = tcc.data;
                            let name = if linked_data.tcc == "NO NAME" {
                                "NN"
                            } else {
                                &linked_data.tcc
                            };
                            let name = format!("{}.{}", linked_data.pn, name);
                            assert!(name.len() <= 16);
                            interpreted.tcc_cal_name.fill(0);
                            interpreted.tcc_cal_name[0..name.len()].copy_from_slice(name.as_bytes());
                            modified = true;
                        }
                        if ui.button("Use Shift algo pack calibration").clicked() {
                            interpreted.shift_algo_cal = shift_algo.data;
                            let name = format!("{}.{}", linked_data.pn, linked_data.shift_algo);
                            assert!(name.len() <= 16);
                            interpreted.shift_algo_cal_name.fill(0);
                            interpreted.shift_algo_cal_name[0..name.len()].copy_from_slice(name.as_bytes());
                            modified = true;
                        }
                        if modified {
                            // Sign and save
                            sign_and_crc(&mut interpreted);
                            interpreted.pack_to_slice(flash).unwrap();
                        }
                    });
                if !open {
                    self.viewing_cal = None;
                }
            }


            if let Some(editing) = &self.editing_cal {
                let mut open = true;
                Window::new("Explore calibrations")
                    .open(&mut open)
                    .show(ui.ctx(), |ui| {
                        ui.strong("Editing in the config app is unsupported at this time");
                        ui.label("Save to YML, edit, then load!");
                        ui.colored_label(Color32::RED, "EDITING CALIBRATIONS IS SUPER DANGEROUS. ENSURE YOU KNOW WHAT YOU ARE DOING");
                        if ui.button("Save to YML").clicked() {
                            if let Some(f) = rfd::FileDialog::new().add_filter("yml", &["yml"]).save_file() {
                                let dump = match editing {
                                    CalibrationSection::Hyraulic => serde_yaml::to_string(&interpreted.hydr_cal).unwrap(),
                                    CalibrationSection::Mechanical => serde_yaml::to_string(&interpreted.mech_cal).unwrap(),
                                    CalibrationSection::TorqueConverter => serde_yaml::to_string(&interpreted.tcc_cal).unwrap(),
                                    CalibrationSection::ShiftAlgo => serde_yaml::to_string(&interpreted.shift_algo_cal).unwrap(),
                                };
                                let mut r = File::create(f).unwrap();
                                r.write_all(dump.as_bytes()).unwrap();
                            }
                        }
                        if ui.button("Load from file").clicked() {
                            if let Some(f) = rfd::FileDialog::new().add_filter("yml", &["yml"]).pick_file() {
                                let mut r = File::open(f.clone()).unwrap();
                                let mut buf = Vec::new();
                                r.read_to_end(&mut buf).unwrap();
                                let contents = String::from_utf8(buf).unwrap();
                                let res = match editing {
                                    CalibrationSection::Hyraulic => serde_yaml::from_str::<EgsHydraulicConfiguration>(&contents).map(|x| interpreted.hydr_cal = x),
                                    CalibrationSection::Mechanical => serde_yaml::from_str::<EgsMechanicalConfiguration>(&contents).map(|x| interpreted.mech_cal = x),
                                    CalibrationSection::TorqueConverter => serde_yaml::from_str::<EgsTorqueConverterConfiguration>(&contents).map(|x| interpreted.tcc_cal = x),
                                    CalibrationSection::ShiftAlgo => serde_yaml::from_str::<EgsShiftMapConfiguration>(&contents).map(|x| interpreted.shift_algo_cal = x),
                                };
                                if let Err(e) = res {
                                    let msg = format!("Failed to load calibrations: {}", e.to_string());
                                    action = PageAction::SendNotification { text: msg, kind: ToastKind::Error };
                                } else {
                                    let n = f.file_name().unwrap();
                                    let sl= n.to_string_lossy();
                                    let cal_name = sl.split(".yml").next().unwrap();
                                    let mut buf = cal_name.as_bytes().to_vec();
                                    buf.resize(16, 0x00);
                                    match editing {
                                        CalibrationSection::Hyraulic => interpreted.hydr_cal_name = buf.try_into().unwrap(),
                                        CalibrationSection::Mechanical => interpreted.mech_cal_name = buf.try_into().unwrap(),
                                        CalibrationSection::TorqueConverter => interpreted.tcc_cal_name = buf.try_into().unwrap(),
                                        CalibrationSection::ShiftAlgo => interpreted.shift_algo_cal_name = buf.try_into().unwrap()
                                    }
                                    // Sign and save
                                    sign_and_crc(&mut interpreted);
                                    interpreted.pack_to_slice(flash).unwrap();
                                }
                            }
                        }
                    });
                if !open {
                    self.editing_cal = None;
                }
            }
        }
        action
    }

    fn get_title(&self) -> &'static str {
        "EGS Compatibility Config"
    }

    fn should_show_statusbar(&self) -> bool {
        true
    }
}