use std::{sync::{atomic::AtomicBool, Arc, RwLock}, borrow::Borrow, time::{Instant, Duration}, ops::RangeInclusive, fs::File, io::{Write, Read}, any::Any, fmt::format, num::Wrapping};
use backend::{diag::{Nag52Diag, settings::{SettingsData, ModuleSettingsData, EnumMap, SettingsType, SettingsVariable, EnumDesc}, module_settings_flash_store::{ModuleSettingsFlashHeader, MsFlashReadError}}, ecu_diagnostics::{kwp2000::{KwpSessionType, KwpCommand, KwpSessionTypeByte}, DiagServerResult}, serde_yaml};
use eframe::{egui::{ProgressBar, DragValue, self, CollapsingHeader, ScrollArea, Window, TextEdit, TextBuffer, Layout, Label, Button, RichText}, epaint::{Color32, ahash::HashMap, Vec2}, emath};
use egui_extras::{TableBuilder, Column};
use egui_toast::ToastKind;
use egui_plot::{PlotPoints, Line, Plot};
use serde::{Serialize, Deserialize, de::DeserializeOwned, __private::de};

use crate::window::{InterfacePage, PageLoadState, PageAction};

pub const PAGE_LOAD_TIMEOUT: f32 = 10000.0;



pub struct TcuAdvSettingsUi {
    status: Arc<RwLock<PageLoadState>>,
    error: Option<String>,
    nag: Nag52Diag,
    start_time: Instant,
    yml: Arc<RwLock<Option<ModuleSettingsData>>>,
    current_settings: Arc<RwLock<HashMap<u8, DiagServerResult<Vec<u8>>>>>,
    default_settings: Arc<RwLock<HashMap<u8, DiagServerResult<Vec<u8>>>>>,
    current_setting: Option<u8>
}

impl TcuAdvSettingsUi {
    pub fn new(nag: Nag52Diag, ctx: egui::Context) -> Self {

        let status = Arc::new(RwLock::new(PageLoadState::Waiting(format!("Init"))));
        let status_c = status.clone();

        let yml = Arc::new(RwLock::new(None));
        let yml_c = yml.clone();

        let default_settings = Arc::new(RwLock::new(HashMap::default()));
        let default_settings_c = default_settings.clone();

        let current_settings = Arc::new(RwLock::new(HashMap::default()));
        let current_settings_c = current_settings.clone();
        let nag_c = nag.clone();
        // Firstly, try to read from flash
        std::thread::spawn(move || {
            *status_c.write().unwrap() = PageLoadState::Waiting(format!("Entering NAG52 diag mode"));
            ctx.request_repaint();
            if nag_c.with_kwp(|x| x.kwp_set_session(KwpSessionTypeByte::Extended(0x93))).is_err() {
                *status_c.write().unwrap() = PageLoadState::Err("Cannot enter 0x93 diag mode".into())
            } else {
                // Read flash
                let part_info = nag_c.get_module_settings_desc_partition();
                *status_c.write().unwrap() = PageLoadState::Waiting(format!("Initializing download from flash"));
                ctx.request_repaint();
                match nag_c.begin_download(&part_info) {
                    Err(e) => {
                        *status_c.write().unwrap() = PageLoadState::Err(format!("Failed to start download {e}"));
                        ctx.request_repaint();
                        return;
                    },
                    Ok(_block_size) => {
                        let mut counter: u8 = 0;
                        let mut read_contents = Vec::new();
                        let mut header: Option<Result<ModuleSettingsFlashHeader, MsFlashReadError>> = None;


                        let mut read_len = part_info.size;
                        if let Some(Ok(r)) = header {
                            read_len = r.length_compressed + std::mem::size_of::<ModuleSettingsFlashHeader>() as u32;
                        }

                        while read_contents.len() < read_len as usize {
                            *status_c.write().unwrap() = PageLoadState::Waiting(format!("Reading flash address 0x{:06X}", part_info.address + read_contents.len() as u32));
                            ctx.request_repaint();
                            match nag_c.read_data(counter) {
                                Ok(data) => {
                                    read_contents.extend_from_slice(&data);
                                    counter = counter.wrapping_add(1);
                                    if read_contents.len() > std::mem::size_of::<ModuleSettingsFlashHeader>() && header.is_none() {
                                        header = Some(ModuleSettingsFlashHeader::read_header_from_buffer(&read_contents));
                                        println!("Header: {:?}", header);
                                    }
                                    ctx.request_repaint();
                                },
                                Err(e) => {
                                    *status_c.write().unwrap() = PageLoadState::Err(format!("Failed to read flash at address 0x{:06X}: {e:?}", part_info.address + read_contents.len() as u32));
                                    ctx.request_repaint();
                                    return;
                                }
                            }
                        }
                        match ModuleSettingsFlashHeader::from_flash_bytes_to_yml_bytes(&read_contents) {
                            Ok((_header, yml_bytes)) => {
                                let s = String::from_utf8(yml_bytes).unwrap();
                                match serde_yaml::from_str::<ModuleSettingsData>(&s) {
                                    Ok(s) => {
                                        *yml_c.write().unwrap() = Some(s.clone());
                                        for setting in &s.settings {
                                            let scn_id = setting.scn_id.unwrap();
                                            let _ = nag_c.with_kwp(|k| {
                                                *status_c.write().unwrap() = PageLoadState::Waiting(format!("Reading {} current configuration", setting.name));
                                                let res = k.send_byte_array_with_response(&[0x21, 0xFC, scn_id])
                                                    .map(|x| x[3..].to_vec());
                                                ctx.request_repaint();
                                                current_settings_c.write().unwrap().insert(scn_id, res);
                                                *status_c.write().unwrap() = PageLoadState::Waiting(format!("Reading {} default configuration", setting.name));
                                                let res = k.send_byte_array_with_response(&[0x21, 0xFC, scn_id | 0b10000000])
                                                    .map(|x| x[3..].to_vec());
                                                default_settings_c.write().unwrap().insert(scn_id, res);
                                                ctx.request_repaint();
                                                Ok(())
                                            });
                                        }
                                        *status_c.write().unwrap() = PageLoadState::Ok;
                                        ctx.request_repaint();
                                        // Now read all the states
                                    },
                                    Err(e) => {
                                        *status_c.write().unwrap() = PageLoadState::Err(format!("Failed to decode YML: {e:?}"));
                                        ctx.request_repaint();
                                    }
                                }
                            },
                            Err(e) => {
                                *status_c.write().unwrap() = PageLoadState::Err(format!("Failed to inflate YML from flash: {e:?}"));
                                ctx.request_repaint();
                            }
                        }
                    }
                }
            }
        });

        Self {
            status,
            error: None,
            nag,
            start_time: Instant::now(),
            yml,
            current_settings,
            default_settings,
            current_setting: None
        }
    } 
}

fn gen_drag_value<'a, Num: emath::Numeric>(value: &'a mut Num, var: &'a SettingsVariable, decimals: bool) -> DragValue<'a> {
    let mut dv = DragValue::new(value).speed(0.0);

    if decimals {
        dv = dv.max_decimals(3).fixed_decimals(3);
    } else {
        dv = dv.max_decimals(0).fixed_decimals(0);
    }
    

    if let Some(mut unit) = var.unit.clone() {
        if unit == "%" {
            // Obvious
            dv = dv.clamp_range(0..=100);
        }
        if unit == "milliseconds" {
            unit = "ms".into();
        }

        dv = dv.custom_formatter(move |n, _| {
            if decimals {
                format!("{n:.3} {unit}")
            } else {
                format!("{n:.0} {unit}")
            }
        });
    }
    dv
}

fn gen_row(ui: &mut egui::Ui, var: &SettingsVariable, coding: &mut [u8], enums: &[EnumMap], internal_structs: &[SettingsData]) -> SettingsType {
    ui.code(&var.name);
    let v = match var.to_settings_type(&coding, enums, internal_structs) {
        SettingsType::Bool(mut b) => {
            ui.checkbox(&mut b, "");
            SettingsType::Bool(b)
        },
        SettingsType::F32(mut f) => {
            ui.add(gen_drag_value(&mut f, &var, true));
            SettingsType::F32(f)
        },
        SettingsType::I16(mut i) => {
            ui.add(gen_drag_value(&mut i, &var, false));
            SettingsType::I16(i)
        }
        SettingsType::U16(mut u) => {
            ui.add(gen_drag_value(&mut u, &var, false));
            SettingsType::U16(u)
        },
        SettingsType::U8(mut u) => {
            ui.add(gen_drag_value(&mut u, &var, false));
            SettingsType::U8(u)
        },
        SettingsType::Enum { mut value, mapping } => { 
            let s = mapping.mappings.get(&value).cloned().unwrap_or(EnumDesc {
                name: "INVALID CODING".to_string(),
                desc: format!("Value of 0x{:02X?} not known", value),
            });
            egui::ComboBox::from_id_source(format!("Enum-{}-select", var.name))
                .width(100.0)
                .selected_text(&s.name)
                .show_ui(ui, |x| {
                    for (k, e) in mapping.mappings.clone() {
                        x.push_id(format!("{}-{}", var.name, e.name), |x| {
                            x.selectable_value(
                                &mut value, 
                                k, 
                                e.name
                            ).on_hover_text(e.desc)
                        });
                    }
                });
            SettingsType::Enum { value, mapping } 
        },
        SettingsType::Struct { mut raw, s } => {
            
            CollapsingHeader::new("Show internal")
                .id_source(format!("It-var-editor-{}",var.name))
                .show(ui, |ui| {
                    egui::Grid::new(format!("setting-var-editor-{}",var.name)).num_columns(3).striped(true).show(ui, |ui| {
                        ui.strong("Setting");
                        ui.strong("Value");
                        ui.strong("Description");
                        ui.end_row();
                        for param in &s.params {
                            let s = gen_row(ui, param, &mut raw, enums, internal_structs);
                            ui.end_row();
                            param.insert_back_into_coding_string(s, &mut raw);
                        }            
                    });
                });
            SettingsType::Struct { raw, s }
        },
    };
    ui.add(Label::new(var.description.clone().unwrap_or("-".into())).wrap(true));
    v
}

fn generate_editor_ui(nag: &Nag52Diag, coding: &mut Vec<u8>, default: &[u8], setting: &SettingsData, enums: &[EnumMap], internal_structs: &[SettingsData], ui: &mut egui::Ui) -> Option<PageAction> {
    let mut ret = None;
    let width = ui.available_width();
    ScrollArea::new([true, false]).max_width(width).id_source("CODING_VIEW").show(ui, |r| {
        egui::Grid::new("COD").num_columns(coding.len()+1).striped(true).show(r, |ui| {
            ui.strong("Byte");
            for (idx, _) in coding.iter().enumerate() {
                ui.strong(format!("{}", idx));
            }
            ui.end_row();
            ui.strong("Current coding");
            for (idx, b) in coding.iter().enumerate() {
                if *b != default[idx] {
                    ui.label(RichText::new(format!("{:02X?}", b)).color(Color32::RED));
                } else {
                    ui.label(format!("{:02X?}", b));
                }
            }
            ui.end_row();
            ui.strong("Default coding");
            for b in default {
                ui.label(format!("{:02X?}", b));
            }
            ui.end_row();
        });
    });
    ui.add_space(10.0);
    ui.horizontal(|r| {
        if r.button("Reset coding to default").clicked() {
            coding.copy_from_slice(default);
        }
        if r.button("Write to TCU").clicked() {
            ret = match nag.with_kwp(|kwp| {
                let mut tx = vec![KwpCommand::WriteDataByLocalIdentifier.into(), 0xFC, setting.scn_id.unwrap()];
                tx.extend_from_slice(coding);
                kwp.send_byte_array_with_response(&tx)
            }) {
                Ok(_) => {
                    Some(
                        PageAction::SendNotification { 
                            text: format!("Writing of setting {} OK!", setting.name),
                            kind: ToastKind::Success
                        }
                    )
                },
                Err(e) => {
                    Some(
                        PageAction::SendNotification { 
                            text: format!("Writing of setting {} failed: {e:?}", setting.name),
                            kind: ToastKind::Error
                        }
                    )
                }
            }
        }
    });
    ui.add_space(10.0);
    ScrollArea::new([false, true]).max_height(ui.available_height()).show(ui, |ui| {
        egui::Grid::new("setting-var-editor").num_columns(3).striped(true).show(ui, |ui| {
            ui.strong("Setting");
            ui.strong("Value");
            ui.strong("Description");
            ui.end_row();
            for param in &setting.params {
                let s = gen_row(ui, param, coding, enums, internal_structs);
                ui.end_row();
                param.insert_back_into_coding_string(s, coding);
            }            
        });
    });
    ret
}

impl InterfacePage for TcuAdvSettingsUi {
    fn make_ui(&mut self, ui: &mut eframe::egui::Ui, frame: &eframe::Frame) -> crate::window::PageAction {
        let state = self.status.read().unwrap().clone();
        let yml = self.yml.read().unwrap().clone();
        let def_settings = self.default_settings.read().unwrap().clone();
        let curr_settings = self.current_settings.read().unwrap().clone();
        let mut action = PageAction::None;
        match state {
            PageLoadState::Ok => {
                let yml = yml.as_ref().unwrap().clone();
                ui.heading("Select coding string");
                ui.horizontal(|row| {
                    for (k, v) in &curr_settings {
                        let setting_def = yml.settings.iter().find(|x| x.scn_id.unwrap() == *k).unwrap();
                        row.selectable_value(&mut self.current_setting, Some(*k), setting_def.name.clone());
                    }
                });
                if let Some(current_id) = self.current_setting {
                    let setting_def = yml.settings.iter().find(|x| x.scn_id.unwrap() == current_id).unwrap();
                    let default = def_settings.get(&current_id).unwrap().clone();
                    let modifying = curr_settings.get(&current_id).unwrap().clone();

                    if modifying.is_ok() && default.is_ok() {
                        let def = default.unwrap().clone();
                        let mut modify = modifying.unwrap().clone();
                        ui.separator();
                        if let Some(a) = generate_editor_ui(&self.nag, &mut modify, &def, setting_def, &yml.enums, &yml.internal_structures, ui) {
                            action = a;
                        }
                        self.current_settings.write().unwrap().insert(current_id, Ok(modify));
                    } else {
                        ui.label("Cannot load UI for this coding string due to TCU query error!");
                    }
                } else {
                    ui.label("No coding string selected");
                }
            },
            PageLoadState::Waiting(txt) => {
                ui.label(txt);
            },
            PageLoadState::Err(e) => {
                ui.strong("Page load failed:");
                ui.label(e);

                ui.label("Try manually selecting MODULE_SETTINGS.yml");
                if ui.button("Select YML").clicked() {
                    if let Some(f) =  rfd::FileDialog::new().set_title("Choose MODULE_SETTINGS.yml").add_filter("YML", &["yml"]).pick_file() {
                        let status_c = self.status.clone();
                        let yml_c = self.yml.clone();
                        let ctx = ui.ctx().clone();
                        let nag_c = self.nag.clone();

                        let default_settings_c = self.default_settings.clone();
                        let current_settings_c = self.current_settings.clone();

                        std::thread::spawn(move || {
                            let mut f = File::open(f).unwrap();
                            let mut s = String::new();
                            f.read_to_string(&mut s).unwrap();
                            match serde_yaml::from_str::<ModuleSettingsData>(&s) {
                                Ok(s) => {
                                    *yml_c.write().unwrap() = Some(s.clone());
                                    *status_c.write().unwrap() = PageLoadState::Waiting(format!("Entering NAG52 diag mode"));
                                    ctx.request_repaint();
                                    if nag_c.with_kwp(|x| x.kwp_set_session(KwpSessionTypeByte::Extended(0x93))).is_err() {
                                        *status_c.write().unwrap() = PageLoadState::Err("Cannot enter 0x93 diag mode".into())
                                    } else {
                                        for setting in &s.settings {
                                            let scn_id = setting.scn_id.unwrap();
                                            let _ = nag_c.with_kwp(|k| {
                                                *status_c.write().unwrap() = PageLoadState::Waiting(format!("Reading {} current configuration", setting.name));
                                                let res = k.send_byte_array_with_response(&[0x21, 0xFC, scn_id])
                                                    .map(|x| x[3..].to_vec());
                                                ctx.request_repaint();
                                                current_settings_c.write().unwrap().insert(scn_id, res);
                                                *status_c.write().unwrap() = PageLoadState::Waiting(format!("Reading {} default configuration", setting.name));
                                                let res = k.send_byte_array_with_response(&[0x21, 0xFC, scn_id | 0b10000000])
                                                    .map(|x| x[3..].to_vec());
                                                default_settings_c.write().unwrap().insert(scn_id, res);
                                                ctx.request_repaint();
                                                Ok(())
                                            });
                                        }
                                        *status_c.write().unwrap() = PageLoadState::Ok;
                                    }
                                    ctx.request_repaint();
                                    // Now read all the states
                                },
                                Err(e) => {
                                    *status_c.write().unwrap() = PageLoadState::Err(format!("Failed to decode YML: {e:?}"));
                                }
                            }
                        }); 
                    }
                }
            },
        }

        action
    }

    fn get_title(&self) -> &'static str {
        "Advanced settings"
    }

    fn should_show_statusbar(&self) -> bool {
        true
    }

    fn destroy_nag(&self) -> bool {
        false
    }

    fn on_load(&mut self, nag: Option<Arc<Nag52Diag>>){}

    fn nag_destroy_before_load(&self) -> bool {
        false
    }
}

impl Drop for TcuAdvSettingsUi {
    fn drop(&mut self) {
        self.nag.with_kwp(|x| x.kwp_set_session(KwpSessionType::Normal.into()));
    }
}
