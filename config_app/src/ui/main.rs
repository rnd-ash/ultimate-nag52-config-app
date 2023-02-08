use backend::diag::ident::IdentData;
use backend::diag::Nag52Diag;
use eframe::egui;
use eframe::Frame;
use std::sync::{mpsc, Arc, Mutex};

use crate::window::{InterfacePage, PageAction};

use super::updater::UpdatePage;
use super::{
    configuration::ConfigPage, crashanalyzer::CrashAnalyzerUI,
    diagnostics::solenoids::SolenoidPage, firmware_update::FwUpdateUI,
    io_maipulator::IoManipulatorPage, map_editor::MapEditor, routine_tests::RoutinePage,
    status_bar::MainStatusBar,
};
use crate::ui::diagnostics::DiagnosticsPage;

pub struct MainPage {
    bar: MainStatusBar,
    show_about_ui: bool,
    diag_server: Nag52Diag,
    info: Option<IdentData>,
}

impl MainPage {
    pub fn new(nag: Nag52Diag) -> Self {
        Self {
            bar: MainStatusBar::new(),
            show_about_ui: false,
            diag_server: nag,
            info: None,
        }
    }
}

impl InterfacePage for MainPage {
    fn make_ui(&mut self, ui: &mut egui::Ui, frame: &Frame) -> crate::window::PageAction {
        // UI context menu
        egui::menu::bar(ui, |bar_ui| {
            bar_ui.menu_button("File", |x| {
                if x.button("Quit").clicked() {
                    //TODO
                }
                if x.button("About").clicked() {
                    if let Ok(ident) = self.diag_server.query_ecu_data() {
                        self.info = Some(ident);
                    }
                    self.show_about_ui = true;
                }
            })
        });
        ui.add(egui::Separator::default());
        let mut create_page = None;
        ui.vertical(|v| {
            v.heading("Utilities");
            v.label("Legacy (Use Updater on newer FW)");
            if v.button("Firmware updater")
                .on_disabled_hover_ui(|u| {
                    u.label("Broken, will be added soon!");
                })
                .clicked()
            {
                create_page = Some(PageAction::Add(Box::new(FwUpdateUI::new(
                    self.diag_server.clone(),
                ))));
            }
            if v.button("Crash analyzer").clicked() {
                create_page = Some(PageAction::Add(Box::new(CrashAnalyzerUI::new(
                    self.diag_server.clone(),
                ))));
            }
            v.label("New!");
            if v.button("Updater").clicked() {
                create_page = Some(PageAction::Add(Box::new(UpdatePage::new(
                    self.diag_server.clone(),
                    self.bar.clone(),
                ))));
            }
            if v.button("Diagnostics").clicked() {
                create_page = Some(PageAction::Add(Box::new(DiagnosticsPage::new(
                    self.diag_server.clone(),
                    self.bar.clone(),
                ))));
            }
            if v.button("Solenoid live view").clicked() {
                create_page = Some(PageAction::Add(Box::new(SolenoidPage::new(
                    self.diag_server.clone(),
                    self.bar.clone(),
                ))));
            }
            if v.button("IO Manipulator").clicked() {
                create_page = Some(PageAction::Add(Box::new(IoManipulatorPage::new(
                    self.diag_server.clone(),
                    self.bar.clone(),
                ))));
            }
            if v.button("Diagnostic routine executor").clicked() {
                create_page = Some(PageAction::Add(Box::new(RoutinePage::new(
                    self.diag_server.clone(),
                    self.bar.clone(),
                ))));
            }
            if v.button("Map tuner").clicked() {
                create_page = Some(PageAction::Add(Box::new(MapEditor::new(
                    self.diag_server.clone(),
                    self.bar.clone(),
                ))));
            }
            if v.button("Configure drive profiles").clicked() {}
            if v.button("Configure vehicle / gearbox").clicked() {
                create_page = Some(PageAction::Add(Box::new(ConfigPage::new(
                    self.diag_server.clone(),
                    self.bar.clone(),
                ))));
            }
        });
        if let Some(page) = create_page {
            return page;
        }

        if self.show_about_ui {
            egui::containers::Window::new("About")
                .resizable(false)
                .collapsible(false)
                .default_pos(&[400f32, 300f32])
                .show(ui.ctx(), |win| {
                    win.vertical(|about_cols| {
                        about_cols.heading("Version data");
                        about_cols.label(format!(
                            "Configuration app version: {}",
                            env!("CARGO_PKG_VERSION")
                        ));
                        about_cols.separator();
                        if let Some(ident) = self.info {
                            about_cols.heading("TCU Data");
                            about_cols.label(format!(
                                "PCB Version: {} (HW date: {} week 20{})",
                                ident.board_ver, ident.hw_week, ident.hw_year
                            ));
                            about_cols.label(format!(
                                "PCB Production date: {}/{}/20{}",
                                ident.manf_day, ident.manf_month, ident.manf_year
                            ));
                            about_cols.label(format!(
                                "PCB Software date: week {} of 20{}",
                                ident.sw_week, ident.sw_year
                            ));
                            about_cols
                                .label(format!("EGS CAN Matrix selected: {}", ident.egs_mode));
                        } else {
                            about_cols.heading("Could not read TCU Ident data");
                        }

                        about_cols.separator();
                        about_cols.heading("Open source");
                        about_cols.add(egui::Hyperlink::from_label_and_url(
                            "Github repository (Configuration utility)",
                            "https://github.com/rnd-ash/ultimate_nag52/tree/main/config_app",
                        ));
                        about_cols.add(egui::Hyperlink::from_label_and_url(
                            "Github repository (TCM source code)",
                            "https://github.com/rnd-ash/ultimate-nag52-fw",
                        ));
                        about_cols.separator();
                        about_cols.heading("Author");
                        about_cols.add(egui::Hyperlink::from_label_and_url(
                            "rnd-ash",
                            "https://github.com/rnd-ash",
                        ));
                        if about_cols.button("Close").clicked() {
                            self.show_about_ui = false;
                        }
                    })
                });
        }

        PageAction::None
    }

    fn get_title(&self) -> &'static str {
        "Ultimate-Nag52 configuration utility (Home)"
    }

    fn get_status_bar(&self) -> Option<Box<dyn crate::window::StatusBar>> {
        Some(Box::new(self.bar.clone()))
    }
}
