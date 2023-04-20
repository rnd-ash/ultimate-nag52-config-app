use crate::ui::status_bar::MainStatusBar;
use crate::window::{PageAction, StatusBar};
use backend::diag::Nag52Diag;
use eframe::egui::plot::{Legend, Line, Plot};
use eframe::egui::{Color32, RichText, Ui};
use std::collections::hash_map::DefaultHasher;
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::time::Instant;

pub mod data;
pub mod rli;
pub mod solenoids;
use crate::ui::diagnostics::rli::{LocalRecordData, RecordIdents};

use self::rli::ChartData;

pub enum CommandStatus {
    Ok(String),
    Err(String),
}

pub struct DiagnosticsPage {
    bar: MainStatusBar,
    nag: Nag52Diag,
    text: CommandStatus,
    record_data: Option<LocalRecordData>,
    record_to_query: Option<RecordIdents>,
    last_query_time: Instant,
    charting_data: VecDeque<(u128, ChartData)>,
    chart_idx: u128,
}

impl DiagnosticsPage {
    pub fn new(nag: Nag52Diag, bar: MainStatusBar) -> Self {
        Self {
            nag,
            bar,
            text: CommandStatus::Ok("".into()),
            record_data: None,
            record_to_query: None,
            last_query_time: Instant::now(),
            charting_data: VecDeque::new(),
            chart_idx: 0,
        }
    }
}

impl crate::window::InterfacePage for DiagnosticsPage {
    fn make_ui(&mut self, ui: &mut Ui, _frame: &eframe::Frame) -> PageAction {
        let mut pending = false;
        ui.heading("This is experimental, use with MOST up-to-date firmware");

        if ui.button("Query gearbox sensor").clicked() {
            self.record_to_query = Some(RecordIdents::GearboxSensors);
            self.chart_idx = 0;
            self.charting_data.clear();
            self.record_data = None;
        }
        if ui.button("Query gearbox solenoids").clicked() {
            self.record_to_query = Some(RecordIdents::SolenoidStatus);
            self.chart_idx = 0;
            self.charting_data.clear();
            self.record_data = None;
        }
        if ui.button("Query solenoid pressures").clicked() {
            self.record_to_query = Some(RecordIdents::PressureStatus);
            self.chart_idx = 0;
            self.charting_data.clear();
            self.record_data = None;
        }
        if ui.button("Query can Rx data").clicked() {
            self.record_to_query = Some(RecordIdents::CanDataDump);
            self.chart_idx = 0;
            self.charting_data.clear();
            self.record_data = None;
        }
        if ui.button("Query Shift data").clicked() {
            self.record_to_query = Some(RecordIdents::SSData);
            self.chart_idx = 0;
            self.charting_data.clear();
            self.record_data = None;
        }

        if ui.button("Query Performance metrics").clicked() {
            self.record_to_query = Some(RecordIdents::SysUsage);
            self.chart_idx = 0;
            self.charting_data.clear();
            self.record_data = None;
        }

        match &self.text {
            CommandStatus::Ok(res) => {
                ui.label(RichText::new(res).color(Color32::from_rgb(0, 255, 0)));
            }
            CommandStatus::Err(res) => {
                ui.label(RichText::new(res).color(Color32::from_rgb(255, 0, 0)));
            }
        }

        if pending || (self.last_query_time.elapsed().as_millis() > 100) {
            self.last_query_time = Instant::now();
            self.chart_idx += 100;
            if let Some(rid) = self.record_to_query {
                match self.nag.with_kwp(|server| rid.query_ecu(server)) {
                    Ok(r) => self.record_data = Some(r),
                    Err(e) => {
                        eprintln!("Could not query {}", e);
                    }
                }
            }
        }

        if let Some(data) = &self.record_data {
            data.to_table(ui);

            let c = data.get_chart_data();

            if !c.is_empty() {
                let d = &c[0];
                self.charting_data.push_back((self.chart_idx, d.clone()));

                if self.charting_data.len() > (20000 / 100) {
                    // 20 seconds
                    let _ = self.charting_data.pop_front();
                }

                // Can guarantee everything in `self.charting_data` will have the SAME length
                // as `d`
                let mut lines = Vec::new();
                let legend = Legend::default();

                for (idx, (key, _, _)) in d.data.iter().enumerate() {
                    let mut points: Vec<[f64; 2]> = Vec::new();
                    for (timestamp, point) in &self.charting_data {
                        points.push([*timestamp as f64, point.data[idx].1 as f64])
                    }
                    let mut key_hasher = DefaultHasher::default();
                    key.hash(&mut key_hasher);
                    let r = key_hasher.finish();
                    lines.push(Line::new(points).name(key.clone()).color(Color32::from_rgb(
                        (r & 0xFF) as u8,
                        ((r >> 8) & 0xFF) as u8,
                        ((r >> 16) & 0xFF) as u8,
                    )))
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

                plot.show(ui, |plot_ui| {
                    for x in lines {
                        plot_ui.line(x)
                    }
                });
            }
            ui.ctx().request_repaint();
        }

        PageAction::None
    }

    fn get_title(&self) -> &'static str {
        "Ultimate-NAG52 diagnostics"
    }

    fn get_status_bar(&self) -> Option<Box<dyn StatusBar>> {
        Some(Box::new(self.bar.clone()))
    }
}
