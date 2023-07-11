use crate::window::{PageAction, StatusBar};
use backend::diag::Nag52Diag;
use backend::ecu_diagnostics::kwp2000::{KwpSessionTypeByte, KwpSessionType};
use eframe::egui::plot::{Legend, Line, Plot};
use eframe::egui::{Color32, RichText, Ui};
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

use self::rli::{ChartData, RLI_QUERY_INTERVAL};

pub enum CommandStatus {
    Ok(String),
    Err(String),
}

pub struct DiagnosticsPage {
    query_ecu: Arc<AtomicBool>,
    last_update_time: Arc<AtomicU64>,
    curr_values: Arc<RwLock<Option<LocalRecordData>>>,
    prev_values: Arc<RwLock<Option<LocalRecordData>>>,
    time_since_launch: Instant,
    record_to_query: Arc<RwLock<Option<RecordIdents>>>,
    charting_data: VecDeque<(u128, ChartData)>,
    chart_idx: u128,
    read_error: Arc<RwLock<Option<String>>>
}

impl DiagnosticsPage {
    pub fn new(nag: Nag52Diag) -> Self {
        
        let run = Arc::new(AtomicBool::new(true));
        let run_t = run.clone();

        let store = Arc::new(RwLock::new(None));
        let store_t = store.clone();

        let store_old = Arc::new(RwLock::new(None));
        let store_old_t = store_old.clone();

        let to_query: Arc<RwLock<Option<RecordIdents>>> = Arc::new(RwLock::new(None));
        let to_query_t = to_query.clone();
        let last_update = Arc::new(AtomicU64::new(0));
        let last_update_t = last_update.clone();

        let launch_time = Instant::now();
        let launch_time_t = launch_time.clone();

        let err_text = Arc::new(RwLock::new(None));
        let err_text_t = err_text.clone();

        let _ = thread::spawn(move || {
            nag.with_kwp(|server| {
                server.kwp_set_session(KwpSessionTypeByte::Standard(KwpSessionType::Normal))
            });
            while run_t.load(Ordering::Relaxed) {
                let start = Instant::now();
                if let Some(to_query) = *to_query_t.read().unwrap() {
                    match nag.with_kwp(|server| to_query.query_ecu(server)) {
                        Ok(r) => {
                            *err_text_t.write().unwrap() = None;
                            let curr = store_t.read().unwrap().clone();
                            *store_old_t.write().unwrap() = curr;
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
                if taken < rli::RLI_QUERY_INTERVAL {
                    std::thread::sleep(Duration::from_millis(rli::RLI_QUERY_INTERVAL - taken));
                }
            }
        });
        
        Self {
            query_ecu: run,
            last_update_time: last_update,
            prev_values: store_old,
            curr_values: store,
            record_to_query: to_query,
            charting_data: VecDeque::new(),
            chart_idx: 0,
            time_since_launch: launch_time_t,
            read_error: err_text
        }
    }
}

impl crate::window::InterfacePage for DiagnosticsPage {
    fn make_ui(&mut self, ui: &mut Ui, _frame: &eframe::Frame) -> PageAction {
        ui.heading("This is experimental, use with MOST up-to-date firmware");

        if ui.button("Query gearbox sensor").clicked() {
            *self.record_to_query.write().unwrap() = Some(RecordIdents::GearboxSensors);
            self.chart_idx = 0;
            self.charting_data.clear();
            *self.curr_values.write().unwrap() = None;
            *self.prev_values.write().unwrap() = None;
        }
        if ui.button("Query gearbox solenoids").clicked() {
            *self.record_to_query.write().unwrap() = Some(RecordIdents::SolenoidStatus);
            self.chart_idx = 0;
            self.charting_data.clear();
            *self.curr_values.write().unwrap() = None;
            *self.prev_values.write().unwrap() = None;
        }
        if ui.button("Query solenoid pressures").clicked() {
            *self.record_to_query.write().unwrap() = Some(RecordIdents::PressureStatus);
            self.chart_idx = 0;
            self.charting_data.clear();
            *self.curr_values.write().unwrap() = None;
            *self.prev_values.write().unwrap() = None;
        }
        if ui.button("Query can Rx data").clicked() {
            *self.record_to_query.write().unwrap() = Some(RecordIdents::CanDataDump);
            self.chart_idx = 0;
            self.charting_data.clear();
            *self.curr_values.write().unwrap() = None;
            *self.prev_values.write().unwrap() = None;
        }
        if ui.button("Query Shift data").clicked() {
            *self.record_to_query.write().unwrap() = Some(RecordIdents::SSData);
            self.chart_idx = 0;
            self.charting_data.clear();
            *self.curr_values.write().unwrap() = None;
            *self.prev_values.write().unwrap() = None;
        }

        if ui.button("Query Performance metrics").clicked() {
            *self.record_to_query.write().unwrap() = Some(RecordIdents::SysUsage);
            self.chart_idx = 0;
            self.charting_data.clear();
            *self.curr_values.write().unwrap() = None;
            *self.prev_values.write().unwrap() = None;
        }

        if ui.button("Query Clutch speeds").clicked() {
            *self.record_to_query.write().unwrap() = Some(RecordIdents::ClutchSpeeds);
            self.chart_idx = 0;
            self.charting_data.clear();
            *self.curr_values.write().unwrap() = None;
            *self.prev_values.write().unwrap() = None;
        }

        if ui.button("Query shift clutch velocities").clicked() {
            *self.record_to_query.write().unwrap() = Some(RecordIdents::ClutchVelocities);
            self.chart_idx = 0;
            self.charting_data.clear();
            *self.curr_values.write().unwrap() = None;
            *self.prev_values.write().unwrap() = None;
        }

        if let Some(e) = self.read_error.read().unwrap().clone() {
            ui.label(RichText::new(format!("Error querying ECU: {e}")).color(Color32::RED));
        }

        let current_val = self.curr_values.read().unwrap().clone();
        if let Some(data) = current_val {
            let prev_value = self.prev_values.read().unwrap().clone().unwrap_or(data.clone());
            let c = data.get_chart_data();

            if !c.is_empty() {
                let mut d = c[0].clone();
                // Compare to 
                let mut prev = d.clone();
                if let Some(p) = prev_value.get_chart_data().get(0).cloned() {
                    prev = p;
                }
                if prev != d {
                    // Linear interp when values differ
                    let ms_since_update = std::cmp::min(
                        RLI_QUERY_INTERVAL,
                        self.time_since_launch.elapsed().as_millis() as u64
                            - self.last_update_time.load(Ordering::Relaxed),
                    );
                    let mut proportion_curr: f32 = (ms_since_update as f32) / RLI_QUERY_INTERVAL as f32; // Percentage of old value to use
                    let mut proportion_prev: f32 = 1.0 - proportion_curr; // Percentage of curr value to use
                    if ms_since_update == 0 {
                        proportion_prev = 1.0;
                        proportion_curr = 0.0;
                    } else if ms_since_update == RLI_QUERY_INTERVAL {
                        proportion_prev = 0.0;
                        proportion_curr = 1.0;
                    }
                    for idx in 0..d.data.len() {
                        let interpolated = (prev.data[idx].1 * proportion_prev) + (d.data[idx].1 * proportion_curr);
                        d.data[idx].1 = interpolated;
                    }
                }
                self.charting_data.push_back((self.chart_idx, d.clone()));
                self.chart_idx+=1;
                if self.charting_data.len() > (50000 / 100) {
                    // 20 seconds
                    let _ = self.charting_data.pop_front();
                }

                // Can guarantee everything in `self.charting_data` will have the SAME length
                // as `d`
                let mut lines = Vec::new();
                let legend = Legend::default().position(eframe::egui::plot::Corner::LeftTop);
                for (idx, (key, _, _)) in d.data.iter().enumerate() {
                    let mut points: Vec<[f64; 2]> = Vec::new();
                    for (timestamp, point) in &self.charting_data {
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
                if self.chart_idx < 500 {
                    plot = plot.include_x(500);
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
