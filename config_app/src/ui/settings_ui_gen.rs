use std::{fs::File, io::{BufReader, Cursor, Read}, sync::{Arc, RwLock}};
use backend::{diag::{Nag52Diag, settings::{SettingsData, ModuleSettingsData, EnumMap, SettingsType, SettingsVariable, EnumDesc}}, ecu_diagnostics::{kwp2000::{KwpSessionType, KwpCommand, KwpSessionTypeByte}, DiagServerResult}, serde_yaml};
use eframe::{egui::{self, CollapsingHeader, DragValue, Label, MenuBar, ProgressBar, RichText, ScrollArea}, emath, epaint::{Color32, ahash::HashMap}};
use zip::ZipArchive;

use crate::window::{InterfacePage, PageAction};

#[derive(Debug, Clone)]
pub enum LoadState {
    Msg(String),
    Download {
        curr_addr: u32,
        total: u32,
        done: u32
    },
    Ready,
    Err(String)
}

/// Strips the 3-byte positive-response header from a coding string reply.
///
/// The TCU response is untrusted; a short frame must not panic a worker thread.
fn coding_string_payload(x: Vec<u8>) -> DiagServerResult<Vec<u8>> {
    if x.len() < 3 {
        Err(backend::ecu_diagnostics::DiagError::InvalidResponseLength)
    } else {
        Ok(x[3..].to_vec())
    }
}

pub struct TcuAdvSettingsUi {
    status: Arc<RwLock<LoadState>>,
    nag: Nag52Diag,
    yml: Arc<RwLock<Option<ModuleSettingsData>>>,
    current_settings: Arc<RwLock<HashMap<u8, DiagServerResult<Vec<u8>>>>>,
    default_settings: Arc<RwLock<HashMap<u8, DiagServerResult<Vec<u8>>>>>,
    current_setting: Option<u8>
}

impl TcuAdvSettingsUi {
    pub fn new(nag: Nag52Diag, ctx: egui::Context) -> Self {

        let status = Arc::new(RwLock::new(LoadState::Msg(format!("Init"))));
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

            fn load_file(status: Arc<RwLock<LoadState>>, nag: Nag52Diag, ctx: egui::Context) -> Result<ModuleSettingsData, String> {
                *status.write().unwrap() = LoadState::Msg(format!("Entering NAG52 diag mode"));
                ctx.request_repaint();
                nag.with_kwp(|x| x.kwp_set_session(KwpSessionTypeByte::Extended(0x93))).map_err(|e| e.to_string())?;
                *status.write().unwrap() = LoadState::Msg(format!("Locating embedded container"));
                ctx.request_repaint();
                let part_info = nag.get_embed_file_info().map_err(|e| e.to_string())?;
                let mut read_contents = Vec::new();
                while read_contents.len() < part_info.size as usize {
                    let to_read = std::cmp::min(250, part_info.size as usize - read_contents.len()) as u8;
                    let addr = part_info.address + read_contents.len() as u32;
                    *status.write().unwrap() = LoadState::Download {
                        curr_addr: part_info.address + read_contents.len() as u32,
                        total: part_info.size,
                        done: read_contents.len() as u32,
                    };
                    ctx.request_repaint();
                    let data = nag.read_mem_by_addr_ext(addr, to_read).map_err(|e| e.to_string())?;
                    read_contents.extend_from_slice(&data);
                }
                let reader = BufReader::new(Cursor::new( read_contents));
                let mut zip = ZipArchive::new(reader).map_err(|_| format!("Data on EGS is corrupt!"))?;
                let mut mod_settings = zip.by_name("MODULE_SETTINGS.yml").map_err(|_| format!("Data on EGS does not contain MODULE_SETTINGS"))?;
                let mut s = String::new();
                mod_settings.read_to_string(&mut s).map_err(|e| format!("MODULE_SETTINGS.yml could not be read: {e}"))?;
                serde_yaml::from_str::<ModuleSettingsData>(&s).map_err(|e| e.to_string())
            }

            match load_file(status_c.clone(), nag_c.clone(), ctx.clone()) {
                Ok(yml) => {
                    *yml_c.write().unwrap() = Some(yml.clone());
                    for setting in &yml.settings {
                        // A setting with no SCN_ID cannot be addressed on the TCU; skip it
                        // rather than taking the whole settings page down.
                        let Some(scn_id) = setting.scn_id else {
                            eprintln!("Setting '{}' has no SCN_ID, skipping", setting.name);
                            continue;
                        };
                        let _ = nag_c.with_kwp(|k| {
                            *status_c.write().unwrap() = LoadState::Msg(format!("Reading {} current configuration", setting.name));
                            let res = k.send_byte_array_with_response(&[0x21, 0xFC, scn_id], None)
                                .and_then(coding_string_payload);
                            ctx.request_repaint();
                            current_settings_c.write().unwrap().insert(scn_id, res);
                            *status_c.write().unwrap() = LoadState::Msg(format!("Reading {} default configuration", setting.name));
                            let res_defaut = k.send_byte_array_with_response(&[0x21, 0xFC, scn_id | 0b10000000], None)
                                .and_then(coding_string_payload);
                            default_settings_c.write().unwrap().insert(scn_id, res_defaut);
                            ctx.request_repaint();
                            Ok(())
                        });
                    }
                    *status_c.write().unwrap() = LoadState::Ready;
                    *status_c.write().unwrap() = LoadState::Ready
                },
                Err(e) => {
                    *status_c.write().unwrap() = LoadState::Err(e)
                }
            }
            ctx.request_repaint();
        });

        Self {
            status,
            nag,
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
            dv = dv.range(0..=100);
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

/// Renders one editable setting.
///
/// Returns `None` when the coding string and the YAML description disagree (bad offset,
/// length or type name); the row then shows the decode error instead of the app dying.
fn gen_row(ui: &mut egui::Ui, var: &SettingsVariable, coding: &mut [u8], enums: &[EnumMap], internal_structs: &[SettingsData]) -> Option<SettingsType> {
    ui.code(&var.name);
    let decoded = match var.to_settings_type(&coding, enums, internal_structs) {
        Ok(d) => d,
        Err(e) => {
            ui.colored_label(Color32::RED, "Cannot decode");
            ui.add(Label::new(e.to_string()).wrap());
            return None;
        }
    };
    let v = match decoded {
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
            egui::ComboBox::new(format!("Enum-{}-select", var.name), "")
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
                .id_salt(format!("It-var-editor-{}",var.name))
                .show(ui, |ui| {
                    egui::Grid::new(format!("setting-var-editor-{}",var.name)).num_columns(3).striped(true).show(ui, |ui| {
                        ui.strong("Setting");
                        ui.strong("Value");
                        ui.strong("Description");
                        ui.end_row();
                        for param in &s.params {
                            let edited = gen_row(ui, param, &mut raw, enums, internal_structs);
                            ui.end_row();
                            if let Some(edited) = edited {
                                if let Err(e) = param.insert_back_into_coding_string(edited, &mut raw) {
                                    ui.colored_label(Color32::RED, e.to_string());
                                    ui.end_row();
                                }
                            }
                        }
                    });
                });
            SettingsType::Struct { raw, s }
        },
    };
    ui.add(Label::new(var.description.clone().unwrap_or("-".into())).wrap());
    Some(v)
}

fn generate_editor_ui(nag: &Nag52Diag, coding: &mut Vec<u8>, default: &[u8], setting: &SettingsData, enums: &[EnumMap], internal_structs: &[SettingsData], ui: &mut egui::Ui) -> Option<PageAction> {
    let mut ret = None;
    let width = ui.available_width();
    ui.collapsing("Show coding bytes", |ui| {
        ScrollArea::new([true, false]).max_width(width).show(ui, |r| {
            egui::Grid::new("COD")
                .num_columns(coding.len()+1)
                .striped(true).show(r, |ui| {
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
    });
    ui.horizontal(|r| {
        // Current and default coding strings come from two separate ECU reads, so their
        // lengths are not guaranteed to match.
        let lengths_match = coding.len() == default.len();
        if r.add_enabled(lengths_match, egui::Button::new("Reset coding to default")).clicked() {
            coding.copy_from_slice(default);
        }
        if !lengths_match {
            r.colored_label(
                Color32::RED,
                format!(
                    "Cannot reset: TCU returned {} current bytes but {} default bytes",
                    coding.len(),
                    default.len()
                ),
            );
        }
        if r.button("Write to TCU").clicked() {
            let Some(scn_id) = setting.scn_id else {
                ret = Some(PageAction::SendNotification {
                    text: format!("Setting {} has no SCN_ID in MODULE_SETTINGS.yml", setting.name),
                    kind: egui_notify::ToastLevel::Error,
                });
                return;
            };
            ret = match nag.with_kwp(|kwp| {
                let mut tx = vec![KwpCommand::WriteDataByLocalIdentifier.into(), 0xFC, scn_id];
                tx.extend_from_slice(coding);
                kwp.send_byte_array_with_response(&tx, None)
            }) {
                Ok(_) => {
                    Some(
                        PageAction::SendNotification { 
                            text: format!("Writing of setting {} OK!", setting.name),
                            kind: egui_notify::ToastLevel::Success
                        }
                    )
                },
                Err(e) => {
                    Some(
                        PageAction::SendNotification { 
                            text: format!("Writing of setting {} failed: {e:?}", setting.name),
                            kind: egui_notify::ToastLevel::Error
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
                let edited = gen_row(ui, param, coding, enums, internal_structs);
                ui.end_row();
                if let Some(edited) = edited {
                    if let Err(e) = param.insert_back_into_coding_string(edited, coding) {
                        ui.colored_label(Color32::RED, e.to_string());
                        ui.end_row();
                    }
                }
            }            
        });
    });
    ret
}

impl InterfacePage for TcuAdvSettingsUi {
    fn make_ui(&mut self, ui: &mut eframe::egui::Ui) -> crate::window::PageAction {
        let state = self.status.read().unwrap().clone();
        let yml = self.yml.read().unwrap().clone();
        let def_settings = self.default_settings.read().unwrap().clone();
        let curr_settings = self.current_settings.read().unwrap().clone();
        let mut action = PageAction::None;
        match state {
            LoadState::Ready => {
                // `Ready` is only published after the YAML has loaded, but do not make the
                // whole page a panic if that invariant ever changes.
                let Some(yml) = yml.as_ref().cloned() else {
                    ui.colored_label(Color32::RED, "Settings description is not loaded");
                    return action;
                };
                MenuBar::new()
                    .ui(ui, |ui| {
                    ui.menu_button("Select coding string", |ui| {
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                        for (k, _) in &curr_settings {
                            let Some(setting_def) = yml.settings.iter().find(|x| x.scn_id == Some(*k)) else {
                                continue;
                            };
                            let text = setting_def.description.as_ref().unwrap_or(&setting_def.name);
                            ui.selectable_value(&mut self.current_setting, Some(*k), text);
                        }
                    });
                });
                ui.separator();
                if let Some(current_id) = self.current_setting {
                    let setting_def = yml.settings.iter().find(|x| x.scn_id == Some(current_id));
                    let default = def_settings.get(&current_id);
                    let modifying = curr_settings.get(&current_id);

                    match (setting_def, default, modifying) {
                        (Some(setting_def), Some(Ok(def)), Some(Ok(modify))) => {
                            let def = def.clone();
                            let mut modify = modify.clone();
                            if let Some(a) = generate_editor_ui(&self.nag, &mut modify, &def, setting_def, &yml.enums, &yml.internal_structures, ui) {
                                action = a;
                            }
                            self.current_settings.write().unwrap().insert(current_id, Ok(modify));
                        }
                        (None, _, _) => {
                            ui.label(format!("No description for coding string 0x{current_id:02X} in MODULE_SETTINGS.yml"));
                        }
                        _ => {
                            ui.label("Cannot load UI for this coding string due to TCU query error!");
                        }
                    }
                } else {
                    ui.label("No coding string selected");
                }
            },
            LoadState::Msg(txt) => {
                ui.label(txt);
            },
            LoadState::Download { curr_addr, total, done } => {
                let fraction = if total == 0 { 0.0 } else { done as f32 / total as f32 };
                let pb = ProgressBar::new(fraction)
                    .animate(true)
                    .show_percentage()
                    .text(format!("Downloading diagnostic info. Addr: {:08X}", curr_addr));
                ui.add(pb);
            },
            LoadState::Err(e) => {
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
                            let mut s = String::new();
                            match File::open(&f).and_then(|mut fh| fh.read_to_string(&mut s)) {
                                Ok(_) => {}
                                Err(e) => {
                                    *status_c.write().unwrap() =
                                        LoadState::Err(format!("Could not read {}: {e}", f.display()));
                                    ctx.request_repaint();
                                    return;
                                }
                            }
                            match serde_yaml::from_str::<ModuleSettingsData>(&s) {
                                Ok(s) => {
                                    *yml_c.write().unwrap() = Some(s.clone());
                                    *status_c.write().unwrap() = LoadState::Msg(format!("Entering NAG52 diag mode"));
                                    ctx.request_repaint();
                                    if nag_c.with_kwp(|x| x.kwp_set_session(KwpSessionTypeByte::Extended(0x93))).is_err() {
                                        *status_c.write().unwrap() = LoadState::Err("Cannot enter 0x93 diag mode".into())
                                    } else {
                                        for setting in &s.settings {
                                            let Some(scn_id) = setting.scn_id else {
                                                eprintln!("Setting '{}' has no SCN_ID, skipping", setting.name);
                                                continue;
                                            };
                                            let _ = nag_c.with_kwp(|k| {
                                                *status_c.write().unwrap() = LoadState::Msg(format!("Reading {} current configuration", setting.name));
                                                let res = k.send_byte_array_with_response(&[0x21, 0xFC, scn_id], None)
                                                    .and_then(coding_string_payload);
                                                ctx.request_repaint();
                                                current_settings_c.write().unwrap().insert(scn_id, res);
                                                *status_c.write().unwrap() = LoadState::Msg(format!("Reading {} default configuration", setting.name));
                                                let res = k.send_byte_array_with_response(&[0x21, 0xFC, scn_id | 0b10000000], None)
                                                    .and_then(coding_string_payload);
                                                default_settings_c.write().unwrap().insert(scn_id, res);
                                                ctx.request_repaint();
                                                Ok(())
                                            });
                                        }
                                        *status_c.write().unwrap() = LoadState::Ready;
                                    }
                                    ctx.request_repaint();
                                    // Now read all the states
                                },
                                Err(e) => {
                                    *status_c.write().unwrap() = LoadState::Err(format!("Failed to decode YML: {e:?}"));
                                }
                            }
                        }); 
                    }
                }
            },
        }

        action
    }

    fn should_show_statusbar(&self) -> bool {
        true
    }

    fn destroy_nag(&self) -> bool {
        false
    }

    fn on_load(&mut self, _nag: Option<Arc<Nag52Diag>>){}

    fn nag_destroy_before_load(&self) -> bool {
        false
    }
}

impl Drop for TcuAdvSettingsUi {
    fn drop(&mut self) {
        let _ = self.nag.with_kwp(|x| x.kwp_set_session(KwpSessionType::Normal.into()));
    }
}
