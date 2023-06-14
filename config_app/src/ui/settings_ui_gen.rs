use std::{sync::{atomic::AtomicBool, Arc, RwLock}, borrow::Borrow, time::{Instant, Duration}, ops::RangeInclusive, fs::File, io::{Write, Read}};

use backend::{diag::{settings::{TcuSettings, TccSettings, unpack_settings, LinearInterpSettings, pack_settings, SolSettings, SbsSettings, NagSettings, PrmSettings, AdpSettings}, Nag52Diag, DataState}, ecu_diagnostics::{kwp2000::{KwpSessionType, KwpCommand}, DiagServerResult}, serde_yaml::{Value, Mapping, self}};
use eframe::{egui::{ProgressBar, DragValue, self, CollapsingHeader, plot::{PlotPoints, Line, Plot}, ScrollArea, Window, TextEdit, TextBuffer, Layout, Label, Button, RichText}, epaint::Color32};
use egui_extras::{TableBuilder, Column};
use nfd::Response;
use serde::{Serialize, Deserialize, de::DeserializeOwned};

use crate::window::{InterfacePage, PageLoadState, PageAction};

pub const PAGE_LOAD_TIMEOUT: f32 = 10000.0;

#[derive(Debug, Clone)]
pub struct TcuSettingsWrapper<T>(Arc<RwLock<DataState<T>>>)
where T: TcuSettings;

impl<T> TcuSettingsWrapper<T>
where T: TcuSettings {
    pub fn new_pair() -> (Self, Self) {
        let s = Self(Arc::new(RwLock::new(DataState::Unint)));
        (s.clone(), s)
    }

    pub fn loaded_ok(&self) -> bool {
        self.0.read().unwrap().is_ok()
    }

    pub fn get_err_msg(&self) -> String {
        self.0.read().unwrap().get_err()
    }

    pub fn get_name(&self) -> &'static str {
        T::setting_name()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OpenSetting {
    None,
    Tcc,
    Sol,
    Sbs,
    Nag,
    Prm,
    Adp
}

pub struct TcuAdvSettingsUi {
    ready: Arc<RwLock<PageLoadState>>,
    nag: Nag52Diag,
    start_time: Instant,
    tcc_settings: TcuSettingsWrapper<TccSettings>,
    sol_settings: TcuSettingsWrapper<SolSettings>,
    sbs_settings: TcuSettingsWrapper<SbsSettings>,
    nag_settings: TcuSettingsWrapper<NagSettings>,
    prm_settings: TcuSettingsWrapper<PrmSettings>,
    adp_settings: TcuSettingsWrapper<AdpSettings>,
    open_settings: OpenSetting
}

pub fn read_scn_settings<T>(nag: &Nag52Diag, dest: &TcuSettingsWrapper<T>)
where T: TcuSettings {
    match nag.with_kwp(|kwp| {
        kwp.send_byte_array_with_response(&[0x21, 0xFC, T::get_scn_id()])
    }) {
        Ok(res) => {
            match unpack_settings::<T>(T::get_scn_id(), &res[2..]) {
                Ok(r) => *dest.0.write().unwrap() = DataState::LoadOk(r),
                Err(e) => *dest.0.write().unwrap() = DataState::LoadErr(e.to_string()),
            }
        },
        Err(e) => {
            *dest.0.write().unwrap() = DataState::LoadErr(e.to_string());
        },
    }
}

impl TcuAdvSettingsUi {
    pub fn new(nag: Nag52Diag) -> Self {
        let is_ready = Arc::new(RwLock::new(PageLoadState::waiting("Initializing")));
        let is_ready_t = is_ready.clone();

        let (tcc, tcc_t) = TcuSettingsWrapper::new_pair();
        let (sol, sol_t) = TcuSettingsWrapper::new_pair();
        let (sbs, sbs_t) = TcuSettingsWrapper::new_pair();
        let (gbs, gbs_t) = TcuSettingsWrapper::new_pair();
        let (prm, prm_t) = TcuSettingsWrapper::new_pair();
        let (adp, adp_t) = TcuSettingsWrapper::new_pair();
        let nag_c = nag.clone();
        std::thread::spawn(move|| {
            let res = nag_c.with_kwp(|x| {
                *is_ready_t.write().unwrap() = PageLoadState::waiting("Setting TCU diag mode");
                x.kwp_set_session(0x93.into())
            });

            match res {
                Ok(_) => {
                    *is_ready_t.write().unwrap() = PageLoadState::waiting("Reading TCC Settings")
                },
                Err(e) => {
                    *is_ready_t.write().unwrap() = PageLoadState::Err(e.to_string());
                    return;
                },
            };
            read_scn_settings(&nag_c, &tcc_t);
            read_scn_settings(&nag_c, &sol_t);
            read_scn_settings(&nag_c, &sbs_t);
            read_scn_settings(&nag_c, &gbs_t);
            read_scn_settings(&nag_c, &prm_t);
            read_scn_settings(&nag_c, &adp_t);
            *is_ready_t.write().unwrap() = PageLoadState::Ok;
        });
        Self {
            ready: is_ready,
            nag,
            start_time: Instant::now(),
            tcc_settings: tcc,
            sol_settings: sol,
            sbs_settings: sbs,
            nag_settings: gbs,
            prm_settings: prm,
            adp_settings: adp,
            open_settings: OpenSetting::None
        }
    } 
}

pub fn make_settings_ui<'de, T: TcuSettings>(nag: &Nag52Diag, settings_ref: &TcuSettingsWrapper<T>, ui: &mut eframe::egui::Ui) -> Option<PageAction>
where T: Clone + Copy + Serialize + DeserializeOwned {
    let mut action = None;
    let setting_state = settings_ref.0.read().unwrap().clone();
    if let DataState::LoadOk(mut settings) = setting_state {
        ui.with_layout(Layout::top_down(eframe::emath::Align::Min), |ui| {
            ui.label(format!("Setting revision name: {}", T::get_revision_name()));
            if let Some(url) = T::wiki_url() {
                ui.hyperlink_to(format!("Help on {}", T::setting_name()), url);
            }
            let ba = pack_settings(T::get_scn_id(), settings);
            ui.add_space(10.0);
            ui.label("Hex SCN coding (Display only)");
            let w = ui.available_width();
            ScrollArea::new([true, false]).id_source(ba.clone()).show(ui, |ui| {
                ui.add(Label::new(format!("{:02X?}", ba)).wrap(false));
                //let mut s = format!("{:02X?}", ba);
                //ui.add_enabled(true, TextEdit::singleline(&mut s).desired_width(100000.0));
            });
            ui.add_space(10.0);
            ui.horizontal(|x| {
                if x.button("Write settings").clicked() {
                    let res = nag.with_kwp(|x| {
                        let mut req = vec![KwpCommand::WriteDataByLocalIdentifier.into(), 0xFC];
                        req.extend_from_slice(&ba);
                        x.send_byte_array_with_response(&req)
                    });
                    match res {
                        Ok(_) => {
                            if T::effect_immediate() {
                                action = Some(PageAction::SendNotification { 
                                    text: format!("{} write OK!", T::setting_name()), 
                                    kind: egui_toast::ToastKind::Success 
                                });
                            } else {
                                action = Some(PageAction::SendNotification { 
                                    text: format!("{} write OK, but changes are only applied after a restart!", T::setting_name()), 
                                    kind: egui_toast::ToastKind::Warning 
                                });
                            }
                        },
                        Err(e) => {
                            action = Some(PageAction::SendNotification { 
                                text: format!("Error writing {}: {}", T::setting_name(), e.to_string()), 
                                kind: egui_toast::ToastKind::Error 
                            })
                        }
                    }
                }
                if x.button("Reset to TCU Default").clicked() {
                    let res = nag.with_kwp(|x| {
                        x.send_byte_array_with_response(&[KwpCommand::WriteDataByLocalIdentifier.into(), 0xFC, T::get_scn_id(), 0x00])
                    });
                    match res {
                        Ok(_) => {
                            if T::effect_immediate() {
                                action = Some(PageAction::SendNotification { 
                                    text: format!("{} reset OK!", T::setting_name()), 
                                    kind: egui_toast::ToastKind::Success 
                                });
                            } else {
                                action = Some(PageAction::SendNotification { 
                                    text: format!("{} reset OK, but changes are only applied after a restart!", T::setting_name()), 
                                    kind: egui_toast::ToastKind::Warning 
                                });
                            }
                            if let Ok(x) = nag.with_kwp(|kwp| kwp.send_byte_array_with_response(&[0x21, 0xFC, T::get_scn_id()])) {
                                if let Ok(res) = unpack_settings(T::get_scn_id(), &x[2..]) {
                                    settings = res;
                                }
                            }
                        },
                        Err(e) => {
                            action = Some(PageAction::SendNotification { 
                                text: format!("Error resetting {}: {}", T::setting_name(), e.to_string()), 
                                kind: egui_toast::ToastKind::Error 
                            })
                        }
                    }
                }
                if x.button("Save to YML").clicked() {
                    // Backup the settings to file
                    if let Ok(dialog) = nfd::dialog_save().filter("yml").open() {
                        if let Response::Okay(mut file) = dialog {
                            if !file.ends_with(".yml") {
                                file.push_str(".yml");
                            }
                            File::create(file.clone()).unwrap().write_all(serde_yaml::to_string(&settings).unwrap().as_bytes()).unwrap();
                            action = Some(PageAction::SendNotification { 
                                text: format!("{} backup created at {}!", T::setting_name(), file), 
                                kind: egui_toast::ToastKind::Success 
                            });
                        }
                    }

                }
                if x.button("Load from YML").clicked() {
                    // Backup the settings to file
                    if let Ok(dialog) = nfd::open_dialog(Some("yml"), None, nfd::DialogType::SingleFile) {
                        if let Response::Okay(file) = dialog {
                            let mut s = String::new();
                            let mut f = File::open(&file).unwrap();
                            f.read_to_string(&mut s).unwrap();
                            if let Ok(s) = serde_yaml::from_str(&s) {
                                settings = s;
                                action = Some(PageAction::SendNotification { 
                                    text: format!("{} loaded OK from {}!", T::setting_name(), file), 
                                    kind: egui_toast::ToastKind::Success 
                                });
                            } else {
                                action = Some(PageAction::SendNotification { 
                                    text: format!("Cannot load {}. Invalid settings YML!", file), 
                                    kind: egui_toast::ToastKind::Error 
                                });
                            }
                        }
                    }
                }
            });
            ui.add_space(10.0);
            ScrollArea::new([false, true]).show(ui, |ui| {
                let mut v = serde_yaml::to_value(&settings).unwrap();
                make_ui_for_value(T::setting_name(), &mut v, ui);
                if let Ok(s) = serde_yaml::from_value::<T>(v) {
                    settings = s;
                }
            });
        });
        *settings_ref.0.write().unwrap() = DataState::LoadOk(settings);
    }
    return action;
}

impl InterfacePage for TcuAdvSettingsUi {
    fn make_ui(&mut self, ui: &mut eframe::egui::Ui, frame: &eframe::Frame) -> crate::window::PageAction {
        match self.ready.read().unwrap().clone() {
            PageLoadState::Ok => {
                ui.heading("Advanced TCU settings");
            },
            PageLoadState::Waiting(reason) => {
                ui.heading("Please wait...");
                let prog = 
                ProgressBar::new(self.start_time.elapsed().as_millis() as f32 / PAGE_LOAD_TIMEOUT).animate(true);
                ui.add(prog);
                ui.label(format!("Current action: {}", reason));
                return PageAction::DisableBackBtn;
                
            },
            PageLoadState::Err(e) => {
                ui.heading("Page loading failed!");
                ui.label(format!("Error: {:?}", e));
                return PageAction::None;
            },
        }
        // Continues if OK
        ui.separator();
        let mut load_errors: Vec<(&'static str, String)> = Vec::new();
        ui.horizontal(|ui| {
            ui.strong("Choose program:");
            if self.tcc_settings.loaded_ok() {
                ui.selectable_value(&mut self.open_settings, OpenSetting::Tcc, self.tcc_settings.get_name());
            } else {
                load_errors.push((self.tcc_settings.get_name(), self.tcc_settings.get_err_msg()))
            }
            if self.sol_settings.loaded_ok() {
                ui.selectable_value(&mut self.open_settings, OpenSetting::Sol, self.sol_settings.get_name());
            } else {
                load_errors.push((self.sol_settings.get_name(), self.sol_settings.get_err_msg()))
            }
            if self.sbs_settings.loaded_ok() {
                ui.selectable_value(&mut self.open_settings, OpenSetting::Sbs, self.sbs_settings.get_name());
            } else {
                load_errors.push((self.sbs_settings.get_name(), self.sbs_settings.get_err_msg()))
            }
            if self.nag_settings.loaded_ok() {
                ui.selectable_value(&mut self.open_settings, OpenSetting::Nag, self.nag_settings.get_name());
            } else {
                load_errors.push((self.nag_settings.get_name(), self.nag_settings.get_err_msg()))
            }
            if self.prm_settings.loaded_ok() {
                ui.selectable_value(&mut self.open_settings, OpenSetting::Prm, self.prm_settings.get_name());
            } else {
                load_errors.push((self.nag_settings.get_name(), self.nag_settings.get_err_msg()))
            }
            if self.adp_settings.loaded_ok() {
                ui.selectable_value(&mut self.open_settings, OpenSetting::Adp, self.adp_settings.get_name());
            } else {
                load_errors.push((self.nag_settings.get_name(), self.nag_settings.get_err_msg()))
            }
        });
        ui.separator();
        ui.strong("Load status");
        if load_errors.is_empty() {
            ui.label("No load errors! All program settings loaded OK");
        } else {
            for err in load_errors {
                ui.label(RichText::new(format!("{} - {}", err.0, err.1)).color(Color32::RED));
            }
        }
        ui.separator();
        let action = match self.open_settings {
            OpenSetting::None => None,
            OpenSetting::Tcc => make_settings_ui(&self.nag, &self.tcc_settings, ui),
            OpenSetting::Sol => make_settings_ui(&self.nag, &self.sol_settings, ui),
            OpenSetting::Sbs => make_settings_ui(&self.nag, &self.sbs_settings, ui),
            OpenSetting::Nag => make_settings_ui(&self.nag, &self.nag_settings, ui),
            OpenSetting::Prm => make_settings_ui(&self.nag, &self.prm_settings, ui),
            OpenSetting::Adp => make_settings_ui(&self.nag, &self.adp_settings, ui),
        };
        if let Some(act) = action {
            act
        } else {
            crate::window::PageAction::None
        }
    }

    fn get_title(&self) -> &'static str {
        "Advanced settings"
    }

    fn should_show_statusbar(&self) -> bool {
        true
    }
}

impl Drop for TcuAdvSettingsUi {
    fn drop(&mut self) {
        self.nag.with_kwp(|x| x.kwp_set_session(KwpSessionType::Normal.into()));
    }
}

fn make_ui_for_value(setting_name: &'static str, v: &mut Value, ui: &mut egui::Ui) {
    if v.is_mapping() {
        make_ui_for_mapping(setting_name, &mut v.as_mapping_mut().unwrap(), ui)
    }
}

fn make_ui_for_mapping(setting_name: &'static str, v: &mut Mapping, ui: &mut egui::Ui) {
    egui::Grid::new(format!("Grid-{}", setting_name))
    .striped(true)
    .min_col_width(100.0)
    .show(ui, |ui| {
        ui.strong("Variable");
        ui.strong("Value");
        ui.end_row();
        for (i, v) in v.iter_mut() {
            let key = i.as_str().unwrap();
            if v.is_mapping() {
                CollapsingHeader::new(key).default_open(false).show(ui,|sub| {
                    if let Ok(lerp) = serde_yaml::from_value::<LinearInterpSettings>(v.clone()) {
                        // Linear interp extra display
                        sub.label("Linear interpolation settings");
                        sub.hyperlink_to("Help on Linear interpolation", "https://docs.ultimate-nag52.net/en/gettingstarted/configuration/settings/linearinterpolation");
                        sub.label("Representation:");
                        let mut points = Vec::new();
                        let mut x = 0.0_f32.min(lerp.raw_min - (lerp.raw_min/10.0));
                        while x < lerp.raw_max + (lerp.raw_max/10.0) {
                            points.push([x as f64, lerp.calc_with_value(x) as f64]);
                            x += 1.0;
                        }
                        let line =  Line::new(PlotPoints::new(points));

                        Plot::new(format!("lerp-{}", key))
                            .include_x(lerp.raw_min - (lerp.raw_min/10.0)) // Min X
                            .include_x(lerp.raw_max + (lerp.raw_max/10.0)) // Max X
                            .include_y(lerp.new_min - (lerp.new_min/10.0)) // Min Y
                            .include_y(lerp.new_max + (lerp.new_max/10.0)) // Max Y
                            .include_x(0)
                            .include_y(0)
                            .allow_drag(false)
                            .allow_scroll(false)
                            .allow_zoom(false)
                            .show(sub, |p| {
                                p.line(line)
                            });
                    }
                    make_ui_for_mapping(setting_name,&mut v.as_mapping_mut().unwrap(), sub);
                });
                ui.end_row();
            } else if v.is_bool() {
                ui.code(format!("{key}"));
                let mut o = v.as_bool().unwrap();
                ui.checkbox(&mut o, "");
                *v = Value::from(o);
                ui.end_row();
            } else if v.is_f64() {
                ui.code(format!("{key}: "));
                let mut o = v.as_f64().unwrap();
                let d = DragValue::new(&mut o).max_decimals(3).speed(0);
                ui.add(d);
                *v = Value::from(o);
                ui.end_row();
            } else if v.is_u64(){
                ui.code(format!("{key}: "));
                let mut o = v.as_u64().unwrap();
                let d = DragValue::new(&mut o).max_decimals(0).speed(0).clamp_range(RangeInclusive::new(0, i32::MAX));
                ui.add(d);
                *v = Value::from(o);
                ui.end_row();
            } else {
                ui.label(format!("FIXME: {:?} - {:?}", i, v));
                ui.end_row();
            }
        }
    });
}
