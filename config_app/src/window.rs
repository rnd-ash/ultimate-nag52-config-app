use std::{
    collections::VecDeque,
    ops::Add,
    time::{Duration, Instant}, sync::Arc, borrow::BorrowMut,
};

use backend::{diag::Nag52Diag, ecu_diagnostics::{DiagError, dynamic_diag::ServerEvent}, hw::usb::{EspLogMessage, EspLogLevel}};
use eframe::{
    egui::{self, Direction, RichText, WidgetText, Sense, Button, ScrollArea, Context},
    epaint::{Pos2, Vec2, Color32, Rect, Rounding, FontId}, emath::Align2,
};
use egui_extras::{TableBuilder, Column};
use egui_toast::{Toast, ToastKind, ToastOptions, Toasts, ERROR_COLOR};

static mut GLOBAL_EGUI_CONTEXT: Option<Context> = None;

pub fn get_context() -> &'static Context {
    unsafe {
        GLOBAL_EGUI_CONTEXT.as_ref().unwrap()
    }
}

#[derive(Debug, Clone)]
pub enum PageLoadState {
    Ok,
    Waiting(String),
    Err(String)
}

impl PageLoadState {
    pub fn waiting<T: Into<String>>(s: T) -> Self {
        Self::Waiting(s.into())
    }
}

pub struct MainWindow {
    nag: Option<Arc<Nag52Diag>>,
    pages: VecDeque<Box<dyn InterfacePage>>,
    show_sbar: bool,
    show_back: bool,
    last_repaint_time: Instant,
    logs: VecDeque<EspLogMessage>,
    trace: VecDeque<String>,
    show_logger: bool,
    show_tracer: bool,
    last_data_query_time: Instant,
    last_tx_rate: u32,
    last_rx_rate: u32
}

impl MainWindow {
    pub fn new() -> Self {
        Self {
            pages: VecDeque::new(),
            show_sbar: false,
            show_back: true,
            nag: None,
            last_repaint_time: Instant::now(),
            logs: VecDeque::new(),
            trace: VecDeque::new(),
            show_logger: false,
            show_tracer: false,
            last_data_query_time: Instant::now(),
            last_tx_rate: 0,
            last_rx_rate: 0
        }
    }
    pub fn add_new_page(&mut self, p: Box<dyn InterfacePage>) {
        self.show_sbar = p.should_show_statusbar();
        self.pages.push_front(p);
        self.pages[0].on_load(self.nag.clone());
    }

    pub fn pop_page(&mut self) {
        self.pages.pop_front();
        if let Some(pg) = self.pages.get_mut(0) {
            self.show_sbar = pg.should_show_statusbar();
            if pg.nag_destroy_before_load() {
                drop(self.nag.take());
            }
            pg.on_load(self.nag.clone());
        }
    }
}

pub const MAX_BANDWIDTH: f32 = 155200.0 / 4.0;

impl eframe::App for MainWindow {
    fn update(&mut self, ctx: &eframe::egui::Context, frame: &mut eframe::Frame) {

        if unsafe { GLOBAL_EGUI_CONTEXT.is_none() } {
            unsafe { GLOBAL_EGUI_CONTEXT = Some(ctx.clone()) };
        }

        let stack_size = self.pages.len();
        let mut s_bar_height = 0.0;
        if stack_size > 0 {
            let mut pop_page = false;
            if self.show_sbar {
                egui::TopBottomPanel::bottom("NAV").show(ctx, |nav| {
                    nav.horizontal(|row| {
                        egui::widgets::global_dark_light_mode_buttons(row);
                        if stack_size > 1 {
                            if row.add_enabled(self.show_back, Button::new("Back")).clicked() {
                                pop_page = true;
                            }
                        }
                        if let Some(nag) = &self.nag {

                            let _ = nag.with_kwp(|f| {
                                if f.is_ecu_connected() {
                                    if let Some(mode) = f.get_current_diag_mode() {
                                        row.label(format!("Mode: {}(0x{:02X?})", mode.name, mode.id));
                                    } 
                                } else {
                                    row.label(RichText::new("Disconnected").color(ERROR_COLOR));
                                }
                                Ok(())
                            });

                            if nag.has_logger() {
                                if let Some(msg) = nag.read_log_msg() {
                                    self.logs.push_back(msg);
                                    if self.logs.len() > 1000 {
                                        self.logs.pop_front();
                                    }
                                }
                                if row.button("Show Log view").clicked() {
                                    self.show_logger = true;
                                }
                            } else {
                                row.label("Log view disabled (Connection is not USB)");
                            }

                            if row.button("Show packet trace").clicked() {
                                self.show_tracer = true;
                            }
                            if let Some(evt) = nag.get_server_event() {

                                let fmt_str = match evt {
                                    ServerEvent::ServerStart => format!("--Server start--"),
                                    ServerEvent::ServerExit => format!("--Server end--"),
                                    ServerEvent::BytesSendState(b, state) => {
                                        match state {
                                            Ok(_) => {
                                                format!("--> {:02X?}", b)
                                            },
                                            Err(e) => {
                                                format!("--> ERROR: {} - {:02X?}", e.to_string(), b)
                                            },
                                        }
                                    },
                                    ServerEvent::BytesRecvState(res) => {
                                        match res {
                                            Ok(b) => {
                                                format!("<-- {:02X?}", b)
                                            },
                                            Err(e) => {
                                                format!("<-- ERROR: {}", e.to_string())
                                            },
                                        }
                                    },
                                };
                                self.trace.push_back(fmt_str);
                                if self.trace.len() > 100 {
                                    self.trace.pop_front();
                                }
                                if self.show_logger {
                                    ctx.request_repaint();
                                }
                            }

                            let height = row.available_height();
                            
                            let (s_tx_resp, p_tx) = row.allocate_painter(Vec2::new(height, height), Sense::hover());
                            row.add_space(2.0);
                            let (s_rx_resp, p_rx) = row.allocate_painter(Vec2::new(height, height), Sense::hover());

                            let r_tx = s_tx_resp.rect;
                            let r_rx = s_rx_resp.rect;
                            
                            if self.last_data_query_time.elapsed().as_millis() > 250 {
                                if let Some((tx, rx)) = nag.get_data_rate() {
                                    self.last_tx_rate = tx;
                                    self.last_rx_rate = rx;
                                }
                                self.last_data_query_time = Instant::now();
                            }

                            let mut a_tx = (self.last_tx_rate as f32 / MAX_BANDWIDTH as f32) * 255.0;
                            if a_tx > 255.0 {
                                a_tx = 255.0
                            } else if a_tx > 0.0 && a_tx < 10.0 {
                                a_tx = 10.0
                            }

                            let mut a_rx = (self.last_rx_rate as f32 / MAX_BANDWIDTH as f32) * 255.0;
                            if a_rx > 255.0 {
                                a_rx = 255.0
                            } else if a_rx > 0.0 && a_rx < 10.0 {
                                a_rx = 10.0
                            }

                            let c_tx = Color32::from_rgba_unmultiplied(0, 255, 0, a_tx as u8);
                            let c_rx = Color32::from_rgba_unmultiplied(255, 0, 0, a_rx as u8);

                            p_tx.rect_filled(r_tx, Rounding::none(), c_tx);
                            p_rx.rect_filled(r_rx, Rounding::none(), c_rx);
                            p_tx.text(r_tx.center(), Align2::CENTER_CENTER, "Tx", FontId::monospace(10.0), Color32::WHITE);
                            p_rx.text(r_rx.center(), Align2::CENTER_CENTER, "Rx", FontId::monospace(10.0), Color32::WHITE);

                            s_tx_resp.on_hover_ui(|h| {
                                h.label(format!("{} B/s", self.last_tx_rate));
                            });

                            s_rx_resp.on_hover_ui(|h| {
                                h.label(format!("{} B/s", self.last_rx_rate));
                            });
                        }
                        let elapsed = self.last_repaint_time.elapsed().as_micros() as u64;
                        self.last_repaint_time = Instant::now();
                        row.label(format!("{:.3} FPS", (1000*1000)/elapsed));
                    });
                    s_bar_height = nav.available_height()
                });
            }
            if pop_page {
                self.pop_page();
            }

            let mut toasts = Toasts::new()
                .anchor(Pos2::new(
                    5.0,
                    ctx.available_rect().height() - s_bar_height - 10.0,
                ))
                .align_to_end(false)
                .direction(Direction::BottomUp);
            self.show_back = true;
            egui::CentralPanel::default().show(ctx, |main_win_ui| {
                match self.pages[0].make_ui(main_win_ui, frame) {
                    PageAction::None => {}
                    PageAction::Destroy => {
                        if self.pages[0].destroy_nag() {
                            self.nag = None;
                        }
                        self.pop_page()
                    },
                    PageAction::Add(mut p) => {
                        self.add_new_page(p)
                    },
                    PageAction::Overwrite(p) => {
                        self.pages[0] = p;
                        self.pages[0].on_load(self.nag.clone());
                        self.show_sbar = self.pages[0].should_show_statusbar();
                    }
                    PageAction::DisableBackBtn => {
                        self.show_back = false;
                    }
                    PageAction::SendNotification { text, kind } => {
                        println!("Pushing notification {}", text);
                        toasts.add(Toast {
                            kind,
                            text: WidgetText::RichText(RichText::new(text)),
                            options: ToastOptions {
                                show_icon: true,
                                expires_at: Some(Instant::now().add(Duration::from_secs(5))),
                            },
                        });
                    }
                    PageAction::RegisterNag(n) => {
                        self.nag = Some(n)
                    },
                }
            });
            toasts.show(&ctx);

            // Show Log viewer
            if self.show_logger {
                egui::Window::new("Log view").open(&mut self.show_logger).show(ctx, |ui| {
                    let is_dark = ctx.style().visuals.dark_mode;
                    let table = TableBuilder::new(ui)
                        .striped(false)
                        .resizable(true)
                        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                        .column(Column::auto()) // Level
                        .column(Column::initial(100.0).at_least(40.0)) // Timestamp
                        .column(Column::initial(100.0).range(40.0..=300.0).clip(true)) // Module
                        .column(Column::remainder()) // Message
                        .stick_to_bottom(true)
                        .max_scroll_height(400.0)
                        .min_scrolled_height(100.0);

                    table.header(20.0, |mut header| {
                        header.col(|ui| {
                            ui.strong("Level");
                        });
                        header.col(|ui| {
                            ui.strong("Since boot");
                        });
                        header.col(|ui| {
                            ui.strong("Module");
                        });
                        header.col(|ui| {
                            ui.strong("Message");
                        });
                    }).body(|mut body| {
                        body.rows(10.0, self.logs.len(), |row_index, mut row| {
                            let msg = &self.logs[row_index];
                            let c = match msg.lvl {
                                EspLogLevel::Debug => Color32::DEBUG_COLOR,
                                EspLogLevel::Info => if is_dark { Color32::GREEN } else { Color32::DARK_GREEN },
                                EspLogLevel::Warn => if is_dark { Color32::YELLOW } else { Color32::GOLD },
                                EspLogLevel::Error => if is_dark { Color32::RED } else { Color32::DARK_RED },
                            };
                            row.col(|ui| {
                                let l_txt = match &msg.lvl {
                                    EspLogLevel::Debug => "DEBUG",
                                    EspLogLevel::Info => "INFO",
                                    EspLogLevel::Warn => "WARN",
                                    EspLogLevel::Error => "ERROR",
                                };
                                ui.label(RichText::new(l_txt).color(c));
                            });
                            row.col(|ui| {
                                ui.label(RichText::new(format!("{} Ms", msg.timestamp)).color(c));
                            });
                            row.col(|ui| {
                                ui.label(RichText::new(&msg.tag).color(c));
                            });
                            row.col(|ui| {
                                ui.label(RichText::new(&msg.msg).color(c));
                            });
                        })
                    });
                });
            }

            if self.show_tracer {
                egui::Window::new("packet trace").open(&mut self.show_tracer).show(ctx, |ui| {
                    let r = ScrollArea::new([true, true]).stick_to_bottom(true).max_height(300.0).max_width(600.0).show(ui, |s| {
                        for x in &self.trace {
                            s.label(x);
                        }
                    });
                });
            }
        }
    }
}

pub enum PageAction {
    None,
    Destroy,
    RegisterNag(Arc<Nag52Diag>),
    Add(Box<dyn InterfacePage>),
    DisableBackBtn,
    Overwrite(Box<dyn InterfacePage>),
    SendNotification { text: String, kind: ToastKind },
}

pub trait InterfacePage {
    fn make_ui(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) -> PageAction;
    fn get_title(&self) -> &'static str;
    fn should_show_statusbar(&self) -> bool;
    fn destroy_nag(&self) -> bool {
        false
    }
    fn on_load(&mut self, nag: Option<Arc<Nag52Diag>>){}
    fn nag_destroy_before_load(&self) -> bool {
        false
    }
}

pub trait StatusBar {
    fn draw(&mut self, ui: &mut egui::Ui, ctx: &egui::Context);
}
