use backend::diag::device_modes::TcuDeviceMode;
use backend::diag::DataState;
use backend::diag::ident::IdentData;
use backend::diag::Nag52Diag;
use config_app_macros::include_base64;
use eframe::egui;
use eframe::Frame;
use eframe::egui::RichText;
use eframe::epaint::Color32;
use eframe::epaint::mutex::RwLock;
use std::sync::Arc;
use crate::window::{InterfacePage, PageAction};

use super::configuration::egs_config;
use super::settings_ui_gen::TcuAdvSettingsUi;
use super::updater::UpdatePage;
use super::{
    configuration::ConfigPage,
    diagnostics::solenoids::SolenoidPage,
    io_maipulator::IoManipulatorPage, map_editor::MapEditor, routine_tests::RoutinePage,
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
    fn make_ui(&mut self, ui: &mut egui::Ui, frame: &Frame) -> crate::window::PageAction {
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
        ui.label(r#"
            This application lets you do many things with the TCU!
            If you are lost or need help, you can always consult the wiki below,
            or join the Ultimate-NAG52 discussions Telegram group!
        "#);
        ui.collapsing("Useful links", |ui| {
            ui.hyperlink_to("📢 Announcements 📢", include_base64!("aHR0cHM6Ly9kb2NzLnVsdGltYXRlLW5hZzUyLm5ldC9lbi9hbm5vdW5jZW1lbnRz"));
            // Weblinks are base64 encoded to avoid potential scraping
            ui.hyperlink_to(format!("📓 Ultimate-NAG52 wiki"), include_base64!("ZG9jcy51bHRpbWF0ZS1uYWc1Mi5uZXQ"));
            ui.hyperlink_to(format!("💁 Ultimate-NAG52 dicsussion group"), include_base64!("aHR0cHM6Ly90Lm1lLyt3dU5wZkhua0tTQmpNV0pr"));
            ui.hyperlink_to(format!(" Project progress playlist"), include_base64!("aHR0cHM6Ly93d3cueW91dHViZS5jb20vcGxheWxpc3Q_bGlzdD1QTHhydy00VnQ3eHR1OWQ4bENrTUNHMF9LN29IY3NTTXRG"));
            ui.label("Code repositories");
            ui.hyperlink_to(format!(" The configuration app"), include_base64!("aHR0cHM6Ly9naXRodWIuY29tL3JuZC1hc2gvdWx0aW1hdGUtbmFnNTItY29uZmlnLWFwcA"));
            ui.hyperlink_to(format!(" TCU Firmware"), include_base64!("aHR0cDovL2dpdGh1Yi5jb20vcm5kLWFzaC91bHRpbWF0ZS1uYWc1Mi1mdw"));
        });
        ui.add(egui::Separator::default());
        let mut create_page = None;
        let ctx = ui.ctx().clone();
        if let DataState::LoadOk(mode) = self.tcu_mode.read().clone() {
            ui.vertical_centered(|ui| {
                ui.heading("TCU Status");
                if mode.contains(TcuDeviceMode::NO_CALIBRATION) {
                    ui.colored_label(Color32::RED, 
                        "Your TCU requires calibrations, and will NOT function. Please go to the EGS compatibility page
                        to correct this!"  
                    );
                } else if mode.contains(TcuDeviceMode::NO_EFUSE) {
                    ui.colored_label(Color32::RED, 
                        "Your TCU is freshly built and requires EFUSE configuration. Go to the configuration page
                        to correct this!"  
                    );
                } else if mode.contains(TcuDeviceMode::CANLOGGER) {
                    ui.colored_label(Color32::RED, 
                        "Your TCU is in CAN logging mode, and will NOT function. To disable this,
                        please go to the Diagnostic routine executor page, and then CAN Logger."  
                    );
                } else if mode.contains(TcuDeviceMode::SLAVE) {
                    ui.colored_label(Color32::RED, 
                        "Your TCU is in slave mode! It will NOT function."  
                    );
                } else if mode.contains(TcuDeviceMode::ERROR) {
                    ui.colored_label(Color32::RED, 
                        "Your TCU has encountered an error. Please consult the LOG window to
                        see what is wrong."  
                    );
                } else {
                    ui.label("TCU is running normally.");
                }
                ui.separator();
            });
        }
        
        ui.vertical_centered(|v| {
            v.heading("Tools");
            if v.button("Updater").clicked() {
                create_page = Some(PageAction::Add(Box::new(UpdatePage::new(
                    self.diag_server.clone(),
                ))));
            }
            if v.button("Diagnostics").clicked() {
                create_page = Some(PageAction::Add(Box::new(DiagnosticsPage::new(
                    self.diag_server.clone(),
                    ctx.clone()
                ))));
            }
            if v.button("Solenoid live view").clicked() {
                create_page = Some(PageAction::Add(Box::new(SolenoidPage::new(
                    self.diag_server.clone(),
                    ctx.clone()
                ))));
            }
            if v.button("IO Manipulator").clicked() {
                create_page = Some(PageAction::Add(Box::new(IoManipulatorPage::new(
                    self.diag_server.clone(),
                ))));
            }
            if v.button("Diagnostic routine executor").clicked() {
                create_page = Some(PageAction::Add(Box::new(RoutinePage::new(
                    self.diag_server.clone(),
                ))));
            }
            if v.button("Map Tuner").clicked() {
                create_page = Some(PageAction::Add(Box::new(MapEditor::new(
                    self.diag_server.clone(),
                ))));
            }
            if v.button("TCU Program settings").on_hover_text("CAUTION. DANGEROUS!").clicked() {
                create_page = Some(PageAction::Add(Box::new(TcuAdvSettingsUi::new(
                    self.diag_server.clone(),
                    ctx,
                ))));
            }
            if v.button("Configure EGS compatibility data").clicked() {
                create_page = Some(
                    PageAction::Add(Box::new(
                        egs_config::EgsConfigPage::new(self.diag_server.clone())
                    ))
                );
            }
            if v.button("Configure drive profiles").clicked() {
                create_page = Some(
                    PageAction::SendNotification {
                        text: "You have found a unimplemented feature!".into(),
                        kind: egui_notify::ToastLevel::Info
                    }
                );
            }
            if v.button("Configure vehicle / gearbox").clicked() {
                create_page = Some(PageAction::Add(Box::new(ConfigPage::new(
                    self.diag_server.clone(),
                ))));
            }
        });


        if let Some(page) = create_page {
            return page;
        }

        let info_state = self.info.read().clone();
        match info_state {
            DataState::Unint => { ui.spinner(); },
            DataState::LoadErr(e) => { ui.label(format!("Could not query ECU Ident data: {e}")); },
            DataState::LoadOk(info) => {
                ui.collapsing("Show TCU Info", |ui| {
                    ui.label(format!(
                        "ECU Serial number: {}",
                        match self.sn.read().clone() {
                            DataState::LoadOk(s) => s,
                            DataState::Unint => "...".to_string(),
                            DataState::LoadErr(_) => "Unknown".to_string(),
                        }
                    ));
                    ui.label(format!(
                        "PCB Version: {} (HW date: {} week 20{})",
                        info.board_ver, info.hw_week, info.hw_year
                    ));
                    ui.label(format!(
                        "PCB Production date: {}/{}/20{}",
                        info.manf_day, info.manf_month, info.manf_year
                    ));
                    ui.label(format!(
                        "PCB Software date: week {} of 20{}",
                        info.sw_week, info.sw_year
                    ));
                    ui
                        .label(format!("EGS CAN Matrix selected: {}", info.egs_mode));
                });
            }
        }
        PageAction::None
    }

    fn get_title(&self) -> &'static str {
        "Ultimate-Nag52 configuration utility (Home)"
    }

    fn should_show_statusbar(&self) -> bool {
        true
    }

    fn destroy_nag(&self) -> bool {
        true
    }

    fn on_load(&mut self, nag: Option<Arc<Nag52Diag>>) {
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
