use std::{sync::Arc, time::Duration};

use backend::{diag::{Nag52Diag, device_modes::TcuDeviceMode}, ecu_diagnostics::{kwp2000::{KwpSessionType, ResetType}, DiagServerResult}};
use eframe::{epaint::mutex::RwLock, egui::Context};

use crate::window::{PageAction, PageLoadState};



pub struct CanLoggerPage {
    device_mode: Arc<RwLock<Option<TcuDeviceMode>>>,
    state: Arc<RwLock<PageLoadState>>,
    nag: Nag52Diag,
    
}

impl CanLoggerPage {
    pub fn new(nag: Nag52Diag, ctx: Context) -> Self {

        let dev_mode = Arc::new(RwLock::new(None));
        let dev_mode_c = dev_mode.clone();

        let state = Arc::new(RwLock::new(PageLoadState::Waiting("Setting extended diag mode".into())));
        let state_c = state.clone();

        let nag_c = nag.clone();


        std::thread::spawn(move|| {
            match nag_c.with_kwp(|k| k.kwp_set_session(KwpSessionType::ExtendedDiagnostics.into())) {
                Ok(_) => {
                    *state_c.write() = PageLoadState::Err(format!("Querying device mode"));
                    ctx.request_repaint();
                    if let Ok(mode) = nag_c.read_device_mode() {
                        *dev_mode_c.write() = Some(mode);
                        *state_c.write() = PageLoadState::Ok;
                    } else {
                        *state_c.write() = PageLoadState::Err(format!("Query of current session mode failed"));
                    }
                },
                Err(e) => {
                    *state_c.write() = PageLoadState::Err(format!("Set session mode failed: {e:?}"));
                }
            }
            ctx.request_repaint();
        });

        Self {
            device_mode: dev_mode,
            state: state,
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


impl crate::window::InterfacePage for CanLoggerPage {
    fn make_ui(&mut self, ui: &mut eframe::egui::Ui, frame: &eframe::Frame) -> crate::window::PageAction {
        ui.heading("CAN Logger viewer");
        let state = self.state.read().clone();

        match state {
            
            PageLoadState::Waiting(reason) => {
                ui.label(reason);
            },
            
            PageLoadState::Err(err) => {
                ui.label(format!("Page load failed: {err:}"));
            },

            PageLoadState::Ok => {
                let mode = self.device_mode.read().clone();
                ui.label(format!("Current device mode: {mode:?}"));
                let mut t_mode = None;
                if let Some(current_mode) = mode {
                    if current_mode.contains(TcuDeviceMode::CANLOGGER) {
                        // Already in CAN Logger mode
                        if ui.button("Disable CAN Logger mode").clicked() {
                            t_mode = Some(TcuDeviceMode::NORMAL);
                        }
                    } else {
                        // Not in logger mode
                        if ui.button("Enable CAN Logger mode").clicked() {
                            t_mode = Some(TcuDeviceMode::CANLOGGER);
                        }
                    }
                } else {
                    ui.label("Changing device modes");
                    ui.spinner();
                }
                if let Some(req_mode) = t_mode {
                    let nag_c = self.nag.clone();
                    let old_mode = self.device_mode.clone().read().clone();
                    *self.device_mode.write() = None;
                    let dest_mode_c = self.device_mode.clone();
                    std::thread::spawn(move|| {
                        match set_mode_and_reboot(nag_c, req_mode) {
                            Ok(new_mode) => {
                                *dest_mode_c.write() = Some(new_mode);
                            },
                            Err(e) => {
                                eprintln!("{:?}", e);
                                *dest_mode_c.write() = old_mode;
                            }
                        }
                    });
                }

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