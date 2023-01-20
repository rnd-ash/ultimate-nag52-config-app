use eframe::egui::*;
use std::{
    borrow::BorrowMut,
    collections::VecDeque,
    sync::{mpsc, Arc, Mutex, RwLock},
};

use crate::{
    window::{InterfacePage, StatusBar},
};
use eframe::egui;

#[derive(Clone)]
pub struct MainStatusBar {
    use_light_theme: bool,
}

impl MainStatusBar {
    pub fn new() -> Self {
        Self {
            use_light_theme: false,
        }
    }
}

impl StatusBar for MainStatusBar {
    fn draw(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::widgets::global_dark_light_mode_buttons(ui);
    }
}
