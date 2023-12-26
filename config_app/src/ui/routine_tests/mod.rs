use std::sync::{Arc, Mutex};

use backend::diag::Nag52Diag;

use crate::window::PageAction;

use self::{solenoid_test::SolenoidTestPage, adaptation::AdaptationViewerPage, tcc_control::TccControlPage, canlogger::CanLoggerPage, atf_temp_cal::AtfTempCalibrationPage};

pub mod solenoid_test;
pub mod adaptation;
pub mod tcc_control;
pub mod canlogger;
pub mod atf_temp_cal;

pub struct RoutinePage {
    nag: Nag52Diag,
}

impl RoutinePage {
    pub fn new(nag: Nag52Diag) -> Self {
        Self { nag }
    }
}

impl crate::window::InterfacePage for RoutinePage {
    fn make_ui(
        &mut self,
        ui: &mut eframe::egui::Ui,
        frame: &eframe::Frame,
    ) -> crate::window::PageAction {
        ui.heading("Diagnostic routines");

        ui.label(
            "
            Here you can run some diagnostics on your transmission and TCU, as well as reset adaptation data that the TCU has done.

            NOTE: It is recommended to always reset your adaptation after changing ATF or doing any maintenence on the gearbox!
        ",
        );
        ui.separator();
        let mut page_action = PageAction::None;
        ui.label(
            "
            Run the solenoid test to test if any of gearbox's solenoids are bad
        ",
        );
        if ui.button("Solenoid test").clicked() {
            page_action = PageAction::Add(Box::new(SolenoidTestPage::new(
                self.nag.clone()
            )));
        }

        ui.label(
            "
            Check or reset the TCUs adaptation
        ",
        );
        if ui.button("Adaptation view / reset").clicked() {
            page_action = PageAction::Add(Box::new(AdaptationViewerPage::new(
                self.nag.clone()
            )));
        }

        ui.label(
            "
            Enable or disable the Torque converter (TCC) control solenoid in order to help
            diagnosis of any vibrations in the vehicle
        ",
        );
        if ui.button("TCC solenoid toggler").clicked() {
            page_action = PageAction::Add(Box::new(TccControlPage::new(
                self.nag.clone()
            )));
        }

        ui.label(
            "
            Calibrate ATF temperature curve to remove board to board variance
        ",
        );
        if ui.button("ATF curve calibration").clicked() {
            page_action = PageAction::Add(Box::new(AtfTempCalibrationPage::new(
                self.nag.clone()
            )));
        }

        if self.nag.has_logger() { // AKA USB connection

            ui.label(
                "
            DEBUGGING ONLY! 
            Can logger - Turn your TCU into a CANbus logging device.

            NOTE: This can ONLY be done with a USB connection to the TCU
            "
            );
            if ui.button("CAN Logger").clicked() {
                page_action = PageAction::Add(Box::new(CanLoggerPage::new(
                    self.nag.clone(), ui.ctx().clone()
                )));
            }
        }

        page_action
    }

    fn get_title(&self) -> &'static str {
        "Routine executor"
    }

    fn should_show_statusbar(&self) -> bool {
        true
    }
}
