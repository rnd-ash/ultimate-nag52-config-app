
use backend::{diag::{Nag52Diag}, ecu_diagnostics::{DiagServerResult, kwp2000::{KwpCommand, KwpSessionType}}};

use crate::window::PageAction;



pub struct AdaptResetPage {
    nag: Nag52Diag,
}

impl AdaptResetPage {
    pub fn new(nag: Nag52Diag) -> Self {
        Self {
            nag,
        }
        
    }
}

fn reset(nag: Nag52Diag) -> DiagServerResult<()> {
    nag.with_kwp(|kwp| kwp.kwp_set_session(KwpSessionType::ExtendedDiagnostics.into()))?;
    nag.with_kwp(|kwp| {
        kwp.send_byte_array_with_response(&[KwpCommand::StartRoutineByLocalIdentifier.into(), 0xDD], None)
    })?;
    Ok(())
}


impl crate::window::InterfacePage for AdaptResetPage {
    fn make_ui(&mut self, ui: &mut eframe::egui::Ui) -> crate::window::PageAction {
        let mut ret = PageAction::None;
        ui.heading("Adaptation reset");
        ui.label("
            This resets the shift adaptation values to default
        ");
        if ui.button("Reset shift adaptations").clicked() {
            if let Err(e) = reset(self.nag.clone()) {
                ret = PageAction::SendNotification { text: format!("Reset adaptations failed: {e}"), kind: egui_toast::ToastKind::Error }
            } else {
                ret = PageAction::SendNotification { text: format!("Reset adaptations OK!"), kind: egui_toast::ToastKind::Success }
            }
        }
        ret
    }

    fn should_show_statusbar(&self) -> bool {
        true
    }
}