use std::{
    borrow::BorrowMut,
    collections::HashMap,
    fmt::Display,
    sync::{Arc, Mutex}, hash::Hash,
};

use backend::{
    diag::Nag52Diag,
    ecu_diagnostics::{
        DiagError, DiagServerResult, kwp2000::{KwpCommand, KwpSessionTypeByte},
    },
};
use eframe::{
    egui::{
        self,
        plot::{Bar, BarChart, CoordinatesFormatter, HLine, Legend, Line, LineStyle, PlotPoints},
        Layout, Response, RichText, TextEdit, Ui,
    },
    epaint::{vec2, Color32, FontId, Stroke, TextShape, Rect, Pos2}, emath::lerp,
};
use egui_extras::{Size, Table, TableBuilder, Column};
use egui_toast::ToastKind;
use nom::number::complete::le_u16;
use plotters::{prelude::{IntoDrawingArea, ChartBuilder, Rectangle}, style::{WHITE, BLACK, BLUE, Color}, series::SurfaceSeries};
mod help_view;
mod map_list;
mod map_widget;
use crate::{window::PageAction, plot_backend::{EguiPlotBackend, into_rgba_color}};
use map_list::MAP_ARRAY;
use plotters::prelude::*;

use self::{help_view::HelpView, map_widget::MapWidget};

use super::{
    configuration::{
        self,
        cfg_structs::{EngineType, TcmCoreConfig},
    },
};

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MapCmd {
    Read = 0x01,
    ReadDefault = 0x02,
    Write = 0x03,
    Burn = 0x04,
    ResetToFlash = 0x05,
    Undo = 0x06,
    ReadMeta = 0x07,
    ReadEEPROM = 0x08,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MapViewType {
    EEPROM,
    Default,
    Modify,
}

fn pdf(x: f64, y: f64) -> f64 {
    const SDX: f64 = 0.1;
    const SDY: f64 = 0.1;
    const A: f64 = 5.0;
    let x = x as f64 / 10.0;
    let y = y as f64 / 10.0;
    A * (-x * x / 2.0 / SDX / SDX - y * y / 2.0 / SDY / SDY).exp()
}

#[derive(Debug, Clone)]
pub struct Map {
    meta: MapData,
    x_values: Vec<i16>,
    y_values: Vec<i16>,
    eeprom_key: String,
    /// EEPROM data
    data_eeprom: Vec<i16>,
    /// Map data in memory NOW
    data_memory: Vec<i16>,
    /// Program default map
    data_program: Vec<i16>,
    /// User editing map
    data_modify: Vec<i16>,
    showing_default: bool,
    ecu_ref: Nag52Diag,
    curr_edit_cell: Option<(usize, String, Response)>,
    view_type: MapViewType,
    pitch: f64,
    rot: f64,
    last_draw_hash: u32
}

fn read_i16(a: &[u8]) -> DiagServerResult<(&[u8], i16)> {
    if a.len() < 2 {
        return Err(DiagError::InvalidResponseLength);
    }
    let r = i16::from_le_bytes(a[0..2].try_into().unwrap());
    Ok((&a[2..], r))
}

fn read_u16(a: &[u8]) -> DiagServerResult<(&[u8], u16)> {
    if a.len() < 2 {
        return Err(DiagError::InvalidResponseLength);
    }
    let r = u16::from_le_bytes(a[0..2].try_into().unwrap());
    Ok((&a[2..], r))
}

// https://github.com/emilk/egui/blob/master/crates/egui/src/widgets/plot/mod.rs
fn color_from_contrast(ui: &Ui, contrast: f32) -> Color32 {
    let bg = ui.visuals().extreme_bg_color;
    let fg = ui.visuals().widgets.open.fg_stroke.color;
    let mix = 0.5 * contrast.sqrt();
    Color32::from_rgb(
        lerp((bg.r() as f32)..=(fg.r() as f32), mix) as u8,
        lerp((bg.g() as f32)..=(fg.g() as f32), mix) as u8,
        lerp((bg.b() as f32)..=(fg.b() as f32), mix) as u8,
    )
}

impl Map {
    pub fn new(map_id: u8, mut nag: Nag52Diag, meta: MapData) -> DiagServerResult<Self> {
        // Read metadata

        let ecu_response = nag.with_kwp(|server| {
            server
                .send_byte_array_with_response(&[
                    KwpCommand::ReadDataByLocalIdentifier.into(),
                    0x19,
                    map_id,
                    MapCmd::ReadMeta as u8,
                    0x00,
                    0x00,
                ])
                .map(|mut x| {
                    x.drain(0..1);
                    x
                })
        })?;
        let (data, data_len) = read_u16(&ecu_response)?;
        if data.len() != data_len as usize {
            return Err(DiagError::InvalidResponseLength);
        }
        let (data, x_element_count) = read_u16(data)?;
        let (data, y_element_count) = read_u16(data)?;
        let (mut data, key_len) = read_u16(data)?;
        if data.len() as u16 != ((x_element_count + y_element_count) * 2) + key_len {
            return Err(DiagError::InvalidResponseLength);
        }
        let mut x_elements: Vec<i16> = Vec::new();
        let mut y_elements: Vec<i16> = Vec::new();
        for _ in 0..x_element_count {
            let (d, v) = read_i16(data)?;
            x_elements.push(v);
            data = d;
        }
        for _ in 0..y_element_count {
            let (d, v) = read_i16(data)?;
            y_elements.push(v);
            data = d;
        }
        let key = String::from_utf8(data.to_vec()).unwrap();

        let mut default: Vec<i16> = Vec::new();
        let mut current: Vec<i16> = Vec::new();
        let mut eeprom: Vec<i16> = Vec::new();

        // Read current data
        let ecu_response = nag.with_kwp(|server| {
            server
                .send_byte_array_with_response(&[
                    KwpCommand::ReadDataByLocalIdentifier.into(),
                    0x19,
                    map_id,
                    MapCmd::Read as u8,
                    0x00,
                    0x00,
                ])
                .map(|mut x| {
                    x.drain(0..1);
                    x
                })
        })?;
        let (mut c_data, c_arr_size) = read_u16(&ecu_response)?;
        if c_data.len() != c_arr_size as usize {
            return Err(DiagError::InvalidResponseLength);
        }
        for _ in 0..(c_arr_size / 2) {
            let (d, v) = read_i16(c_data)?;
            current.push(v);
            c_data = d;
        }
        // Read default data
        let ecu_response = nag.with_kwp(|server| {
            server
                .send_byte_array_with_response(&[
                    KwpCommand::ReadDataByLocalIdentifier.into(),
                    0x19,
                    map_id,
                    MapCmd::ReadDefault as u8,
                    0x00,
                    0x00,
                ])
                .map(|mut x| {
                    x.drain(0..1);
                    x
                })
        })?;
        let (mut d_data, d_arr_size) = read_u16(&ecu_response)?;
        if d_data.len() != d_arr_size as usize {
            return Err(DiagError::InvalidResponseLength);
        }
        for _ in 0..(d_arr_size / 2) {
            let (d, v) = read_i16(d_data)?;
            default.push(v);
            d_data = d;
        }
        let ecu_response = nag.with_kwp(|server| {
            server
                .send_byte_array_with_response(&[
                    KwpCommand::ReadDataByLocalIdentifier.into(),
                    0x19,
                    map_id,
                    MapCmd::ReadEEPROM as u8,
                    0x00,
                    0x00,
                ])
                .map(|mut x| {
                    x.drain(0..1);
                    x
                })
        })?;
        let (mut e_data, e_arr_size) = read_u16(&ecu_response)?;
        if e_data.len() != e_arr_size as usize {
            return Err(DiagError::InvalidResponseLength);
        }
        for _ in 0..(e_arr_size / 2) {
            let (d, v) = read_i16(e_data)?;
            eeprom.push(v);
            e_data = d;
        }

        Ok(Self {
            x_values: x_elements,
            y_values: y_elements,
            eeprom_key: key,
            data_eeprom: eeprom,
            data_memory: current.clone(),
            data_program: default,
            data_modify: current,
            meta,
            showing_default: false,
            ecu_ref: nag,
            curr_edit_cell: None,
            view_type: MapViewType::Modify,
            pitch: 0.8,
            rot: 0.8,
            last_draw_hash: 0
        })
    }

    fn data_to_byte_array(&self, data: &Vec<i16>) -> Vec<u8> {
        let mut ret = Vec::new();
        ret.extend_from_slice(&((data.len() * 2) as u16).to_le_bytes());
        for point in data {
            ret.extend_from_slice(&point.to_le_bytes());
        }
        ret
    }

    pub fn write_to_ram(&mut self) -> DiagServerResult<()> {
        let mut payload: Vec<u8> = vec![
            KwpCommand::WriteDataByLocalIdentifier.into(),
            0x19,
            self.meta.id,
            MapCmd::Write as u8,
        ];
        payload.extend_from_slice(&self.data_to_byte_array(&self.data_modify));
        self.ecu_ref
            .with_kwp(|server| server.send_byte_array_with_response(&payload))?;
        Ok(())
    }

    pub fn save_to_eeprom(&mut self) -> DiagServerResult<()> {
        let payload: Vec<u8> = vec![
            KwpCommand::WriteDataByLocalIdentifier.into(),
            0x19,
            self.meta.id,
            MapCmd::Burn as u8,
            0x00,
            0x00,
        ];
        self.ecu_ref
            .with_kwp(|server| server.send_byte_array_with_response(&payload))?;
        Ok(())
    }

    pub fn undo_changes(&mut self) -> DiagServerResult<()> {
        let payload: Vec<u8> = vec![
            KwpCommand::WriteDataByLocalIdentifier.into(),
            0x19,
            self.meta.id,
            MapCmd::Undo as u8,
            0x00,
            0x00,
        ];
        self.ecu_ref
            .with_kwp(|server| server.send_byte_array_with_response(&payload))?;
        Ok(())
    }

    fn get_x_label(&self, idx: usize) -> String {
        if let Some(replace) = self.meta.x_replace {
            format!("{}", replace.get(idx).unwrap_or(&"ERROR"))
        } else {
            format!("{} {}", self.x_values[idx], self.meta.x_unit)
        }
    }

    fn get_y_label(&self, idx: usize) -> String {
        if let Some(replace) = self.meta.y_replace {
            format!("{}", replace.get(idx).unwrap_or(&"ERROR"))
        } else {
            format!("{} {}", self.y_values[idx], self.meta.y_unit)
        }
    }

    fn gen_edit_table(&mut self, raw_ui: &mut egui::Ui) {
        let hash = match self.view_type {
            MapViewType::EEPROM => &self.data_eeprom,
            MapViewType::Default => &self.data_program,
            MapViewType::Modify => &self.data_modify,
        };
        let header_color = raw_ui.visuals().warn_fg_color;
        let cell_edit_color = raw_ui.visuals().error_fg_color;
        if !self.meta.x_desc.is_empty() {
            raw_ui.label(format!("X: {}", self.meta.x_desc));
        }
        if !self.meta.y_desc.is_empty() {
            raw_ui.label(format!("Y: {}", self.meta.y_desc));
        }
        if !self.meta.v_desc.is_empty() {
            raw_ui.label(format!("Values: {}", self.meta.v_desc));
        }
        let mut copy = self.clone();
        let resp = raw_ui.push_id(&hash, |ui| {
            let mut table_builder = egui_extras::TableBuilder::new(ui)
                .striped(true)
                .scroll(false)
                .cell_layout(
                    Layout::left_to_right(egui::Align::Center)
                        .with_cross_align(egui::Align::Center),
                )
                .column(Column::initial(60.0).at_least(60.0));
            for _ in 0..copy.x_values.len() {
                table_builder = table_builder.column(Column::initial(70.0).at_least(70.0));
            }
            table_builder
                .header(15.0, |mut header| {
                    header.col(|_| {}); // Nothing in corner cell
                    if copy.x_values.len() == 1 {
                        header.col(|_| {});
                    } else {
                        for v in 0..copy.x_values.len() {
                            header.col(|u| {
                                u.label(
                                    RichText::new(format!("{}", copy.get_x_label(v)))
                                        .color(header_color),
                                );
                            });
                        }
                    }
                })
                .body(|body| {
                    body.rows(15.0, copy.y_values.len(), |row_id, mut row| {
                        // Header column
                        row.col(|c| {
                            c.label(
                                RichText::new(format!("{}", copy.get_y_label(row_id)))
                                    .color(header_color),
                            );
                        });

                        // Data columns
                        for x_pos in 0..copy.x_values.len() {
                            row.col(|cell| match self.view_type {
                                MapViewType::EEPROM => {
                                    cell.label(format!(
                                        "{}",
                                        copy.data_eeprom[(row_id * copy.x_values.len()) + x_pos]
                                    ));
                                }
                                MapViewType::Default => {
                                    cell.label(format!(
                                        "{}",
                                        copy.data_program[(row_id * copy.x_values.len()) + x_pos]
                                    ));
                                }
                                MapViewType::Modify => {
                                    let map_idx = (row_id * copy.x_values.len()) + x_pos;
                                    let mut value = format!("{}", copy.data_modify[map_idx]);
                                    if let Some((curr_edit_idx, current_edit_txt, resp)) =
                                        &copy.curr_edit_cell
                                    {
                                        if *curr_edit_idx == map_idx {
                                            value = current_edit_txt.clone();
                                        }
                                    }
                                    let changed_value =
                                        value != format!("{}", copy.data_eeprom[map_idx]);
                                    let mut edit = TextEdit::singleline(&mut value);
                                    if changed_value {
                                        edit = edit.text_color(cell_edit_color);
                                    }
                                    let mut response = cell.add(edit);
                                    if changed_value {
                                        response = response.on_hover_text(format!(
                                            "Current in EEPROM: {}",
                                            copy.data_eeprom[map_idx]
                                        ));
                                    }                              
                                    if response.lost_focus()
                                        || cell.ctx().input(|x| x.key_pressed(egui::Key::Enter))
                                    {
                                        if let Ok(new_v) = i16::from_str_radix(&value, 10) {
                                            copy.data_modify[map_idx] = new_v;
                                        }
                                        copy.curr_edit_cell = None;
                                    } else if response.gained_focus() || response.has_focus() {
                                        if let Some((curr_edit_idx, current_edit_txt, _resp)) =
                                            &copy.curr_edit_cell
                                        {
                                            if let Ok(new_v) =
                                                i16::from_str_radix(&current_edit_txt, 10)
                                            {
                                                copy.data_modify[*curr_edit_idx] = new_v;
                                            }
                                        }
                                        copy.curr_edit_cell = Some((map_idx, value, response));
                                    }
                                }
                            });
                        }
                    })
                });
        });
        *self = copy;
    }

    fn generate_window_ui(&mut self, raw_ui: &mut egui::Ui) -> Option<PageAction> {
        raw_ui.label(format!("EEPROM key: {}", self.eeprom_key));
        raw_ui.label(format!(
            "Map has {} elements",
            self.x_values.len() * self.y_values.len()
        ));
        self.gen_edit_table(raw_ui);
        // Generate display chart
        if self.x_values.len() == 1 {
            // Bar chart
            let mut bars = Vec::new();
            for x in 0..self.y_values.len() {
                // Distinct points
                let value = match self.view_type {
                    MapViewType::Default => self.data_program[x],
                    MapViewType::EEPROM => self.data_eeprom[x],
                    MapViewType::Modify => self.data_modify[x],
                };
                let key = self.get_y_label(x);
                bars.push(Bar::new(x as f64, value as f64).name(key))
            }
            egui::plot::Plot::new(format!("PLOT-{}", self.eeprom_key))
                .allow_drag(false)
                .allow_scroll(false)
                .allow_zoom(false)
                .height(150.0)
                .include_x(0)
                .include_y((self.y_values.len() + 1) as f64 * 1.5)
                .show(raw_ui, |plot_ui| plot_ui.bar_chart(BarChart::new(bars)));
        } else if (self.meta.x_replace.is_some() || self.meta.y_replace.is_some()) {
            // Line chart
            let mut lines: Vec<Line> = Vec::new();
            for (y_idx, key) in self.y_values.iter().enumerate() {
                let mut points: Vec<[f64; 2]> = Vec::new();
                for (x_idx, key) in self.x_values.iter().enumerate() {
                    let map_idx = (y_idx * self.x_values.len()) + x_idx;
                    let data = match self.view_type {
                        MapViewType::Default => self.data_program[map_idx],
                        MapViewType::EEPROM => self.data_eeprom[map_idx],
                        MapViewType::Modify => self.data_modify[map_idx],
                    };
                    points.push([*key as f64, data as f64]);
                }
                lines.push(Line::new(points).name(self.get_y_label(y_idx)));
            }
            egui::plot::Plot::new(format!("PLOT-{}", self.eeprom_key))
                .allow_drag(false)
                .allow_scroll(false)
                .allow_zoom(false)
                .height(150.0)
                .width(raw_ui.available_width())
                .show(raw_ui, |plot_ui| {
                    for l in lines {
                        plot_ui.line(l);
                    }
                });
        } else {
            let src = match self.view_type {
                MapViewType::Default => &self.data_program,
                MapViewType::EEPROM => &self.data_eeprom,
                MapViewType::Modify => &self.data_modify,
            };
            let desired_size = egui::Vec2::new(raw_ui.available_width(), 400.0);
            let (rect, response) = raw_ui.allocate_exact_size(desired_size, egui::Sense::drag());
            let painter = raw_ui.painter_at(rect);
            let area = EguiPlotBackend::new(painter, raw_ui.style().to_owned()).into_drawing_area();
            
            let x_min = *self.x_values.iter().min().unwrap() as f64;
            let x_max = *self.x_values.iter().max().unwrap() as f64;
            let z_min = *self.y_values.iter().min().unwrap() as f64;
            let z_max = *self.y_values.iter().max().unwrap() as f64;

            let y_min = *src.iter().min().unwrap() as f64;
            let y_max = *src.iter().max().unwrap() as f64;

            self.pitch += response.drag_delta().y as f64 /30.0;
            self.rot += response.drag_delta().x as f64 /30.0;
            if self.pitch < 0.0 {
                self.pitch = 0.0;
            } else if self.pitch > 1.57 {
                self.pitch = 1.57;
            }
            let vis = &raw_ui.ctx().style().visuals;
            area.fill(&into_rgba_color(vis.extreme_bg_color));
            let mut chart = ChartBuilder::on(&area)
                .build_cartesian_3d(x_min..x_max, y_min..y_max, z_min..z_max).unwrap();
                chart.with_projection(|mut p| {
                p.pitch = self.pitch; //0.8;
                p.scale = 0.75;
                p.yaw = self.rot;
                p.into_matrix() // build the projection matrix
            });


            chart
                .configure_axes()
                .x_labels(self.x_values.len())
                .y_labels(10)
                .z_labels(self.y_values.len())
                .light_grid_style(into_rgba_color(vis.text_color()))
                .max_light_lines(1)
                .draw().unwrap();

            chart.draw_series(
                SurfaceSeries::xoz(
                    self.x_values.iter().map(|x| *x as f64),
                    self.y_values.iter().map(|y| *y as f64),
                    |x, y| {
                        let x_v = x as i16;
                        let y_v = y as i16;
                        let x_idx = self.x_values.iter().position(|s| *s == x_v).unwrap();
                        let y_idx = self.y_values.iter().position(|s| *s == y_v).unwrap();
                        let len = self.x_values.len();
                        src[(len*y_idx)+x_idx] as f64
                    }
                )
                .style_func(&|&v| {
                    (&HSLColor((v / y_max)*0.3, 1.0, 0.5)).into()
                })
            )
            .unwrap();
            area.present();
        }
        raw_ui.label("View mode:");
        raw_ui.horizontal(|row| {
            row.selectable_value(&mut self.view_type, MapViewType::Modify, "User changes");
            row.selectable_value(&mut self.view_type, MapViewType::EEPROM, "EEPROM");
            row.selectable_value(&mut self.view_type, MapViewType::Default, "TCU default");
        });
        if self.data_modify != self.data_eeprom {
            if raw_ui.button("Undo user changes").clicked() {
                return match self.undo_changes() {
                    Ok(_) => {
                        self.data_modify = self.data_eeprom.clone();
                        Some(PageAction::SendNotification {
                            text: format!("Map {} undo OK!", self.eeprom_key),
                            kind: ToastKind::Success,
                        })
                    }
                    Err(e) => Some(PageAction::SendNotification {
                        text: format!("Map {} undo failed! {}", self.eeprom_key, e),
                        kind: ToastKind::Error,
                    }),
                };
            }
            if raw_ui.button("Write changes (To RAM)").clicked() {
                return match self.write_to_ram() {
                    Ok(_) => {
                        self.data_memory = self.data_modify.clone();
                        Some(PageAction::SendNotification {
                            text: format!("Map {} RAM write OK!", self.eeprom_key),
                            kind: ToastKind::Success,
                        })
                    }
                    Err(e) => Some(PageAction::SendNotification {
                        text: format!("Map {} RAM write failed! {}", self.eeprom_key, e),
                        kind: ToastKind::Error,
                    }),
                };
            }
        }
        if self.data_memory != self.data_eeprom {
            if raw_ui.button("Write changes (To EEPROM)").clicked() {
                return match self.save_to_eeprom() {
                    Ok(_) => {
                        if let Ok(new_data) =
                            Self::new(self.meta.id, self.ecu_ref.clone(), self.meta.clone())
                        {
                            *self = new_data;
                        }
                        Some(PageAction::SendNotification {
                            text: format!("Map {} EEPROM save OK!", self.eeprom_key),
                            kind: ToastKind::Success,
                        })
                    }
                    Err(e) => Some(PageAction::SendNotification {
                        text: format!("Map {} EEPROM save failed! {}", self.eeprom_key, e),
                        kind: ToastKind::Error,
                    }),
                };
            }
        }
        if self.data_modify != self.data_program {
            if raw_ui.button("Reset to flash defaults").clicked() {
                self.data_modify = self.data_program.clone();
            }
        }
        None
    }
}

#[derive(Debug, Clone)]
pub struct MapData {
    id: u8,
    name: &'static str,
    x_unit: &'static str,
    y_unit: &'static str,
    x_desc: &'static str,
    y_desc: &'static str,
    v_desc: &'static str,
    value_unit: &'static str,
    x_replace: Option<&'static [&'static str]>,
    y_replace: Option<&'static [&'static str]>,
    show_help: bool,
}

impl MapData {
    pub const fn new(
        id: u8,
        name: &'static str,
        x_unit: &'static str,
        y_unit: &'static str,
        x_desc: &'static str,
        y_desc: &'static str,
        v_desc: &'static str,
        value_unit: &'static str,
        x_replace: Option<&'static [&'static str]>,
        y_replace: Option<&'static [&'static str]>,
    ) -> Self {
        Self {
            id,
            name,
            x_unit,
            y_unit,
            x_desc,
            y_desc,
            v_desc,
            value_unit,
            x_replace,
            y_replace,
            show_help: false,
        }
    }
}

pub struct MapEditor {
    nag: Nag52Diag,
    loaded_maps: HashMap<String, Map>,
    error: Option<String>,
}

impl MapEditor {
    pub fn new(mut nag: Nag52Diag) -> Self {
        nag.with_kwp(|server| server.kwp_set_session(KwpSessionTypeByte::Extended(0x93)));
        Self {
            nag,
            loaded_maps: HashMap::new(),
            error: None,
        }
    }
}

impl super::InterfacePage for MapEditor {
    fn make_ui(
        &mut self,
        ui: &mut eframe::egui::Ui,
        frame: &eframe::Frame,
    ) -> crate::window::PageAction {
        for map in MAP_ARRAY {
            if ui.button(map.name).clicked() {
                self.error = None;
                match Map::new(map.id, self.nag.clone(), map.clone()) {
                    Ok(m) => {
                        // Only if map is not already loaded
                        if !self.loaded_maps.contains_key(&m.eeprom_key) {
                            self.loaded_maps.insert(m.eeprom_key.clone(), m);
                        }
                    }
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
        }

        let mut remove_list: Vec<String> = Vec::new();
        let mut action = None;
        for (key, map) in self.loaded_maps.iter_mut() {
            let mut open = true;
            egui::Window::new(map.meta.name)
                .auto_sized()
                .collapsible(true)
                .open(&mut open)
                .vscroll(false)
                .default_size(vec2(800.0, 400.0))
                .show(ui.ctx(), |window| {
                    action = map.generate_window_ui(window);
                });
            if !open {
                remove_list.push(key.clone())
            }
        }
        for key in remove_list {
            self.loaded_maps.remove(&key);
        }
        if let Some(act) = action {
            return act;
        }
        PageAction::None
    }

    fn get_title(&self) -> &'static str {
        "Map editor"
    }

    fn should_show_statusbar(&self) -> bool {
        true
    }
}
