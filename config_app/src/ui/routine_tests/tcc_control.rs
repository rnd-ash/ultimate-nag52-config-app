use std::sync::{Arc, RwLock, atomic::{AtomicBool, Ordering}};

use backend::{diag::Nag52Diag, ecu_diagnostics::kwp2000::KwpSessionType};
use eframe::egui::Context;

use crate::window::PageAction;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
enum TccCommand {
    EnableTcc = 2,
    DisableTcc = 0
}

pub struct TccControlPage {
    nag: Nag52Diag,
    status: Arc<RwLock<String>>,
    running: Arc<AtomicBool>
}

impl TccControlPage {
    pub fn new(nag: Nag52Diag) -> Self {
        Self {
            nag,
            running: Arc::new(AtomicBool::new(false)),
            status: Arc::new(RwLock::new(String::new()))
        }
    }
}

impl TccControlPage {
    fn run_tcc_control(&mut self, mode: TccCommand, ctx: Context) {
        let nag_c = self.nag.clone();
        let status_c = self.status.clone();
        let running_c = self.running.clone();
        std::thread::spawn(move|| {
            running_c.store(true, Ordering::Relaxed);
            *status_c.write().unwrap() = String::new();
            ctx.request_repaint();

            let res = nag_c.with_kwp(|kwp| {
                kwp.kwp_set_session(KwpSessionType::ExtendedDiagnostics.into())?;
                kwp.send_byte_array_with_response(&[0x31, 0x33, mode as u8])?;
                kwp.kwp_set_session(KwpSessionType::Normal.into())?;
                Ok(())
            });

            *status_c.write().unwrap() = match res {
                Ok(_) => "Operation completed successfully".into(),
                Err(e) => format!("Operation failed. Error: {e:?}"),
            };

            running_c.store(false, Ordering::Relaxed);
            ctx.request_repaint();
        });
    }
}

impl crate::window::InterfacePage for TccControlPage {
    fn make_ui(&mut self, ui: &mut eframe::egui::Ui, frame: &eframe::Frame) -> crate::window::PageAction {
        ui.heading("Torque converter solenoid control");
        ui.label("
            Here you can either enable or disable control of the Torque converter (TCC)
            solenoid in order to diagnose any issues with vibrations in the vehicle.

            NOTE: Disabling the TCC solenoid will only persist until ignition off. The next time
            the vehicle is turned on, the TCC solenoid will be back to active again.
        ");

        if !self.running.load(Ordering::Relaxed) {
            ui.horizontal(|ui| {
                if ui.button("Enable the TCC solenoid").clicked() {
                    self.run_tcc_control(TccCommand::EnableTcc, ui.ctx().clone())
                }
                if ui.button("Disable the TCC solenoid").clicked() {
                    self.run_tcc_control(TccCommand::DisableTcc, ui.ctx().clone())
                }
            });
        } else {
            ui.label("Routine running...");
        }
        ui.label(self.status.read().unwrap().clone());

        PageAction::None
    }

    fn get_title(&self) -> &'static str {
        "TCC control"
    }

    fn should_show_statusbar(&self) -> bool {
        true
    }
}