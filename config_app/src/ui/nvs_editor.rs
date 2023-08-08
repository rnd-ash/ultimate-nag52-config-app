use std::{sync::{Arc, RwLock}, time::Instant};

use backend::{diag::{Nag52Diag, flash::PartitionInfo, nvs::NvsPartition}, ecu_diagnostics::{kwp2000::KwpSessionType, DiagServerResult}};
use eframe::egui::{ProgressBar, widgets, ScrollArea};

use crate::window::{PageLoadState, InterfacePage, PageAction};

const PAGE_LOAD_TIMEOUT: f32 = 30000.0;
const NVS_PART_OFFSET: u32 = 0x9000;
const NVS_PART_LEN: u32 = 0x4000;
pub struct NvsEditor {
    ready: Arc<RwLock<PageLoadState>>,
    start_time: Instant,
    nag: Nag52Diag,
    nvs_part_data:  Arc<RwLock<Option<NvsPartition>>>
}


impl NvsEditor {
    pub fn new(nag: Nag52Diag) -> Self {
        let state = Arc::new(RwLock::new(PageLoadState::waiting("Entering diag mode")));
        let state_c = state.clone();
        let n_c = nag.clone();
        let nvs = Arc::new(RwLock::new(None));
        let nvs_t = nvs.clone();
        std::thread::spawn(move|| {
            fn download(n: Nag52Diag, s: Arc<RwLock<PageLoadState>>) -> DiagServerResult<Vec<u8>> {
                let part_info = PartitionInfo {
                    address: NVS_PART_OFFSET,
                    size: NVS_PART_LEN,
                };
                *s.write().unwrap() = PageLoadState::waiting("Beginning download");
                let bs = n.begin_download(&part_info)?;
                let mut res: Vec<u8> = vec![];
                let mut blk_id = 1u8;
                while res.len() < NVS_PART_LEN as usize {
                    let read = n.read_data(blk_id)?;
                    res.extend_from_slice(&read);
                    *s.write().unwrap() = PageLoadState::waiting(format!("Reading offset 0x{:08X}", (NVS_PART_OFFSET as usize) + res.len()));
                    blk_id = blk_id.wrapping_add(1);
                }
                let _ = n.end_ota(false);
                Ok(res)
            }

            match download(n_c, state_c.clone()) {
                Ok(res) => {
                    *state_c.write().unwrap() = PageLoadState::Ok;
                    *nvs_t.write().unwrap() = Some(NvsPartition::new(res.clone()));
                },
                Err(e) => {
                    *state_c.write().unwrap() = PageLoadState::Err(format!("{:?}", e));
                },
            }
        });


        Self {
            ready: state,
            start_time: Instant::now(),
            nag,
            nvs_part_data: nvs
        }
    }
}

impl InterfacePage for NvsEditor {
    fn make_ui(&mut self, ui: &mut eframe::egui::Ui, frame: &eframe::Frame) -> crate::window::PageAction {
        match self.ready.read().unwrap().clone() {
            PageLoadState::Ok => {
                ui.heading("NVS Editor");
            },
            PageLoadState::Waiting(reason) => {
                ui.heading("Please wait...");
                let prog = 
                ProgressBar::new(self.start_time.elapsed().as_millis() as f32 / PAGE_LOAD_TIMEOUT).animate(true);
                ui.add(prog);
                ui.label(format!("Current action: {}", reason));
                return PageAction::DisableBackBtn;
                
            },
            PageLoadState::Err(e) => {
                ui.heading("Page loading failed!");
                ui.label(format!("Error: {:?}", e));
                return PageAction::None;
            },
        }
        let part_data = self.nvs_part_data.read().unwrap().clone().unwrap();
        ScrollArea::new([false, true]).show(ui, |scroll|
            for page in &part_data.pages {
                scroll.collapsing(format!("Page {}", unsafe { page.seqnr }), |pg_ui| {
                    for (id, blk) in page.entries.iter().enumerate() {
                        //let bm = (blk.bitmap[i / 4] >> ((i % 4) * 2)) & 0x03;
                        pg_ui.collapsing(format!("Block {}", id), |blk_ui| {
                            blk_ui.code(format!("{:#02X?}", blk));
                        });
                    }
                });
            }
        );
        
        PageAction::None
    }

    fn get_title(&self) -> &'static str {
        "NVS Editor"
    }

    fn should_show_statusbar(&self) -> bool {
        true
    }
}