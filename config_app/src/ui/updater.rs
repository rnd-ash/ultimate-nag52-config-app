use std::{sync::{Arc, RwLock}, time::Instant, path::PathBuf, fs::File, io::Write};

use backend::{diag::{Nag52Diag, flash::PartitionInfo}, hw::firmware::{Firmware, load_binary, FirmwareHeader}};
use eframe::egui;
use nfd::Response;

use crate::window::{InterfacePage, PageAction};

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
    fw: Option<Firmware>,
    status: Arc<RwLock<CurrentFlashState>>,
    flash_start: Option<Instant>,
    coredump: Option<PartitionInfo>,
    old_fw: Option<(FirmwareHeader, PartitionInfo)>,
}

impl UpdatePage {
    pub fn new(mut nag: Nag52Diag) -> Self {
        let coredump_info = nag.get_coredump_flash_info().ok();
        let curr_fw_info = nag.get_running_fw_info().ok().zip(nag.get_running_partition_flash_info().ok());
        Self{
            nag, 
            fw: None,
            status: Arc::new(RwLock::new(CurrentFlashState::None)),
            flash_start: None,
            coredump: coredump_info,
            old_fw: curr_fw_info
        }
    }
}

fn make_fw_info(ui: &mut egui::Ui, id: &str, fw: &FirmwareHeader, part_info: Option<&PartitionInfo>) {
    egui::Grid::new(id).striped(true).show(ui, |ui| {
        if let Some(info) = part_info {
            ui.label("FW Partition address");
            ui.label(format!("0x{:08X?}", info.address));
            ui.end_row();
            ui.label("FW size");
            ui.label(format!("{:.1}Kb", (info.size as f32)/1024.0));
            ui.end_row();
        }
        
        ui.label("FW Name");
        ui.label(fw.get_fw_name());
        ui.end_row();

        ui.label("FW Version");
        ui.label(fw.get_version());
        ui.end_row();

        ui.label("ESP IDF Version");
        ui.label(fw.get_idf_version());
        ui.end_row();

        ui.label("Build date");
        ui.label(fw.get_date());
        ui.end_row();

        ui.label("Build time");
        ui.label(fw.get_time());
        ui.end_row();
    });
}

impl InterfacePage for UpdatePage {
    fn make_ui(&mut self, ui: &mut eframe::egui::Ui, frame: &eframe::Frame) -> crate::window::PageAction {
        ui.heading("Updater and dumper (New)");
        let state = self.status.read().unwrap().clone();
        let mut read_partition: Option<(PartitionInfo, String)> = None;
        ui.heading("Coredump info");
        if let Some(coredump) = &self.coredump {
            if coredump.size != 0 {
                egui::Grid::new("cdumpinfo").striped(true).show(ui, |ui| {
                    ui.label("Address");
                    ui.label(format!("0x{:08X?}", coredump.address));
                    ui.end_row();

                    ui.label("Size");
                    ui.label(format!("{:.1}Kb", (coredump.size as f32)/1024.0));
                    ui.end_row();
                });
                if ui.button("Read coredump").clicked() {
                    read_partition = Some((coredump.clone(), "un52_coredump.elf".to_string()))
                }
            } else {
                ui.label("No coredump stored on this TCU!");
            }
        } else {
            ui.label("No coredump found");
        }
        ui.separator();
        if let Some((info, part_info)) = &self.old_fw {
            ui.heading("Current Firmware");
            make_fw_info(ui, "cfw", &info, Some(part_info));
            if ui.button("Backup current firmware").clicked() {
                read_partition = Some((part_info.clone(), format!("un52_fw_{}_backup.bin", info.get_date())))
            }
        }
        if ui.button("Backup entire flash (WARNING. SLOW)").clicked() {
            read_partition = Some((PartitionInfo { address: 0, size: 0x400000  } , "un52_flash_backup.bin".to_string()))
        }
        ui.separator();
        ui.heading("Update to new Firmware");
        if ui.button("Load FW").clicked() {
            match nfd::open_file_dialog(Some("bin"), None) {
                Ok(Response::Okay(path)) => {
                    match load_binary(path) {
                        Ok(f) => self.fw = Some(f),
                        Err(e) => {
                            eprintln!("E loading binary! {:?}", e)
                        }
                    }
                }
                _ => {}
            }
        }
        if let Some(fw) = &self.fw {
            make_fw_info(ui, "nfw",&fw.header, None);
            if ui.button("Flash new FW").clicked() {
                let mut ng = self.nag.clone();
                let fw_c = self.fw.clone().unwrap();
                let state_c = self.status.clone();
                std::thread::spawn(move || {
                    *state_c.write().unwrap() = CurrentFlashState::Prepare;
                    let (start_addr, bs) = match ng.begin_ota(fw_c.raw.len() as u32) {
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
                    match ng.end_ota(true) {
                        Ok(_) => *state_c.write().unwrap() = CurrentFlashState::Completed("Done!".to_string()),
                        Err(e) => {
                            *state_c.write().unwrap() = CurrentFlashState::Failed(format!("Error verification: {}", e));
                        }
                    }
                });
            }
        }
        // Check for read operation
        if let Some((read_op, save_name)) = &read_partition {
            let mut ng = self.nag.clone();
            let state_c = self.status.clone();
            let mut save_path = None;
            if let Ok(nfd::Response::Okay(p)) = nfd::open_pick_folder(None) {
                save_path = Some(PathBuf::from(p).join(save_name));
            } else {
                *state_c.write().unwrap() = CurrentFlashState::Failed(format!("user did not specify save path"));
                return PageAction::None;
            }
            let read_op_c = read_op.clone();
            std::thread::spawn(move || {
                *state_c.write().unwrap() = CurrentFlashState::Prepare;
                let bs = match ng.begin_download(&read_op_c) {
                    Ok(bs) => bs,
                    Err(e) => {
                        *state_c.write().unwrap() = CurrentFlashState::Failed(format!("Failed to prepare for reading. {}", e));
                        return;
                    },
                };
                let mut read = 0;
                let mut read_buffer: Vec<u8> = vec![];
                let mut counter = 0u8;
                let start = read_op_c.address;
                while read_buffer.len() < read_op_c.size as usize {
                    counter = counter.wrapping_add(1);
                    match ng.read_data(counter) {
                        Ok(data) => { 
                            read += data.len();
                            read_buffer.extend_from_slice(&data);
                            *state_c.write().unwrap() = CurrentFlashState::Read { start_addr: start, current: read as u32, total: read_op_c.size };
                        },
                        Err(e) => {
                            *state_c.write().unwrap() = CurrentFlashState::Failed(format!("Failed to read address 0x{:08X?}. {}", start as usize + read ,e));
                            return;
                        }
                    }
                }
                match ng.end_ota(false) {
                    Ok(_) => *state_c.write().unwrap() = {
                        File::create(save_path.unwrap()).unwrap().write_all(&read_buffer).unwrap();
                        CurrentFlashState::Completed("Done!".to_string())
                    },
                    Err(e) => {
                        *state_c.write().unwrap() = CurrentFlashState::Failed(format!("Error verification: {}", e));
                    }
                }
            });
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
            ui.label(text);
        }
        crate::window::PageAction::None
    }

    fn get_title(&self) -> &'static str {
        "Flash updater"
    }

    fn should_show_statusbar(&self) -> bool {
        true
    }
}