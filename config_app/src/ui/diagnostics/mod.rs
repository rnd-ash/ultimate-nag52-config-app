use crate::window::{PageAction, StatusBar, get_context};
use backend::diag::Nag52Diag;
use backend::ecu_diagnostics::kwp2000::{KwpSessionTypeByte, KwpSessionType};
use chrono::Local;
use egui_extras::Size;
use egui_plot::{Legend, Line, Plot, PlotPoints};
use eframe::egui::{Color32, RichText, Ui, Context};
use eframe::epaint::Stroke;
use eframe::epaint::mutex::RwLock;
use std::borrow::Borrow;
use std::collections::hash_map::DefaultHasher;
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::sync::{Arc};
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
    charting_data: Arc<RwLock<VecDeque<(u128, Vec<ChartData>)>>>,
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

        let store_old = Arc::new(RwLock::new(Option::<LocalRecordData>::None));
        let store_old_t = store_old.clone();

        let to_query: Arc<RwLock<Option<RecordIdents>>> = Arc::new(RwLock::new(None));
        let to_query_t = to_query.clone();
        let last_update = Arc::new(AtomicU64::new(0));
        let last_update_t = last_update.clone();

        let launch_time = Instant::now();
        let launch_time_t = launch_time.clone();

        let rli_start_time = Arc::new(AtomicU64::new(0));

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
                if let Some(to_query) = to_query_t.read().clone() {
                    match nag.with_kwp(|server| to_query.query_ecu(server)) {
                        Ok(r) => {
                            let cd = r.get_chart_data();
                            *store_old_t.write() = store_t.read().clone();
                            *store_t.write() = Some(r);
                            let mut m = charting_data_t.write();
                            m.push_back((launch_time_t.elapsed().as_millis(), cd));
                            if launch_time_t.elapsed().as_millis() - m[0].0 > 20000 {
                                m.pop_front();
                            }
                            drop(m);
                            last_update_t.store(
                                launch_time_t.elapsed().as_millis() as u64,
                                Ordering::Relaxed,
                            );

                        },
                        Err(e) => {
                            *err_text_t.write() = Some(e.to_string());
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
        ui.add_space(5.0);
        let ui_height = ui.available_height();
        let current_val = self.curr_values.read().clone();
        let chart_data = self.charting_data.read().clone();
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                let mut rli_reset = false;
                if ui.button("Query gearbox sensor").clicked() {
                    *self.record_to_query.write() = Some(RecordIdents::GearboxSensors);
                    rli_reset = true;
                }
                if ui.button("Query gearbox solenoids").clicked() {
                    *self.record_to_query.write() = Some(RecordIdents::SolenoidStatus);
                    rli_reset = true;
                }
                if ui.button("Query solenoid pressures").clicked() {
                    *self.record_to_query.write() = Some(RecordIdents::PressureStatus);
                    rli_reset = true;
                }
                if ui.button("Query can Rx data").clicked() {
                    *self.record_to_query.write() = Some(RecordIdents::CanDataDump);
                    rli_reset = true;
                }
                if ui.button("Query Shift data").clicked() {
                    *self.record_to_query.write() = Some(RecordIdents::SSData);
                    rli_reset = true;
                }
                if ui.button("Query Performance metrics").clicked() {
                    *self.record_to_query.write() = Some(RecordIdents::SysUsage);
                    rli_reset = true;
                }
                if ui.button("Query Clutch speeds").clicked() {
                    *self.record_to_query.write() = Some(RecordIdents::ClutchSpeeds);
                    rli_reset = true;
                }
                if ui.button("Query shift algorithm data").clicked() {
                    *self.record_to_query.write() = Some(RecordIdents::ShiftingAlgoFeedback);
                    rli_reset = true;
                }
                if ui.button("Query TCC Program data").clicked() {
                    *self.record_to_query.write() = Some(RecordIdents::TccProgram);
                    rli_reset = true;
                }

                if rli_reset {
                    self.chart_idx = 0;
                    self.charting_data.write().clear();
                    *self.curr_values.write() = None;
                    *self.prev_values.write() = None;
                    self.rli_start_time.store(self.launch_time.elapsed().as_millis() as u64, Ordering::Relaxed);
                }

                if let Some(e) = self.read_error.read().clone() {
                    ui.label(RichText::new(format!("Error querying ECU: {e}")).color(Color32::RED));
                }
                if let Some(data) = current_val.clone() {
                    data.to_table(ui);
                }
            });

            if let Some(data) = current_val {
                ui.vertical(|col| {
                    let start_time = self.rli_start_time.load(Ordering::Relaxed);
                    let legend = Legend::default().position(egui_plot::Corner::LeftTop);
                    let space_per_chart = (ui_height / data.get_chart_data().len() as f32);

                    let builder = egui_extras::StripBuilder::new(col)
                        .sizes(Size::exact(space_per_chart), data.get_chart_data().len())
                        .vertical(|mut strip| {
                            for (idx, d) in data.get_chart_data().iter().enumerate() {
                                strip.cell(|ui| {
                                    let mut lines: Vec<Line> = Vec::new();
                                    ui.strong(d.group_name.clone());
                                    let unit: Option<&'static str> =  d.data[0].2.clone();
                                    
                                    for (i, (key, _, _, color)) in d.data.iter().enumerate() {
                                        let points: PlotPoints = chart_data.iter().map(|(timestamp, value)| {
                                            [*timestamp as f64 - start_time as f64, value[idx].data[i].1 as f64]
                                        }).collect();
                                        lines.push(Line::new(points).name(key.clone()).stroke(Stroke::new(2.0, color.clone())));
                                    }
            
                                    let now = self.launch_time.elapsed().as_millis() - start_time as u128;
                                    let mut last_bound = now as f64 - 20000.0;
                                    if last_bound < 0.0 {
                                        last_bound = 0.0;
                                    }
                                    let x = unit.clone();
                                    let mut plot = Plot::new(d.group_name.clone())
                                        //.height(space_per_chart)
                                        .allow_drag(false)
                                        .include_x(std::cmp::max(20000, now) as f64)
                                        .auto_bounds([true, true].into())
                                        .legend(legend.clone())
                                        .x_axis_formatter(|f, r| {
                                            let seconds = f.value / 1000.0;
                                            let mins = (f.value / 60000.0) as u32;
                                            format!("{:02}:{:02.1}", mins, seconds)
                                        })
                                        .y_axis_formatter(move |f, r| {
                                            if let Some(u) = x.clone() {
                                                format!("{}{}", f.value, u)
                                            } else {
                                                f.value.to_string()
                                            }
                                        });
                                    if let Some((min, max)) = &d.bounds {
                                        plot = plot.include_y(*min);
                                        if *max > 0.1 {
                                            // 0.0 check
                                            plot = plot.include_y(*max);
                                        }
                                    }
                                    plot.show(ui, |f| {
                                        for line in lines {
                                            f.line(line);
                                        }
                                    });
                                });
                            }
                        });
                });
            }
        });
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
