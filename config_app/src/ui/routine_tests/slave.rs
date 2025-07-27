use std::time::Duration;

use backend::{diag::{Nag52Diag, device_modes::TcuDeviceMode}, ecu_diagnostics::{kwp2000::{KwpSessionType, ResetType}, DiagServerResult}};
use eframe::egui::Context;

use crate::window::PageAction;



pub struct SlaveModePage {
    device_mode: TcuDeviceMode,
    nag: Nag52Diag,
}

impl SlaveModePage {
    pub fn new(nag: Nag52Diag, ctx: Context) -> Self {
        let mode = nag.read_device_mode().unwrap_or(TcuDeviceMode::NORMAL);
        Self {
            device_mode: mode,
            nag,
        }
        
    }
}

fn set_mode_and_reboot(nag: Nag52Diag, mode: TcuDeviceMode) -> DiagServerResult<TcuDeviceMode> {
    nag.with_kwp(|kwp| kwp.kwp_set_session(KwpSessionType::ExtendedDiagnostics.into()))?;
    nag.set_device_mode(mode, true)?;
    nag.with_kwp(|kwp| {
        kwp.kwp_reset_ecu(ResetType::PowerOnReset)?;
        std::thread::sleep(Duration::from_millis(500));
        kwp.kwp_set_session(KwpSessionType::ExtendedDiagnostics.into())
    })?;
    nag.read_device_mode()
}


impl crate::window::InterfacePage for SlaveModePage {
    fn make_ui(&mut self, ui: &mut eframe::egui::Ui, frame: &eframe::Frame) -> crate::window::PageAction {
        ui.heading("Slave mode toggle");
        let mut t_mode = self.device_mode;
        if self.device_mode.contains(TcuDeviceMode::SLAVE) {
            // Already in CAN Logger mode
            if ui.button("Disable Slave mode").clicked() {
                t_mode = TcuDeviceMode::NORMAL;
            }
            
        } else {
            // Not in logger mode
            if ui.button("Enable Slave mode").clicked() {
                t_mode = TcuDeviceMode::SLAVE;
            }
        }
        if t_mode != self.device_mode {
            if let Ok(mode) = set_mode_and_reboot(self.nag.clone(), t_mode) {
                self.device_mode = mode;
            }
        }
        PageAction::None
    }

    fn get_title(&self) -> &'static str {
        "CAN Logger"
    }

    fn should_show_statusbar(&self) -> bool {
        true
    }
}