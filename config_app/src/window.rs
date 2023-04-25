use std::{
    collections::VecDeque,
    ops::Add,
    time::{Duration, Instant}, sync::Arc,
};

use backend::{diag::Nag52Diag, ecu_diagnostics::DiagError};
use eframe::{
    egui::{self, Direction, RichText, WidgetText, Sense, Button},
    epaint::{Pos2, Vec2},
};
use egui_toast::{Toast, ToastKind, ToastOptions, Toasts, ERROR_COLOR, SUCCESS_COLOR};

#[derive(Debug, Clone)]
pub enum PageLoadState {
    Ok,
    Waiting(&'static str),
    Err(String)
}

pub struct MainWindow {
    nag: Option<Nag52Diag>,
    pages: VecDeque<Box<dyn InterfacePage>>,
    curr_title: String,
    show_sbar: bool,
    show_back: bool,
    last_repaint_time: Instant
}

impl MainWindow {
    pub fn new() -> Self {
        Self {
            pages: VecDeque::new(),
            curr_title: "Ultimate-NAG52 config app".into(),
            show_sbar: false,
            show_back: true,
            nag: None,
            last_repaint_time: Instant::now()
        }
    }
    pub fn add_new_page(&mut self, p: Box<dyn InterfacePage>) {
        self.show_sbar = p.should_show_statusbar();
        self.pages.push_front(p)
    }

    pub fn pop_page(&mut self) {
        self.pages.pop_front();
        if let Some(pg) = self.pages.get_mut(0) {
            self.show_sbar = pg.should_show_statusbar()
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
                        }
                        let elapsed = self.last_repaint_time.elapsed().as_micros() as u64;
                        self.last_repaint_time = Instant::now();
                        row.label(format!("{} FPS", (1000*1000)/elapsed));
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
                    ctx.available_rect().height() - s_bar_height - 5.0,
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
                    PageAction::Add(p) => self.add_new_page(p),
                    PageAction::Overwrite(p) => {
                        self.pages[0] = p;
                        self.show_sbar = self.pages[0].should_show_statusbar();
                    }
                    PageAction::RePaint => ctx.request_repaint(),
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
        }
        ctx.request_repaint();
    }
}

pub enum PageAction {
    None,
    Destroy,
    RegisterNag(Nag52Diag),
    Add(Box<dyn InterfacePage>),
    DisableBackBtn,
    Overwrite(Box<dyn InterfacePage>),
    RePaint,
    SendNotification { text: String, kind: ToastKind },
}

pub trait InterfacePage {
    fn make_ui(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) -> PageAction;
    fn get_title(&self) -> &'static str;
    fn should_show_statusbar(&self) -> bool;
    fn destroy_nag(&self) -> bool {
        false
    }
}

pub trait StatusBar {
    fn draw(&mut self, ui: &mut egui::Ui, ctx: &egui::Context);
}
