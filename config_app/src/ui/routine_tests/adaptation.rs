use backend::diag::Nag52Diag;

use crate::window::PageAction;



pub struct AdaptationViewerPage {
    nag: Nag52Diag,
    
}

impl AdaptationViewerPage {
    pub fn new(nag: Nag52Diag) -> Self {
        Self {
            nag,
        }
    }
}

impl crate::window::InterfacePage for AdaptationViewerPage {
    fn make_ui(&mut self, ui: &mut eframe::egui::Ui, frame: &eframe::Frame) -> crate::window::PageAction {
        ui.heading("Adaptation viewer");
        PageAction::None
    }

    fn get_title(&self) -> &'static str {
        "Adaptation viewer"
    }

    fn should_show_statusbar(&self) -> bool {
        true
    }
}