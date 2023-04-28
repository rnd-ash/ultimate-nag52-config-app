use std::{sync::{atomic::AtomicBool, Arc, RwLock}, borrow::Borrow, time::{Instant, Duration}, ops::RangeInclusive, fs::File, io::{Write, Read}};

use backend::{diag::{settings::{TcuSettings, TccSettings, unpack_settings, LinearInterpSettings, pack_settings, SolSettings, SbsSettings}, Nag52Diag}, ecu_diagnostics::{kwp2000::{KwpSessionType, KwpCommand}, DiagServerResult}, serde_yaml::{Value, Mapping, self}};
use eframe::egui::{ProgressBar, DragValue, self, CollapsingHeader, plot::{PlotPoints, Line, Plot}, ScrollArea, Window, TextEdit, TextBuffer, Layout, Label};
use nfd::Response;
use serde::{Serialize, Deserialize, de::DeserializeOwned};

use crate::window::{InterfacePage, PageLoadState, PageAction};

pub const PAGE_LOAD_TIMEOUT: f32 = 10000.0;

type TcuSettingsWrapper<T> = Arc<RwLock<SettingState<T>>>;

#[derive(Debug, Clone)]
pub enum SettingState<T> {
    LoadOk(T),
    Unint,
    LoadErr(String)
}

pub struct TcuAdvSettingsUi {
    ready: Arc<RwLock<PageLoadState>>,
    nag: Nag52Diag,
    start_time: Instant,
    tcc_settings: TcuSettingsWrapper<TccSettings>,
    sol_settings: TcuSettingsWrapper<SolSettings>,
    sbs_settings: TcuSettingsWrapper<SbsSettings>,
}

pub fn make_tcu_settings_wrapper<T>() -> (TcuSettingsWrapper<T>, TcuSettingsWrapper<T>) where T: Default {
    let x = Arc::new(RwLock::new(SettingState::Unint));
    let y = x.clone();
    (x, y)
}

pub fn read_scn_settings<T>(nag: &Nag52Diag, dest: &TcuSettingsWrapper<T>)
where T: TcuSettings {
    match nag.with_kwp(|kwp| {
        kwp.send_byte_array_with_response(&[0x21, 0xFC, T::get_scn_id()])
    }) {
        Ok(res) => {
            match unpack_settings::<T>(T::get_scn_id(), &res[2..]) {
                Ok(r) => *dest.write().unwrap() = SettingState::LoadOk(r),
                Err(e) => *dest.write().unwrap() = SettingState::LoadErr(e.to_string()),
            }
        },
        Err(e) => {
            *dest.write().unwrap() = SettingState::LoadErr(e.to_string());
        },
    }
}

impl TcuAdvSettingsUi {
    pub fn new(nag: Nag52Diag) -> Self {
        let is_ready = Arc::new(RwLock::new(PageLoadState::Waiting("Initializing")));
        let is_ready_t = is_ready.clone();

        let (tcc, tcc_t) = make_tcu_settings_wrapper::<TccSettings>();
        let (sol, sol_t) = make_tcu_settings_wrapper::<SolSettings>();
        let (sbs, sbs_t) = make_tcu_settings_wrapper::<SbsSettings>();
        let nag_c = nag.clone();
        std::thread::spawn(move|| {
            let res = nag_c.with_kwp(|x| {
                *is_ready_t.write().unwrap() = PageLoadState::Waiting("Setting TCU diag mode");
                x.kwp_set_session(0x93.into())
            });

            match res {
                Ok(_) => {
                    *is_ready_t.write().unwrap() = PageLoadState::Waiting("Reading TCC Settings")
                },
                Err(e) => {
                    *is_ready_t.write().unwrap() = PageLoadState::Err(e.to_string());
                    return;
                },
            };
            read_scn_settings(&nag_c, &tcc_t);
            read_scn_settings(&nag_c, &sol_t);
            read_scn_settings(&nag_c, &sbs_t);
            *is_ready_t.write().unwrap() = PageLoadState::Ok;
        });
        Self {
            ready: is_ready,
            nag,
            start_time: Instant::now(),
            tcc_settings: tcc,
            sol_settings: sol,
            sbs_settings: sbs
        }
    } 
}


pub fn make_settings_window<'de, T: TcuSettings>(nag: &Nag52Diag, settings_ref: &TcuSettingsWrapper<T>, ui: &mut eframe::egui::Ui) -> Option<PageAction>
where T: Clone + Copy + Serialize + DeserializeOwned {
    let mut action = None;
    let setting_state = settings_ref.read().unwrap().clone();
    if let SettingState::LoadOk(mut settings) = setting_state {
        Window::new(T::setting_name()).min_width(300.0).resizable(false).show(ui.ctx(), |ui| {
            ui.with_layout(Layout::top_down(eframe::emath::Align::Min), |ui| {
                ui.label(format!("Setting revision name: {}", T::get_revision_name()));
                if let Some(url) = T::wiki_url() {
                    ui.hyperlink_to(format!("Help on {}", T::setting_name()), url);
                }
                ScrollArea::new([false, true]).max_height(ui.available_height()/2.0).show(ui, |ui| {
                    let mut v = serde_yaml::to_value(&settings).unwrap();
                    make_ui_for_value(T::setting_name(), &mut v, ui);
                    if let Ok(s) = serde_yaml::from_value::<T>(v) {
                        settings = s;
                    }
                });

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
                                action = Some(PageAction::SendNotification { 
                                    text: format!("{} write OK!", T::setting_name()), 
                                    kind: egui_toast::ToastKind::Success 
                                })
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
                                action = Some(PageAction::SendNotification { 
                                    text: format!("{} reset OK!", T::setting_name()), 
                                    kind: egui_toast::ToastKind::Success 
                                });
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
            });
        });
        *settings_ref.write().unwrap() = SettingState::LoadOk(settings);
    } else if let SettingState::LoadErr(e) = setting_state {
        ui.label(format!("{} could not be read: {}", T::setting_name(), e));
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
        let mut action = None;
        action = make_settings_window(&self.nag, &self.tcc_settings, ui);
        if action.is_none() {
            action = make_settings_window(&self.nag, &self.sol_settings, ui);
        }
        if action.is_none() {
            action = make_settings_window(&self.nag, &self.sbs_settings, ui);
        }
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
        ui.heading("Key name");
        ui.heading("Value");
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
                ui.label(key);
                let mut o = v.as_bool().unwrap();
                ui.checkbox(&mut o, "");
                *v = Value::from(o);
                ui.end_row();
            } else if v.is_f64() {
                ui.label(format!("{key}: "));
                let mut o = v.as_f64().unwrap();
                let d = DragValue::new(&mut o).max_decimals(3).speed(0);
                ui.add(d);
                *v = Value::from(o);
                ui.end_row();
            } else if v.is_u64(){
                ui.label(format!("{key}: "));
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