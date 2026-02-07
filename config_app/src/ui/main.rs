use backend::diag::device_modes::TcuDeviceMode;
use backend::diag::DataState;
use backend::diag::ident::IdentData;
use backend::diag::Nag52Diag;
use config_app_macros::include_base64;
use eframe::egui;
use eframe::egui::CentralPanel;
use eframe::egui::RichText;
use eframe::egui::SidePanel;
use eframe::epaint::Color32;
use eframe::epaint::mutex::RwLock;
use std::sync::Arc;
use crate::window::{InterfacePage, PageAction};

use super::configuration::egs_config;
use super::settings_ui_gen::TcuAdvSettingsUi;
use super::updater::UpdatePage;
use super::{
    configuration::ConfigPage,
    map_editor::MapEditor, routine_tests::RoutinePage,
};
use crate::ui::diagnostics::DiagnosticsPage;

pub struct MainPage {
    diag_server: &'static mut Nag52Diag,
    info: Arc<RwLock<DataState<IdentData>>>,
    sn: Arc<RwLock<DataState<String>>>,
    first_run: bool,
    tcu_mode: Arc<RwLock<DataState<TcuDeviceMode>>>
}

impl MainPage {
    pub fn new(nag: Nag52Diag) -> Self {
        // Static mutable ref creation
        // this Nag52 lives the whole lifetime of the app once created,
        // so we have no need to clone it constantly, just throw the pointer around at
        // the subpages.
        //
        // We can keep it here as a ref to create a box from it when Drop() is called
        // so we can drop it safely without a memory leak
        let static_ref: &'static mut Nag52Diag = Box::leak(Box::new(nag));

        Self {
            diag_server: static_ref,
            info: Arc::new(RwLock::new(DataState::Unint)),
            sn: Arc::new(RwLock::new(DataState::Unint)),
            first_run: false,
            tcu_mode: Arc::new(RwLock::new(DataState::Unint)),
        }
    }
}

impl InterfacePage for MainPage {
    fn make_ui(&mut self, ui: &mut egui::Ui) -> crate::window::PageAction {
        if !self.first_run {
            self.first_run = true;
            return PageAction::RegisterNag(Arc::new(self.diag_server.clone()));
        }
        ui.vertical_centered(|x| {
            x.heading("Welcome to the Ultimate-NAG52 configuration app!");
            let os_logo = if cfg!(windows) {
                egui::special_emojis::OS_WINDOWS
            } else if cfg!(target_os = "linux") {
                egui::special_emojis::OS_LINUX
            } else {
                egui::special_emojis::OS_APPLE
            };
            x.label(format!("Config app version {} for {} (Build {})", env!("CARGO_PKG_VERSION"), os_logo, env!("GIT_BUILD")));
            if env!("GIT_BUILD").ends_with("-dirty") || env!("GIT_BUILD") == "UNKNOWN" {
                x.strong(RichText::new("Warning. You have a modified or testing version of the config app! Bugs may be present!").color(Color32::RED));
            } else {
                // Check for updates
            }
            let link = if env!("GIT_BRANCH").contains("main") {
                include_base64!("aHR0cHM6Ly9naXRodWIuY29tL3JuZC1hc2gvdWx0aW1hdGUtbmFnNTItY29uZmlnLWFwcC9yZWxlYXNlcz9xPW1haW4mZXhwYW5kZWQ9dHJ1ZQ")
            } else {
                include_base64!("aHR0cHM6Ly9naXRodWIuY29tL3JuZC1hc2gvdWx0aW1hdGUtbmFnNTItY29uZmlnLWFwcC9yZWxlYXNlcz9xPWRldiZleHBhbmRlZD10cnVl")
            };
            x.hyperlink_to("View config app updates", link);
        });
        ui.separator();
        
        let mut create_page = None;
        let info_state = self.info.read().clone();
        let mode_state = self.tcu_mode.read().clone();

        let mut efuse_ok = true;
        let mut compatibility_ok = true;
        let mut special_mode = false;
        let w = SidePanel::left("l-s").resizable(false).show_inside(ui, |ui| {
            // Left panel (Status)
            ui.vertical_centered(|ui| {
                ui.heading("Status");
            });
            ui.add_space(20.0);
            egui::Grid::new("inf-tab")
            .striped(true)
            .show(ui, |ui| {

                fn datastate_to_ui<T, F: FnOnce(&mut egui::Ui, &T)>(ui: &mut egui::Ui, state: &DataState<T>, fn_ok: F) {
                    match state {
                        DataState::LoadOk(t) => fn_ok(ui, t),
                        DataState::Unint => {
                            ui.spinner();
                        },
                        DataState::LoadErr(e) => {
                            ui.colored_label(Color32::RED, format!("Error querying status: {e}"));
                        },
                    }
                }

                ui.strong("TCU Status");
                datastate_to_ui(ui, &mode_state, |ui, mode| {
                    if mode.contains(TcuDeviceMode::NO_CALIBRATION) {
                        ui.colored_label(Color32::RED, 
                            "No EGS Calibration data selected"  
                        );
                        compatibility_ok = false;
                    } else if mode.contains(TcuDeviceMode::NO_EFUSE) {
                        ui.colored_label(Color32::RED, 
                            "No vehicle information or missing EFUSE data"  
                        );
                        efuse_ok = false;
                    } else if mode.contains(TcuDeviceMode::CANLOGGER) {
                        ui.colored_label(Color32::RED, 
                            "Special mode in use - CAN Logger"  
                        );
                        special_mode = true;
                    } else if mode.contains(TcuDeviceMode::SLAVE) {
                        ui.colored_label(Color32::RED, 
                            "Special mode in use - Slave CAN Manipulator"  
                        );
                        special_mode = true;
                    } else if mode.contains(TcuDeviceMode::ERROR) {
                        ui.colored_label(Color32::RED, 
                            "Your TCU has encountered an error. Please consult the LOG window to
                            see what is wrong."  
                        );
                    } else {
                        ui.colored_label(Color32::GREEN, "TCU is running normally");
                    }
                });
                ui.end_row();

                ui.strong("Serial number");
                datastate_to_ui(ui, &self.sn.read(), |ui, sn| {
                    ui.label(sn);
                });
                ui.end_row();

                ui.strong("PCB Version");
                datastate_to_ui(ui, &info_state, |ui, inf| {
                    ui.label(format!(
                        "{} (HW date: {} week 20{})",
                        inf.board_ver, inf.hw_week, inf.hw_year
                    ));
                });
                ui.end_row();

                ui.strong("Production date");
                datastate_to_ui(ui, &info_state, |ui, inf| {
                    ui.label(format!(
                        "{}/{}/20{}",
                        inf.manf_day, inf.manf_month, inf.manf_year
                    ));
                });
                ui.end_row();

                ui.strong("Software date");
                datastate_to_ui(ui, &info_state, |ui, inf| {
                    ui.label(format!(
                        "Week {} of 20{}",
                        inf.sw_week, inf.sw_year
                    ));
                });
                ui.end_row();

                ui.strong("CAN Layer selected");
                datastate_to_ui(ui, &info_state, |ui, inf| {
                    ui.label(format!("{}", inf.egs_mode));
                });
                ui.end_row();
            });
        }).response.rect.width();
        SidePanel::right("r-s").exact_width(w).resizable(false).show_inside(ui, |ui| {
            // Right panel (links)
            ui.vertical_centered(|ui| {
                ui.heading("Resources");
            });
            ui.add_space(20.0);
            ui.hyperlink_to("📢 Announcements 📢", include_base64!("aHR0cHM6Ly9kb2NzLnVsdGltYXRlLW5hZzUyLm5ldC9lbi9hbm5vdW5jZW1lbnRz"));
            // Weblinks are base64 encoded to avoid potential scraping
            ui.hyperlink_to(format!("📓 Ultimate-NAG52 wiki"), include_base64!("aHR0cHM6Ly9kb2NzLnVsdGltYXRlLW5hZzUyLm5ldA"));
            ui.hyperlink_to(format!("💁 Ultimate-NAG52 dicsussion group"), include_base64!("aHR0cHM6Ly90Lm1lLyt3dU5wZkhua0tTQmpNV0pr"));
            ui.hyperlink_to(format!(" Project progress playlist"), include_base64!("aHR0cHM6Ly93d3cueW91dHViZS5jb20vcGxheWxpc3Q_bGlzdD1QTHhydy00VnQ3eHR1OWQ4bENrTUNHMF9LN29IY3NTTXRG"));
            ui.label("Code repositories");
            ui.hyperlink_to(format!(" The configuration app"), include_base64!("aHR0cHM6Ly9naXRodWIuY29tL3JuZC1hc2gvdWx0aW1hdGUtbmFnNTItY29uZmlnLWFwcA"));
            ui.hyperlink_to(format!(" TCU Firmware"), include_base64!("aHR0cDovL2dpdGh1Yi5jb20vcm5kLWFzaC91bHRpbWF0ZS1uYWc1Mi1mdw"));
        });
        CentralPanel::default().show_inside(ui, |ui| {
            // Action panel
            ui.vertical_centered(|ui| {
                ui.heading("Tools");
            });
            ui.add_space(20.0);
            ui.vertical_centered(|ui|{
                if !efuse_ok {
                    // Efuse must be done
                    ui.label("You must configure vehicle settings or EFUSE before you can use your TCU");
                } else if !compatibility_ok {
                    // EGS compatibility must be done
                    ui.label("You must apply EGS calibrations before you can use your TCU");
                } else if special_mode {
                    // Only special mode can be adjusted
                    ui.label("You must deactivate your TCUs special mode before continuing");
                }
                ui.strong("⚙ Core configuration");
                ui.add_enabled_ui(!special_mode && efuse_ok, |v| {
                    if v.button("Configure EGS calibration data").clicked() {
                        create_page = Some(
                            PageAction::Add(Box::new(
                                egs_config::EgsConfigPage::new(self.diag_server.clone())
                            ))
                        );
                    }
                });
                ui.add_enabled_ui(!special_mode, |v| {
                    if v.button("Configure vehicle parameters").clicked() {
                        create_page = Some(PageAction::Add(Box::new(ConfigPage::new(
                            self.diag_server.clone(),
                        ))));
                    }
                });
                ui.add_space(10.0);
                ui.strong("🔥 Tuning");
                ui.add_enabled_ui(!special_mode && efuse_ok && compatibility_ok, |v| {
                    if v.button("Map Tuner").clicked() {
                        create_page = Some(PageAction::Add(Box::new(MapEditor::new(
                            self.diag_server.clone(),
                        ))));
                    }
                });
                ui.add_enabled_ui(!special_mode && efuse_ok && compatibility_ok, |v| {
                    if v.button("TCU settings").clicked() {
                        create_page = Some(PageAction::Add(Box::new(TcuAdvSettingsUi::new(
                            self.diag_server.clone(),
                            v.ctx().clone(),
                        ))));
                    }
                });
                ui.add_space(10.0);
                ui.strong("🛠 Utilities");
                ui.add_enabled_ui(efuse_ok, |v| {
                    if v.button("Updater").clicked() {
                        create_page = Some(PageAction::Add(Box::new(UpdatePage::new(
                            self.diag_server.clone(),
                        ))));
                    }
                });
                ui.add_enabled_ui(!special_mode && efuse_ok && compatibility_ok, |v| {
                    if v.button("Data logger").clicked() {
                        create_page = Some(PageAction::Add(Box::new(DiagnosticsPage::new(
                            self.diag_server.clone(),
                            v.ctx().clone()
                        ))));
                    }
                });
                ui.add_enabled_ui(efuse_ok, |v| {
                    if v.button("Routine executor").clicked() {
                        create_page = Some(PageAction::Add(Box::new(RoutinePage::new(
                            self.diag_server.clone(),
                        ))));
                    }
                });
            });
        });

        if let Some(page) = create_page {
            return page;
        }
        PageAction::None
    }

    fn should_show_statusbar(&self) -> bool {
        true
    }

    fn destroy_nag(&self) -> bool {
        true
    }

    fn on_load(&mut self, _nag: Option<Arc<Nag52Diag>>) {
        let tcu = self.diag_server.clone();
        let setting_lock = self.info.clone();
        let sn_lock = self.sn.clone();
        let mode_lock = self.tcu_mode.clone();
        std::thread::spawn(move|| {
            let state = match tcu.query_ecu_data() {
                Ok(info) => DataState::LoadOk(info),
                Err(err) => DataState::LoadErr(err.to_string()),
            };
            *setting_lock.write() = state;
            let state: DataState<String> = match tcu.get_ecu_sn() {
                Ok(sn) => DataState::LoadOk(sn),
                Err(err) => DataState::LoadErr(err.to_string()),
            };
            *sn_lock.write() = state;
            let state: DataState<TcuDeviceMode> = match tcu.read_device_mode() {
                Ok(sn) => DataState::LoadOk(sn),
                Err(err) => DataState::LoadErr(err.to_string()),
            };
            *mode_lock.write() = state;
        });
    }

}

impl Drop for MainPage {
    fn drop(&mut self) {
        // Create a temp box so we can drop it
        let b = unsafe { Box::from_raw(self.diag_server) };
        drop(b);
    }
}
