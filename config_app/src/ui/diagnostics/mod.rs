use crate::window::{PageAction, StatusBar, get_context};
use backend::diag::Nag52Diag;
use backend::ecu_diagnostics::kwp2000::{KwpSessionTypeByte, KwpSessionType};
use eframe::egui::plot::{Legend, Line, Plot};
use eframe::egui::{Color32, RichText, Ui, Context};
use eframe::epaint::Stroke;
use std::borrow::Borrow;
use std::collections::hash_map::DefaultHasher;
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, RwLock};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::{Instant, Duration};

pub mod data;
pub mod rli;
pub mod solenoids;
use crate::ui::diagnostics::rli::{LocalRecordData, RecordIdents};

use self::rli::{ChartData, RLI_QUERY_INTERVAL, RLI_PLOT_INTERVAL};

const RLI_CHART_DISPLAY_TIME: u128 = 10000;

pub enum CommandStatus {
    Ok(String),
    Err(String),
}

pub struct DiagnosticsPage {
    query_ecu: Arc<AtomicBool>,
    curr_values: Arc<RwLock<Option<LocalRecordData>>>,
    prev_values: Arc<RwLock<Option<LocalRecordData>>>,
    record_to_query: Arc<RwLock<Option<RecordIdents>>>,
    charting_data: Arc<RwLock<VecDeque<(u128, ChartData)>>>,
    chart_idx: u128,
    read_error: Arc<RwLock<Option<String>>>,
    rli_start_time: Arc<AtomicU64>,
    launch_time: Instant
}

impl DiagnosticsPage {
    pub fn new(nag: Nag52Diag) -> Self {
        
        let run = Arc::new(AtomicBool::new(true));
        let run_t = run.clone();
        let run_tt = run.clone();

        let store = Arc::new(RwLock::new(None));
        let store_t = store.clone();
        let store_tt = store.clone();

        let store_old = Arc::new(RwLock::new(None));
        let store_old_t = store_old.clone();
        let store_old_tt = store_old.clone();

        let to_query: Arc<RwLock<Option<RecordIdents>>> = Arc::new(RwLock::new(None));
        let to_query_t = to_query.clone();
        let last_update = Arc::new(AtomicU64::new(0));
        let last_update_t = last_update.clone();
        let last_update_tt = last_update.clone();

        let launch_time = Instant::now();
        let launch_time_t = launch_time.clone();
        let launch_time_tt = launch_time.clone();

        let rli_start_time = Arc::new(AtomicU64::new(0));
        let rli_start_time_t = rli_start_time.clone();

        let charting_data = Arc::new(RwLock::new(VecDeque::new()));
        let charting_data_t = charting_data.clone();

        let err_text = Arc::new(RwLock::new(None));
        let err_text_t = err_text.clone();

        let _ = thread::spawn(move || {
            nag.with_kwp(|server| {
                server.kwp_set_session(KwpSessionTypeByte::Standard(KwpSessionType::Normal))
            });
            while run_t.load(Ordering::Relaxed) {
                let start = Instant::now();
                if let Some(to_query) = to_query_t.read().unwrap().clone() {
                    match nag.with_kwp(|server| to_query.query_ecu(server)) {
                        Ok(r) => {
                            *store_old_t.write().unwrap() = store_t.read().unwrap().clone();
                            *store_t.write().unwrap() = Some(r);
                            last_update_t.store(
                                launch_time_t.elapsed().as_millis() as u64,
                                Ordering::Relaxed,
                            );
                        },
                        Err(e) => {
                            *err_text_t.write().unwrap() = Some(e.to_string());
                            eprintln!("Could not query {}", e);
                        }
                    }
                }
                let taken = start.elapsed().as_millis() as u64;
                if taken < RLI_QUERY_INTERVAL {
                    std::thread::sleep(Duration::from_millis(RLI_QUERY_INTERVAL - taken));
                }
            }
        });

        let _ = thread::spawn(move || {
            while run_tt.load(Ordering::Relaxed) {
                let start = Instant::now();
                let ltime = launch_time_tt.elapsed().as_millis() as u128;

                let old = store_old_tt.try_read().unwrap().clone();
                let new = store_tt.try_read().unwrap().clone();

                if let (Some(o), Some(n)) = (old, new) {
                    let ms_since_update = std::cmp::min(
                        RLI_QUERY_INTERVAL,
                        ltime as u64 - last_update_tt.load(Ordering::Relaxed),
                    );

                    let start_time = rli_start_time_t.load(Ordering::Relaxed) as u128;
                    let co = o.get_chart_data()[0].clone();
                    let mut cn = n.get_chart_data()[0].clone();
                    if co.group_name == cn.group_name {
                        let mut proportion_curr: f32 = (ms_since_update as f32) / RLI_QUERY_INTERVAL as f32; // Percentage of old value to use
                        let mut proportion_prev: f32 = 1.0 - proportion_curr; // Percentage of curr value to use
                        if ms_since_update == 0 {
                            proportion_prev = 1.0;
                            proportion_curr = 0.0;
                        } else if ms_since_update == RLI_QUERY_INTERVAL {
                            proportion_prev = 0.0;
                            proportion_curr = 1.0;
                        }
                        for idx in 0..co.data.len() {
                            let interpolated = (co.data[idx].1 * proportion_prev) + (cn.data[idx].1 * proportion_curr);
                            cn.data[idx].1 = interpolated;
                        }

                        let ts = ltime - start_time;
                        let mut lck = charting_data_t.write().unwrap();
                        lck.push_back((ts, cn));
                        if lck[0].0 < ts.saturating_sub(RLI_CHART_DISPLAY_TIME) {
                            lck.pop_front();
                        }
                        drop(lck);
                    }
                }
                get_context().request_repaint();
                let taken = start.elapsed().as_millis() as u64;
                if taken < RLI_PLOT_INTERVAL {
                    std::thread::sleep(Duration::from_millis(RLI_PLOT_INTERVAL - taken));
                }
            }
        });
        
        Self {
            query_ecu: run,
            prev_values: store_old,
            curr_values: store,
            record_to_query: to_query,
            charting_data,
            chart_idx: 0,
            read_error: err_text,
            rli_start_time,
            launch_time
        }
    }
}

impl crate::window::InterfacePage for DiagnosticsPage {
    fn make_ui(&mut self, ui: &mut Ui, _frame: &eframe::Frame) -> PageAction {
        ui.heading("This is experimental, use with MOST up-to-date firmware");
        let mut rli_reset = false;
        if ui.button("Query gearbox sensor").clicked() {
            *self.record_to_query.write().unwrap() = Some(RecordIdents::GearboxSensors);
            rli_reset = true;
        }
        if ui.button("Query gearbox solenoids").clicked() {
            *self.record_to_query.write().unwrap() = Some(RecordIdents::SolenoidStatus);
            rli_reset = true;
        }
        if ui.button("Query solenoid pressures").clicked() {
            *self.record_to_query.write().unwrap() = Some(RecordIdents::PressureStatus);
            rli_reset = true;
        }
        if ui.button("Query can Rx data").clicked() {
            *self.record_to_query.write().unwrap() = Some(RecordIdents::CanDataDump);
            rli_reset = true;
        }
        if ui.button("Query Shift data").clicked() {
            *self.record_to_query.write().unwrap() = Some(RecordIdents::SSData);
            rli_reset = true;
        }
        if ui.button("Query Performance metrics").clicked() {
            *self.record_to_query.write().unwrap() = Some(RecordIdents::SysUsage);
            rli_reset = true;
        }
        if ui.button("Query Clutch speeds").clicked() {
            *self.record_to_query.write().unwrap() = Some(RecordIdents::ClutchSpeeds);
            rli_reset = true;
        }
        if ui.button("Query shift clutch velocities").clicked() {
            *self.record_to_query.write().unwrap() = Some(RecordIdents::ClutchVelocities);
            rli_reset = true;
        }

        if rli_reset {
            self.chart_idx = 0;
            self.charting_data.write().unwrap().clear();
            *self.curr_values.write().unwrap() = None;
            *self.prev_values.write().unwrap() = None;
            self.rli_start_time.store(self.launch_time.elapsed().as_millis() as u64, Ordering::Relaxed);
        }

        if let Some(e) = self.read_error.read().unwrap().clone() {
            ui.label(RichText::new(format!("Error querying ECU: {e}")).color(Color32::RED));
        }

        let current_val = self.curr_values.try_read().unwrap().clone();
        let chart_data = self.charting_data.read().unwrap().clone();
        if let Some(data) = current_val {
            let d = data.get_chart_data()[0].clone();

            // Can guarantee everything in `self.charting_data` will have the SAME length
            // as `d`
            let mut lines = Vec::new();
            let legend = Legend::default().position(eframe::egui::plot::Corner::LeftTop);
            for (idx, (key, _, _)) in d.data.iter().enumerate() {
                let mut points: Vec<[f64; 2]> = Vec::new();
                for (timestamp, point) in chart_data.iter() {
                    points.push([*timestamp as f64, point.data[idx].1 as f64])
                }
                let mut key_hasher = DefaultHasher::default();
                key.hash(&mut key_hasher);
                let r = key_hasher.finish();
                lines.push(Line::new(points).name(key.clone()).stroke(Stroke::new(2.0, 
                    Color32::from_rgb(
                        (r & 0xFF) as u8,
                        ((r >> 8) & 0xFF) as u8,
                        ((r >> 16) & 0xFF) as u8,
                    ))))
            }

            let mut plot = Plot::new(d.group_name.clone())
                .allow_drag(false)
                .legend(legend);
            if let Some((min, max)) = &d.bounds {
                plot = plot.include_y(*min);
                if *max > 0.1 {
                    // 0.0 check
                    plot = plot.include_y(*max);
                }
            }
            let h = ui.available_height();
            ui.horizontal(|row| {
                row.collapsing("Show table", |c| {
                    data.to_table(c);
                });
                plot.height(h).show(row, |plot_ui| {
                    for x in lines {
                        plot_ui.line(x)
                    }
                });
            });
        }

        PageAction::None
    }

    fn get_title(&self) -> &'static str {
        "Ultimate-NAG52 diagnostics"
    }

    fn should_show_statusbar(&self) -> bool {
        true
    }
}

impl Drop for DiagnosticsPage {
    fn drop(&mut self) {
        self.query_ecu.store(false, Ordering::Relaxed);
    }
}
