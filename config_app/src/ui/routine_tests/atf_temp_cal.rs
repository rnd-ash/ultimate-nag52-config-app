use std::sync::{Arc, RwLock, atomic::{AtomicBool, Ordering}};

use backend::{diag::Nag52Diag, ecu_diagnostics::kwp2000::KwpSessionType};
use eframe::egui::Context;

use crate::window::PageAction;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
enum AtfCommand {
    ResetCalibration = 2,
    CalibrateToEngineOilTemp = 0
}

pub struct AtfTempCalibrationPage {
    nag: Nag52Diag,
    status: Arc<RwLock<String>>,
    running: Arc<AtomicBool>
}

impl AtfTempCalibrationPage {
    pub fn new(nag: Nag52Diag) -> Self {
        Self {
            nag,
            running: Arc::new(AtomicBool::new(false)),
            status: Arc::new(RwLock::new(String::new()))
        }
    }
}

impl AtfTempCalibrationPage {
    fn run_command(&mut self, mode: AtfCommand, ctx: Context) {
        let nag_c = self.nag.clone();
        let status_c = self.status.clone();
        let running_c = self.running.clone();
        std::thread::spawn(move|| {
            running_c.store(true, Ordering::Relaxed);
            *status_c.write().unwrap() = String::new();
            ctx.request_repaint();

            //let res = nag_c.with_kwp(|kwp| {
            //    kwp.kwp_set_session(KwpSessionType::ExtendedDiagnostics.into())?;
            //    kwp.send_byte_array_with_response(&[0x31, 0x33, mode as u8])?;
            //    kwp.kwp_set_session(KwpSessionType::Normal.into())?;
            //    Ok(())
            //});

            //*status_c.write().unwrap() = match res {
            //    Ok(_) => "Operation completed successfully".into(),
            //    Err(e) => format!("Operation failed. Error: {e:?}"),
            //};

            running_c.store(false, Ordering::Relaxed);
            ctx.request_repaint();
        });
    }
}

impl crate::window::InterfacePage for AtfTempCalibrationPage {
    fn make_ui(&mut self, ui: &mut eframe::egui::Ui, frame: &eframe::Frame) -> crate::window::PageAction {
        ui.heading("ATF Curve calibration");
        ui.label("
            In order to compensate for variances in PCB to PCB, and ATF temp sensors,
            if you are experiencing inconsistent shifting, you can calibrate the curve of the ATF temperature 
            sensor reading here.

            This test MUST ONLY be performed after the car has been off overnight. This allows the temperature
            of the engine oil and transmission oil to both settle to ambiant temperature.

            Trying to perform this with a warm engine WILL RESULT IN MASSVIELY OUT OF SPEC ATF TEMPERATURE!
        ");

        if !self.running.load(Ordering::Relaxed) {
            ui.vertical(|ui| {
                if ui.button("Reset calibration (TCU Default)").clicked() {
                    self.run_command(AtfCommand::ResetCalibration, ui.ctx().clone())
                }
                if ui.button("Calibrate to match engine oil temperature").clicked() {
                    self.run_command(AtfCommand::CalibrateToEngineOilTemp, ui.ctx().clone())
                }
            });
        } else {
            ui.label("Routine running...");
        }
        ui.label(self.status.read().unwrap().clone());

        PageAction::None
    }

    fn get_title(&self) -> &'static str {
        "ATF Curve calibration"
    }

    fn should_show_statusbar(&self) -> bool {
        true
    }
}