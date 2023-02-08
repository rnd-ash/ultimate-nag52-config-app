use std::{sync::{Arc, RwLock}, time::Instant};

use backend::{diag::Nag52Diag, hw::firmware::{Firmware, load_binary}};
use eframe::egui;
use nfd::Response;

use crate::window::InterfacePage;

use super::status_bar::MainStatusBar;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum CurrentFlashState {
    None,
    Prepare,
    Read { start_addr: u32, current: u32, total: u32 },
    Write { start_addr: u32, current: u32, total: u32 },
    Verify,
    Completed(String),
    Failed(String)
}

impl CurrentFlashState {
    pub fn is_idle(&self) -> bool {
        match self {
            CurrentFlashState::None | CurrentFlashState::Completed(_) => true,
            _ => false
        }
    }

    pub fn is_tx_rx(&self) -> bool {
        match self {
            CurrentFlashState::Read { start_addr, current, total } | CurrentFlashState::Write { start_addr, current, total } => true,
            _ => false
        }
    }

    pub fn get_progress(&self) -> (u32, u32, u32) {
        match self {
            CurrentFlashState::Read { start_addr, current, total } => (*start_addr, *current, *total),
            CurrentFlashState::Write { start_addr, current, total } => (*start_addr, *current, *total),
            _ => (0,0,0)
        }
    }
}


pub struct UpdatePage {
    nag: Nag52Diag,
    s_bar: MainStatusBar,
    fw: Option<Firmware>,
    status: Arc<RwLock<CurrentFlashState>>,
    flash_start: Option<Instant>,
}

impl UpdatePage {
    pub fn new(nag: Nag52Diag, bar: MainStatusBar) -> Self {
        Self{
            nag, 
            s_bar: bar,
            fw: None,
            status: Arc::new(RwLock::new(CurrentFlashState::None)),
            flash_start: None,
        }
    }
}

impl InterfacePage for UpdatePage {
    fn make_ui(&mut self, ui: &mut eframe::egui::Ui, frame: &eframe::Frame) -> crate::window::PageAction {
        ui.heading("Updater and dumper (New)");
        let state = self.status.read().unwrap().clone();
        if ui.button("Load FW").clicked() {
            match nfd::open_file_dialog(Some("bin"), None) {
                Ok(f) => {
                    if let Response::Okay(path) = f {
                        match load_binary(path) {
                            Ok(f) => self.fw = Some(f),
                            Err(e) => {
                                eprintln!("E loading binary! {:?}", e)
                            }
                        }
                    }
                }
                Err(_) => {}
            }
        }
        if let Some(fw) = &self.fw {
            egui::Grid::new("DGS").striped(true).show(ui, |ui| {
                ui.label("FW Name");
                ui.label(fw.header.get_fw_name());
                ui.end_row();

                ui.label("FW Version");
                ui.label(fw.header.get_version());
                ui.end_row();

                ui.label("ESP IDF Version");
                ui.label(fw.header.get_idf_version());
                ui.end_row();

                ui.label("Build date");
                ui.label(fw.header.get_date());
                ui.end_row();

                ui.label("Build time");
                ui.label(fw.header.get_time());
                ui.end_row();


            });

            if ui.button("Flash").clicked() {
                let mut ng = self.nag.clone();
                let fw_c = self.fw.clone().unwrap();
                let state_c = self.status.clone();
                std::thread::spawn(move || {
                    *state_c.write().unwrap() = CurrentFlashState::Prepare;
                    let (mut start_addr, bs) = match ng.begin_ota(fw_c.raw.len() as u32) {
                        Ok((a, b)) => (a, b),
                        Err(e) => {
                            *state_c.write().unwrap() = CurrentFlashState::Failed(format!("Failed to prepare for update. {}", e));
                            return;
                        },
                    };
                    let mut written = 0;
                    for (bid, block) in fw_c.raw.chunks(bs as usize).enumerate() {
                        match ng.transfer_data(((bid + 1) & 0xFF) as u8, block) {
                            Ok(_) => { 
                                written += block.len() as u32;
                                *state_c.write().unwrap() = CurrentFlashState::Write { start_addr, current: written, total: fw_c.raw.len() as u32 } 
                            },
                            Err(e) => {
                                *state_c.write().unwrap() = CurrentFlashState::Failed(format!("Failed to write to address 0x{:08X?} for update. {}", start_addr + written ,e));
                                return;
                            }
                        }
                    }
                    *state_c.write().unwrap() = CurrentFlashState::Verify;
                    match ng.end_ota() {
                        Ok(_) => *state_c.write().unwrap() = CurrentFlashState::Completed("Done!".to_string()),
                        Err(e) => {
                            *state_c.write().unwrap() = CurrentFlashState::Failed(format!("Error verification: {}", e));
                        }
                    }
                });
            }
        }
        if state.is_idle() {
            self.flash_start = None;
        }
        if !state.is_idle() {
            let (progress_percent, text) = match state.clone() {
                CurrentFlashState::Read { start_addr, current, total } => {
                    (current as f32 / total as f32, format!("Reading address 0x{:02X?}", start_addr + current))
                },
                CurrentFlashState::Write { start_addr, current, total } => {
                    (current as f32 / total as f32, format!("Writing address 0x{:02X?}", start_addr + current))
                },
                CurrentFlashState::Prepare => {
                    (0.0, "Preparing".to_string())
                },
                CurrentFlashState::None => (1.0, "Idle".to_string()),
                CurrentFlashState::Verify => (1.0, "Verifying".to_string()),
                CurrentFlashState::Completed(s) => (1.0, s),
                CurrentFlashState::Failed(s) => (1.0, s)
            };
            ui.add(egui::widgets::ProgressBar::new(progress_percent).animate(true).show_percentage());
            if state.is_tx_rx() {
                if self.flash_start.is_none() {
                    self.flash_start = Some(Instant::now())
                }
                let f_start = self.flash_start.unwrap();
                let (start_address, current, total) = state.get_progress();
                let spd = (1000.0 * current as f32
                    / f_start.elapsed().as_millis() as f32)
                    as u32;
                let eta = (total - current) / spd;
                ui.label(format!("Avg {:.0} bytes/sec", spd));
                ui.label(format!("ETA: {:02}:{:02} seconds remaining", eta/60, eta % 60));
            }
        }
        crate::window::PageAction::None
    }

    fn get_title(&self) -> &'static str {
        "Flash updater"
    }

    fn get_status_bar(&self) -> Option<Box<dyn crate::window::StatusBar>> {
        Some(Box::new(self.s_bar.clone()))
    }
}