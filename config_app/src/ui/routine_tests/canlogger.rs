use std::{sync::{Arc, atomic::{AtomicBool, Ordering}}, time::Duration, collections::VecDeque, fs::File, io::Write};

use backend::{diag::{Nag52Diag, device_modes::TcuDeviceMode}, ecu_diagnostics::{kwp2000::{KwpSessionType, ResetType}, DiagServerResult, channel::{CanFrame, Packet}}};
use eframe::{epaint::mutex::RwLock, egui::{Context, ScrollArea}};

use crate::window::{PageAction, PageLoadState};



pub struct CanLoggerPage {
    device_mode: Arc<RwLock<Option<TcuDeviceMode>>>,
    state: Arc<RwLock<PageLoadState>>,
    nag: Nag52Diag,
    reader_running: Arc<AtomicBool>,
    dialog_open: Arc<AtomicBool>,
    frames: Arc<RwLock<VecDeque<CanFrame>>>
}

impl CanLoggerPage {
    pub fn new(nag: Nag52Diag, ctx: Context) -> Self {

        let dev_mode = Arc::new(RwLock::new(None));
        let dev_mode_c = dev_mode.clone();

        let state = Arc::new(RwLock::new(PageLoadState::Waiting("Setting extended diag mode".into())));
        let state_c = state.clone();

        let nag_c = nag.clone();

        let running = Arc::new(AtomicBool::new(true));
        let running_c = running.clone();

        let dialog_open = Arc::new(AtomicBool::new(false));
        let dialog_open_c = dialog_open.clone();

        let running = Arc::new(AtomicBool::new(true));
        let running_c = running.clone();

        let frames = Arc::new(RwLock::new(VecDeque::new()));
        let frames_c = frames.clone();

        std::thread::spawn(move|| {
            match nag_c.with_kwp(|k| k.kwp_set_session(KwpSessionType::ExtendedDiagnostics.into())) {
                Ok(_) => {
                    *state_c.write() = PageLoadState::Err(format!("Querying device mode"));
                    ctx.request_repaint();
                    if let Ok(mode) = nag_c.read_device_mode() {
                        *dev_mode_c.write() = Some(mode);
                        *state_c.write() = PageLoadState::Ok;
                        // Now loop querying ECU
                        while running_c.load(Ordering::Relaxed) {
                            if dev_mode_c.read().clone().unwrap_or(TcuDeviceMode::empty()).contains(TcuDeviceMode::CANLOGGER) {
                                let mut activity = false;
                                while let Some(cf) = nag_c.read_can_msg() {
                                    activity = true;
                                    frames_c.write().push_back(cf);
                                }
                                if activity && !dialog_open_c.load(Ordering::Relaxed) {
                                    ctx.request_repaint();
                                }
                            }
                            std::thread::sleep(Duration::from_millis(10));
                        }
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
            reader_running: Arc::new(AtomicBool::new(false)),
            frames,
            dialog_open: dialog_open
        }
        
    }
}

impl Drop for CanLoggerPage {
    fn drop(&mut self) {
        self.reader_running.store(false, Ordering::Relaxed);
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
                let frames_now = self.frames.read().clone();
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

                    if ui.button("Clear CAN").clicked() {
                        self.nag.clear_can_buffer();
                        self.frames.write().clear();
                    }

                    if ui.button("Save to file").clicked() {
                        self.dialog_open.store(true, Ordering::Relaxed);
                        if let Some(p) = rfd::FileDialog::new().add_filter("CAN Log", &["log"]).set_title("Save CAN Log").save_file() {
                            let mut f = File::create(p).unwrap();
                            for frame in frames_now.iter() {
                                let mut f_str = String::new();
                                for byte in frame.get_data() {
                                    f_str.push_str(&format!(" {:02X?}", byte));
                                }
                                f.write_all(format!("0x{:04X}{f_str}\n", frame.get_address()).as_bytes());
                            }
                        }
                        self.dialog_open.store(false, Ordering::Relaxed);
                    }

                    // Now render CAN view UI
                    ui.strong("Read CAN data:");
                    ui.label(format!("Read {} frames so far", frames_now.len()));
                    ScrollArea::new([false, true]).stick_to_bottom(true).show(ui, |ui| {
                        for frame in frames_now.iter() {
                            ui.label(format!("{frame:02X?}"));
                        }
                    });

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
                        nag_c.clear_can_buffer();
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