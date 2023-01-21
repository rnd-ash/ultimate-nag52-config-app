use eframe::{egui, epaint::Color32};
use plotters::style::RGBAColor;
use plotters_backend::BackendColor;


#[inline(always)]
pub fn into_egui_color(c: BackendColor) -> egui::Color32 {
    Color32::from_rgba_premultiplied(c.rgb.0, c.rgb.1, c.rgb.2, (c.alpha * 255.0) as u8)
}

#[inline(always)]
pub fn into_plotter_color(c: egui::Color32) -> BackendColor {
    BackendColor { alpha: c.a() as f64 / 255.0, rgb: (c.r(), c.g(), c.b()) }
}

#[inline(always)]
pub fn into_rgba_color(c: egui::Color32) -> RGBAColor {
    RGBAColor(c.r(), c.g(), c.b(), c.a() as f64 / 255.0)
}