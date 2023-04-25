use std::sync::{Arc, Mutex};

use backend::diag::Nag52Diag;

use crate::window::PageAction;

use self::solenoid_test::SolenoidTestPage;

pub mod solenoid_test;

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
            Select test routine to run
        ",
        );

        let mut page_action = PageAction::None;

        if ui.button("Solenoid test").clicked() {
            page_action = PageAction::Add(Box::new(SolenoidTestPage::new(
                self.nag.clone()
            )));
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
