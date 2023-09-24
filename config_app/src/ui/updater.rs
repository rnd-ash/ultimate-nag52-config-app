use std::{sync::{Arc, RwLock}, time::Instant, path::PathBuf, fs::File, io::{Write, BufReader, Cursor, Read}};

use backend::{diag::{Nag52Diag, flash::PartitionInfo, DataState, settings::ModuleSettingsData, module_settings_flash_store::ModuleSettingsFlashHeader}, hw::firmware::{Firmware, load_binary, FirmwareHeader, load_binary_from_path}};
use curl::easy::{Easy, List};
use eframe::egui::{self, ScrollArea};
use octocrab::{models::repos::Release, repos::releases::ListReleasesBuilder};
use tokio::runtime::Runtime;

use crate::window::{InterfacePage, PageAction, get_context};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum CurrentFlashState {
    None,
    Download(usize, usize),
    DownloadYml(usize, usize),
    Unzip,
    Prepare,
    Read { start_addr: u32, current: u32, total: u32 },
    Write { ty: &'static str, start_addr: u32, current: u32, total: u32 },
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
            CurrentFlashState::Read { start_addr, current, total } | CurrentFlashState::Write { ty:_, start_addr, current, total } => true,
            _ => false
        }
    }

    pub fn get_progress(&self) -> (u32, u32, u32) {
        match self {
            CurrentFlashState::Read { start_addr, current, total } => (*start_addr, *current, *total),
            CurrentFlashState::Write { ty:_, start_addr, current, total } => (*start_addr, *current, *total),
            _ => (0,0,0)
        }
    }
}


pub struct UpdatePage {
    nag: Nag52Diag,
    fw: Arc<RwLock<Option<(Firmware, Vec<u8>)>>>,
    status: Arc<RwLock<CurrentFlashState>>,
    flash_start: Option<Instant>,
    coredump: Option<PartitionInfo>,
    old_fw: Option<(FirmwareHeader, PartitionInfo)>,
    releases:  Arc<RwLock<DataState<Vec<Release>>>>,
    checked_unstable: bool,
    selected_release: Option<Release>
}

impl UpdatePage {
    pub fn new(mut nag: Nag52Diag) -> Self {
        let coredump_info = nag.get_coredump_flash_info().ok();
        let curr_fw_info = nag.get_running_fw_info().ok().zip(nag.get_running_partition_flash_info().ok());

        let fw_list = Arc::new(RwLock::new(DataState::Unint));
        let fw_list_c = fw_list.clone();

        std::thread::spawn(move|| {
            let rt = Runtime::new().unwrap();
            match rt.block_on(async {
                octocrab::instance().repos("rnd-ash", "ultimate-nag52-fw")
                    .releases()
                    .list()
                    .send()
                    .await
            }) {
                Ok(l) => {
                    let mut r_list = vec![];
                    for release in l {
                        r_list.push(release);
                    }
                    *fw_list_c.write().unwrap() = DataState::LoadOk(r_list);
                },
                Err(e) => {
                    *fw_list_c.write().unwrap() = DataState::LoadErr(format!("{:?}", e));
                }
            }
        });

        Self{
            nag, 
            fw: Arc::new(RwLock::new(None)),
            status: Arc::new(RwLock::new(CurrentFlashState::None)),
            flash_start: None,
            coredump: coredump_info,
            old_fw: curr_fw_info,
            releases: fw_list,
            checked_unstable: false,
            selected_release: None
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

        ui.label("Build time");
        ui.label(fw.get_build_timestamp().map(|f| f.to_string()).unwrap_or("Unknown".into()));
        ui.end_row();
    });
}

impl InterfacePage for UpdatePage {
    fn make_ui(&mut self, ui: &mut eframe::egui::Ui, frame: &eframe::Frame) -> crate::window::PageAction {
        ui.heading("Updater and dumper (New)");
        let state = self.status.read().unwrap().clone();
        let mut read_partition: Option<PartitionInfo> = None;
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
                    read_partition = Some(coredump.clone())
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
                read_partition = Some(part_info.clone())
            }
        }
        if ui.button("Backup entire flash (WARNING. SLOW)").clicked() {
            read_partition = Some(PartitionInfo { address: 0, size: 0x400000  })
        }
        ui.separator();
        ui.heading("Update to new Firmware");


        let r = self.releases.read().unwrap().clone();
        match r {
            DataState::LoadOk(release_list) => {
                fn release_to_string(r: &Release) -> String {
                    let date = r.clone().created_at.map(|x| x.to_string()).unwrap_or("Unknown date".into());
                    let rel = r.clone().name.unwrap_or(r.clone().tag_name);
                    format!("{} at {}", rel, date)
                }

                ui.checkbox(&mut self.checked_unstable, "Show unstable releases");
                egui::ComboBox::from_label("Select release")
                    .width(500.0)
                    .selected_text(&self.selected_release.clone().map(|x| release_to_string(&x)).unwrap_or("None".into()))
                    .show_ui(ui, |cb_ui| {
                        for r in release_list {
                            if !self.checked_unstable && (r.prerelease || !r.tag_name.starts_with("main")) {
                                continue;
                            }


                            cb_ui.selectable_value(
                                &mut self.selected_release,
                                Some(r.clone()),
                                release_to_string(&r)
                            );
                        }
                    }
                );
                if let Some(rel) = &self.selected_release {
                    ui.hyperlink_to("Show on GitHub", format!("https://github.com{}", rel.html_url.path()));
                    let fw_url = rel.assets.iter().find(|x| x.name.ends_with(".bin")).cloned();
                    let yml_url = rel.assets.iter().find(|x| x.name.ends_with(".yml")).cloned();
                    let elf_url = rel.assets.iter().find(|x| x.name.ends_with(".elf")).cloned();
                    

                    if let (Some(fw), Some(yml)) = (fw_url, yml_url) {
                        if ui.button("Download firmware").clicked() {
                            let state_c = self.status.clone();
                            let fw_c = self.fw.clone();
                            std::thread::spawn(move|| {
                                let mut url = format!("https://api.github.com{}",fw.url.path());
                                *state_c.write().unwrap() = CurrentFlashState::Download(0, 0);
                                let mut buffer_firmware: Vec<u8> = Vec::new();
                                let mut buffer_yml: Vec<u8> = Vec::new();
                                let mut easy = Easy::new();
                                let mut list = List::new();
                                list.append("Accept: application/octet-stream").unwrap();
                                easy.progress(true);
                                let state_progress = state_c.clone();
                                easy.progress_function(move|dltotal,dlnow,_,_| {
                                    *state_progress.write().unwrap() = CurrentFlashState::Download(dlnow as usize, dltotal as usize);
                                    return true;
                                });
                                easy.http_headers(list).unwrap();
                                easy.useragent("request").unwrap();
                                easy.follow_location(true).unwrap();
                                easy.url(&url).unwrap();
                                {
                                    let mut transfer = easy.transfer();
                                    let _ = transfer.write_function(|data| {
                                        buffer_firmware.extend_from_slice(data);                         
                                        Ok(data.len())
                                    });
                                    let _ = transfer.perform();
                                }
                                
                                let code = easy.response_code().unwrap_or(0);
                                if code == 200 || code == 302 {
                                    *state_c.write().unwrap() = CurrentFlashState::DownloadYml(0,0);
                                    url = format!("https://api.github.com{}",yml.url.path());
                                    // Now try YML download
                                    let mut easy = Easy::new();
                                    let mut list = List::new();
                                    list.append("Accept: application/octet-stream").unwrap();
                                    easy.http_headers(list).unwrap();
                                    easy.useragent("request").unwrap();
                                    easy.follow_location(true).unwrap();
                                    easy.progress(true);

                                    let state_progress = state_c.clone();

                                    easy.progress_function(move|dltotal,dlnow,_,_| {
                                        *state_progress.write().unwrap() = CurrentFlashState::DownloadYml(dlnow as usize, dltotal as usize);
                                        return true;
                                    });
                                    easy.url(&url).unwrap();
                                    {
                                        let mut transfer = easy.transfer();
                                        let _ = transfer.write_function(|data| {
                                            buffer_yml.extend_from_slice(data);
                                            Ok(data.len())
                                        });
                                        let _ = transfer.perform();
                                    }
                                    // Encode
                                    let (header, compressed) = ModuleSettingsFlashHeader::new_from_yml_content(&buffer_yml);
                                    let tx_yml = header.merge_to_tx_data(&compressed);

                                    let code = easy.response_code().unwrap_or(0);
                                    if code == 200 || code == 302 {
                                        // Try and load FW from here
                                        match load_binary(buffer_firmware) {
                                            Ok(bin) => {
                                                *fw_c.write().unwrap() = Some((bin, tx_yml));
                                                *state_c.write().unwrap() = CurrentFlashState::None;
                                            }
                                            Err(e) => {
                                                *state_c.write().unwrap() = CurrentFlashState::Failed(format!("Firmware was invalid: {:?}", e));
                                            }
                                        }
                                    } else {
                                        *state_c.write().unwrap() = CurrentFlashState::Failed(format!("Firmware download YML response code was {code}"));
                                    }
                                } else {
                                    *state_c.write().unwrap() = CurrentFlashState::Failed(format!("Firmware download firmware response code was {code}"));
                                }
                            });
                        }
                    }

                    if let Some(elf) = elf_url {
                        if ui.button("Download debug elf file").clicked() {
                            return PageAction::SendNotification { text: format!("Todo. Debugger UI!"), kind: egui_toast::ToastKind::Info }
                        }
                    }
                }
            },
            DataState::Unint => {
                ui.horizontal(|row| {
                    row.spinner();
                    row.label("Querying release list");
                });
            },
            DataState::LoadErr(e) => {
                ui.label(format!("Could not query release list: {:?}", e));
            },
        }
        //if let Some(sel_rel) = &self.selected_release {
        //    ui.hyperlink_to(label, url)
        //}

        if ui.button("Load FW").clicked() {
            if let Some(bin_path) = rfd::FileDialog::new()
                .add_filter("Firmware bin", &["bin"])
                .pick_file() {
                match load_binary_from_path(bin_path.into_os_string().into_string().unwrap()) {
                    Ok(fw) => {
                        if let Some(yml_path) = rfd::FileDialog::new()
                            .add_filter("MODULE_SETTINGS.yml", &["yml"])
                            .pick_file() {
                                let mut f = File::open(yml_path).unwrap();
                                let mut b = Vec::new();
                                f.read_to_end(&mut b);
                                let mf = ModuleSettingsFlashHeader::new_from_yml_content(&b);
                                let tx_yml = mf.0.merge_to_tx_data(&mf.1);
                                *self.fw.write().unwrap() = Some((fw, tx_yml))
                        }
                    },
                    Err(e) => {
                        eprintln!("E loading binary! {:?}", e)
                    }
                }
            }
        }
        let c_fw = self.fw.clone().read().unwrap().clone();
        if let Some((fw, yml)) = &c_fw {
            make_fw_info(ui, "nfw",&fw.header, None);
            let mut flash = false;
            let mut disclaimer = false;
            if let Some(new_ts) = fw.header.get_build_timestamp() {
                if let Some(old_ts) = self.old_fw.map(|f| f.0.get_build_timestamp()).flatten() {
                    if old_ts > new_ts {
                        ui.strong("WARNING. The new firmware is older than the current firmware! This can cause bootloops!");
                        ui.hyperlink_to("See reverting to old FW versions", "docs.ultiamte-nag52.net");
                        disclaimer = true;
                    }
                }
            }
            if (!fw.header.get_version().contains("main") || fw.header.get_version().contains("dirty")) && self.old_fw.map(|x| x.0.get_version().contains("main")).unwrap_or(true) {
                ui.strong("WARNING. You are about to flash potentially unstable firmware. Proceed with caution!");
                disclaimer = true;
            }
            let text = match disclaimer {
                true => "I have read the warnings. Proceed with flashing",
                false => "Flash new FW",
            };
            if ui.button(text).clicked() {
                flash = true;
            }
            if flash {
                let mut ng = self.nag.clone();
                let (fw_c, yml_c) = c_fw.clone().unwrap();
                let state_c = self.status.clone();
                std::thread::spawn(move || {
                    get_context().request_repaint();
                    *state_c.write().unwrap() = CurrentFlashState::Prepare;
                    let (start_addr, bs) = match ng.begin_ota(fw_c.raw.len() as u32) {
                        Ok((a, b)) => (a, b),
                        Err(e) => {
                            *state_c.write().unwrap() = CurrentFlashState::Failed(format!("Failed to prepare for firmware update. {}", e));
                            return;
                        },
                    };
                    get_context().request_repaint();
                    let mut written = 0;
                    for (bid, block) in fw_c.raw.chunks(bs as usize).enumerate() {
                        match ng.transfer_data(((bid + 1) & 0xFF) as u8, block) {
                            Ok(_) => { 
                                written += block.len() as u32;
                                *state_c.write().unwrap() = CurrentFlashState::Write { ty: "Firmware", start_addr, current: written, total: fw_c.raw.len() as u32 } 
                            },
                            Err(e) => {
                                *state_c.write().unwrap() = CurrentFlashState::Failed(format!("Failed to write to address 0x{:08X?} (Firmware) for update. {}", start_addr + written ,e));
                                return;
                            }
                        }
                        get_context().request_repaint();
                    }

                    // Write block 2 (YML)
                    *state_c.write().unwrap() = CurrentFlashState::Prepare;
                    let (start_addr, bs) = match ng.begin_yml_ota(yml_c.len() as u32) {
                        Ok((a, b)) => (a, b),
                        Err(e) => {
                            *state_c.write().unwrap() = CurrentFlashState::Failed(format!("Failed to prepare SCN coding definition update. {}", e));
                            return;
                        },
                    };

                    get_context().request_repaint();
                    let mut written = 0;
                    for (bid, block) in yml_c.chunks(bs as usize).enumerate() {
                        match ng.transfer_data(((bid + 1) & 0xFF) as u8, block) {
                            Ok(_) => { 
                                written += block.len() as u32;
                                *state_c.write().unwrap() = CurrentFlashState::Write { ty: "SCN Coding definition", start_addr, current: written, total: fw_c.raw.len() as u32 } 
                            },
                            Err(e) => {
                                *state_c.write().unwrap() = CurrentFlashState::Failed(format!("Failed to write to address 0x{:08X?} (SCN Coding definition) for update. {}", start_addr + written ,e));
                                return;
                            }
                        }
                        get_context().request_repaint();
                    }

                    *state_c.write().unwrap() = CurrentFlashState::Verify;
                    match ng.end_ota(true) {
                        Ok(_) => *state_c.write().unwrap() = CurrentFlashState::Completed("Done!".to_string()),
                        Err(e) => {
                            *state_c.write().unwrap() = CurrentFlashState::Failed(format!("Error verification: {}", e));
                        }
                    }
                    get_context().request_repaint();
                });
            }
        }
        // Check for read operation
        if let Some(read_op) = &read_partition {
            let mut ng = self.nag.clone();
            let state_c = self.status.clone();
            let mut save_path = None;
            if let Some(f) = rfd::FileDialog::new()
                .add_filter(".bin", &["bin"])
                .save_file() {
                    save_path = Some(f);
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
                    get_context().request_repaint();
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
                CurrentFlashState::Write { ty, start_addr, current, total } => {
                    (current as f32 / total as f32, format!("Writing block '{ty}' at address 0x{:02X?}", start_addr + current))
                },
                CurrentFlashState::Prepare => {
                    (0.0, "Preparing".to_string())
                },
                CurrentFlashState::None => (1.0, "Idle".to_string()),
                CurrentFlashState::Verify => (1.0, "Verifying".to_string()),
                CurrentFlashState::Completed(s) => (1.0, s),
                CurrentFlashState::Failed(s) => (1.0, s),
                CurrentFlashState::Download(now, total) => {
                    let mut f = 0.0;
                    if total != 0 {
                        f = (now as f32 * 100.0) / total as f32;
                    }
                    (f, format!("1/2: Downloading firmware. {now} bytes done"))
                },
                CurrentFlashState::DownloadYml(now, total) => {
                    let mut f = 0.0;
                    if total != 0 {
                        f = (now as f32 * 100.0) / total as f32;
                    }
                    (f, format!("2/2: Downloading SCN coding information. {now} bytes done"))
                },
                CurrentFlashState::Unzip => {
                    (0.0, "Unzipping firmware".to_string())
                },
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