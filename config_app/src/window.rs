use std::{
    collections::VecDeque,
    ops::Add,
    time::{Duration, Instant}, sync::Arc,
};

use backend::{diag::Nag52Diag, ecu_diagnostics::DiagError, hw::usb::{EspLogMessage, EspLogLevel}};
use eframe::{
    egui::{self, Direction, RichText, WidgetText, Sense, Button, ScrollArea},
    epaint::{Pos2, Vec2, Color32},
};
use egui_extras::{TableBuilder, Column};
use egui_toast::{Toast, ToastKind, ToastOptions, Toasts, ERROR_COLOR, SUCCESS_COLOR};

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
    curr_title: String,
    show_sbar: bool,
    show_back: bool,
    last_repaint_time: Instant,
    logs: VecDeque<EspLogMessage>,
    show_logger: bool,
}

impl MainWindow {
    pub fn new() -> Self {
        Self {
            pages: VecDeque::new(),
            curr_title: "Ultimate-NAG52 config app".into(),
            show_sbar: false,
            show_back: true,
            nag: None,
            last_repaint_time: Instant::now(),
            logs: VecDeque::new(),
            show_logger: false
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
            pg.on_load(self.nag.clone());
        }
    }
}

impl eframe::App for MainWindow {
    fn update(&mut self, ctx: &eframe::egui::Context, frame: &mut eframe::Frame) {
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
                        let elapsed = self.last_repaint_time.elapsed().as_micros() as u64;
                        self.last_repaint_time = Instant::now();
                        row.label(format!("{} FPS", (1000*1000)/elapsed));
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
                            if nag.can_read_log() {
                                while let Some(msg) = nag.read_log_msg() {
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
                        }
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
        }
        ctx.request_repaint_after(Duration::from_millis(1000/60));
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
}

pub trait StatusBar {
    fn draw(&mut self, ui: &mut egui::Ui, ctx: &egui::Context);
}
