use std::{fs::File, io::{Read, Write}};

use backend::{
    diag::Nag52Diag,
    ecu_diagnostics::{
        DiagError, DiagServerResult, kwp2000::{KwpCommand, KwpSessionTypeByte},
    },
};
use eframe::{
    egui::{
        self, DragValue, Layout, MenuBar, RichText, ScrollArea
    }, epaint::Color32,
};
use egui_plot::{Bar, BarChart, Line};
use egui_extras::Column;
use plotters::{prelude::{IntoDrawingArea, ChartBuilder}, series::SurfaceSeries};
use serde::Serialize;
mod help_view;
mod map_list;
use crate::{plot_backend::{into_rgba_color, EguiPlotBackend}, ui::map_editor::map_list::MapType, window::PageAction};
use map_list::MAP_ARRAY;
use plotters::prelude::*;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MapViewType {
    EEPROM,
    Default,
    Modify,
}

#[derive(Debug, Clone, Serialize, serde_derive::Deserialize)]
pub struct MapSaveData {
    id: u8,
    x_values: Vec<i16>,
    y_values: Vec<i16>,
    state: Vec<i16>
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MapSelection {
    anchor: (usize, usize),
    cursor: (usize, usize),
}

impl MapSelection {
    fn bounds(&self) -> (usize, usize, usize, usize) {
        (
            self.anchor.0.min(self.cursor.0),
            self.anchor.1.min(self.cursor.1),
            self.anchor.0.max(self.cursor.0),
            self.anchor.1.max(self.cursor.1),
        )
    }

    fn contains(&self, row: usize, col: usize) -> bool {
        let (min_row, min_col, max_row, max_col) = self.bounds();
        (min_row..=max_row).contains(&row) && (min_col..=max_col).contains(&col)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MapShortcutAction {
    AdjustSelection(i16),
    ClearSelection,
    SelectAll,
    WriteToRam,
    WriteToEeprom,
}

#[derive(Debug, Clone, Copy)]
struct MapShortcut {
    shortcut: egui::KeyboardShortcut,
    action: MapShortcutAction,
    description: &'static str,
}

impl MapShortcut {
    const fn new(
        modifiers: egui::Modifiers,
        key: egui::Key,
        action: MapShortcutAction,
        description: &'static str,
    ) -> Self {
        Self {
            shortcut: egui::KeyboardShortcut::new(modifiers, key),
            action,
            description,
        }
    }
}

const ALT_SHIFT: egui::Modifiers = egui::Modifiers::ALT.plus(egui::Modifiers::SHIFT);

const MAP_EDIT_SHORTCUTS: &[MapShortcut] = &[
    MapShortcut::new(
        ALT_SHIFT,
        egui::Key::ArrowUp,
        MapShortcutAction::AdjustSelection(100),
        "Increase selected cells by 100",
    ),
    MapShortcut::new(
        ALT_SHIFT,
        egui::Key::Plus,
        MapShortcutAction::AdjustSelection(100),
        "Increase selected cells by 100",
    ),
    MapShortcut::new(
        ALT_SHIFT,
        egui::Key::Equals,
        MapShortcutAction::AdjustSelection(100),
        "Increase selected cells by 100",
    ),
    MapShortcut::new(
        ALT_SHIFT,
        egui::Key::ArrowDown,
        MapShortcutAction::AdjustSelection(-100),
        "Decrease selected cells by 100",
    ),
    MapShortcut::new(
        ALT_SHIFT,
        egui::Key::Minus,
        MapShortcutAction::AdjustSelection(-100),
        "Decrease selected cells by 100",
    ),
    MapShortcut::new(
        egui::Modifiers::SHIFT,
        egui::Key::ArrowUp,
        MapShortcutAction::AdjustSelection(10),
        "Increase selected cells by 10",
    ),
    MapShortcut::new(
        egui::Modifiers::SHIFT,
        egui::Key::Plus,
        MapShortcutAction::AdjustSelection(10),
        "Increase selected cells by 10",
    ),
    MapShortcut::new(
        egui::Modifiers::SHIFT,
        egui::Key::Equals,
        MapShortcutAction::AdjustSelection(10),
        "Increase selected cells by 10",
    ),
    MapShortcut::new(
        egui::Modifiers::SHIFT,
        egui::Key::ArrowDown,
        MapShortcutAction::AdjustSelection(-10),
        "Decrease selected cells by 10",
    ),
    MapShortcut::new(
        egui::Modifiers::SHIFT,
        egui::Key::Minus,
        MapShortcutAction::AdjustSelection(-10),
        "Decrease selected cells by 10",
    ),
    MapShortcut::new(
        egui::Modifiers::ALT,
        egui::Key::ArrowUp,
        MapShortcutAction::AdjustSelection(1),
        "Increase selected cells by 1",
    ),
    MapShortcut::new(
        egui::Modifiers::ALT,
        egui::Key::Plus,
        MapShortcutAction::AdjustSelection(1),
        "Increase selected cells by 1",
    ),
    MapShortcut::new(
        egui::Modifiers::ALT,
        egui::Key::Equals,
        MapShortcutAction::AdjustSelection(1),
        "Increase selected cells by 1",
    ),
    MapShortcut::new(
        egui::Modifiers::ALT,
        egui::Key::ArrowDown,
        MapShortcutAction::AdjustSelection(-1),
        "Decrease selected cells by 1",
    ),
    MapShortcut::new(
        egui::Modifiers::ALT,
        egui::Key::Minus,
        MapShortcutAction::AdjustSelection(-1),
        "Decrease selected cells by 1",
    ),
    MapShortcut::new(
        egui::Modifiers::NONE,
        egui::Key::Escape,
        MapShortcutAction::ClearSelection,
        "Clear selected cells",
    ),
    MapShortcut::new(
        egui::Modifiers::CTRL,
        egui::Key::A,
        MapShortcutAction::SelectAll,
        "Select all cells in current map",
    ),
    MapShortcut::new(
        egui::Modifiers::NONE,
        egui::Key::F4,
        MapShortcutAction::WriteToRam,
        "Write changes to RAM when available",
    ),
    MapShortcut::new(
        egui::Modifiers::NONE,
        egui::Key::F5,
        MapShortcutAction::WriteToEeprom,
        "Write changes to EEPROM when available",
    ),
];

fn lerp_u8(start: u8, end: u8, factor: f32) -> u8 {
    (start as f32 + ((end as f32 - start as f32) * factor)).round() as u8
}

fn blend_color(start: Color32, end: Color32, factor: f32) -> Color32 {
    let factor = factor.clamp(0.0, 1.0);
    Color32::from_rgb(
        lerp_u8(start.r(), end.r(), factor),
        lerp_u8(start.g(), end.g(), factor),
        lerp_u8(start.b(), end.b(), factor),
    )
}

fn readable_text_color(background: Color32) -> Color32 {
    let luminance = (0.299 * background.r() as f32)
        + (0.587 * background.g() as f32)
        + (0.114 * background.b() as f32);
    if luminance > 140.0 {
        Color32::BLACK
    } else {
        Color32::WHITE
    }
}

fn plot_auto_color(index: usize) -> Color32 {
    let golden_ratio = (5.0_f32.sqrt() - 1.0) / 2.0;
    let hue = index as f32 * golden_ratio;
    egui::epaint::Hsva::new(hue, 0.85, 0.5, 1.0).into()
}

fn select_all_value_text(response: &egui::Response, value: i16) {
    let mut state = egui::TextEdit::load_state(&response.ctx, response.id).unwrap_or_default();
    state
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::two(
            egui::text::CCursor::default(),
            egui::text::CCursor::new(value.to_string().chars().count()),
        )));
    state.store(&response.ctx, response.id);
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
    ecu_ref: Nag52Diag,
    view_type: MapViewType,
    pitch: f64,
    rot: f64,
    selection: Option<MapSelection>,
    selection_dragging: bool,
    editing_cell: Option<(usize, usize)>,
    edit_focus_pending: bool,
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

impl Map {
    pub fn new(map_id: MapType, nag: Nag52Diag, meta: MapData) -> DiagServerResult<Self> {
        // Read metadata

        let ecu_response = nag.with_kwp(|server| {
            server
                .send_byte_array_with_response(&[
                    KwpCommand::ReadDataByLocalIdentifier.into(),
                    0x19,
                    map_id as u8,
                    MapCmd::ReadMeta as u8,
                    0x00,
                    0x00,
                ], None)
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
                    map_id as u8,
                    MapCmd::Read as u8,
                    0x00,
                    0x00,
                ], None)
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
                    map_id as u8,
                    MapCmd::ReadDefault as u8,
                    0x00,
                    0x00,
                ], None)
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
                    map_id as u8,
                    MapCmd::ReadEEPROM as u8,
                    0x00,
                    0x00,
                ], None)
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
            ecu_ref: nag,
            view_type: MapViewType::Modify,
            pitch: 0.8,
            rot: 0.8,
            selection: None,
            selection_dragging: false,
            editing_cell: None,
            edit_focus_pending: false,
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
            self.meta.id as u8,
            MapCmd::Write as u8,
        ];
        payload.extend_from_slice(&self.data_to_byte_array(&self.data_modify));
        self.ecu_ref
            .with_kwp(|server| server.send_byte_array_with_response(&payload, None))?;
        Ok(())
    }

    pub fn save_to_eeprom(&mut self) -> DiagServerResult<()> {
        let payload: Vec<u8> = vec![
            KwpCommand::WriteDataByLocalIdentifier.into(),
            0x19,
            self.meta.id as u8,
            MapCmd::Burn as u8,
            0x00,
            0x00,
        ];
        self.ecu_ref
            .with_kwp(|server| server.send_byte_array_with_response(&payload, None))?;
        Ok(())
    }

    pub fn undo_changes(&mut self) -> DiagServerResult<()> {
        let payload: Vec<u8> = vec![
            KwpCommand::WriteDataByLocalIdentifier.into(),
            0x19,
            self.meta.id as u8,
            MapCmd::Undo as u8,
            0x00,
            0x00,
        ];
        self.ecu_ref
            .with_kwp(|server| server.send_byte_array_with_response(&payload, None))?;
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

    fn set_selection(&mut self, row: usize, col: usize, extend: bool) {
        self.selection = Some(if extend {
            MapSelection {
                anchor: self.selection.map(|s| s.anchor).unwrap_or((row, col)),
                cursor: (row, col),
            }
        } else {
            MapSelection {
                anchor: (row, col),
                cursor: (row, col),
            }
        });
    }

    fn update_selection_cursor(&mut self, row: usize, col: usize) {
        if let Some(selection) = self.selection.as_mut() {
            selection.cursor = (row, col);
        }
    }

    fn move_selection(&mut self, row_delta: isize, col_delta: isize) {
        let Some(selection) = self.selection else {
            return;
        };
        let (min_row, min_col, max_row, max_col) = selection.bounds();

        let row_delta = if row_delta < 0 && min_row == 0 {
            0
        } else if row_delta > 0 && max_row + 1 >= self.y_values.len() {
            0
        } else {
            row_delta
        };
        let col_delta = if col_delta < 0 && min_col == 0 {
            0
        } else if col_delta > 0 && max_col + 1 >= self.x_values.len() {
            0
        } else {
            col_delta
        };

        if row_delta == 0 && col_delta == 0 {
            return;
        }

        let move_point = |(row, col): (usize, usize)| {
            (
                row.saturating_add_signed(row_delta),
                col.saturating_add_signed(col_delta),
            )
        };
        self.selection = Some(MapSelection {
            anchor: move_point(selection.anchor),
            cursor: move_point(selection.cursor),
        });
        self.editing_cell = None;
        self.edit_focus_pending = false;
    }

    fn edit_selection_start(&mut self) {
        let Some(selection) = self.selection else {
            return;
        };
        let (row, col, _, _) = selection.bounds();
        self.editing_cell = Some((row, col));
        self.edit_focus_pending = true;
    }

    fn clear_selection(&mut self) {
        self.selection = None;
        self.selection_dragging = false;
        self.editing_cell = None;
        self.edit_focus_pending = false;
    }

    fn select_all_cells(&mut self) {
        if self.y_values.is_empty() || self.x_values.is_empty() {
            return;
        }
        self.selection = Some(MapSelection {
            anchor: (0, 0),
            cursor: (self.y_values.len() - 1, self.x_values.len() - 1),
        });
        self.selection_dragging = false;
        self.editing_cell = None;
        self.edit_focus_pending = false;
    }

    fn selected_map_indices(&self) -> Vec<usize> {
        let Some(selection) = self.selection else {
            return Vec::new();
        };
        let (min_row, min_col, max_row, max_col) = selection.bounds();
        let x_len = self.x_values.len();
        let mut result = Vec::new();
        for row in min_row..=max_row {
            for col in min_col..=max_col {
                result.push((row * x_len) + col);
            }
        }
        result
    }

    fn apply_selection_delta(&mut self, delta: i16) {
        for idx in self.selected_map_indices() {
            self.data_modify[idx] = self.data_modify[idx].saturating_add(delta);
        }
    }

    fn handle_selection_navigation(&mut self, ui: &mut egui::Ui) -> bool {
        if self.editing_cell.is_some() {
            return false;
        }

        let action = ui.input_mut(|input| {
            if input.modifiers != egui::Modifiers::NONE {
                return None;
            }
            if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                Some((-1, 0))
            } else if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                Some((1, 0))
            } else if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft) {
                Some((0, -1))
            } else if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight) {
                Some((0, 1))
            } else if input.consume_key(egui::Modifiers::NONE, egui::Key::Enter) {
                Some((0, 0))
            } else {
                None
            }
        });

        match action {
            Some((0, 0)) => {
                self.edit_selection_start();
                true
            }
            Some((row_delta, col_delta)) => {
                self.move_selection(row_delta, col_delta);
                true
            }
            None => false,
        }
    }

    fn can_write_to_ram(&self) -> bool {
        self.data_modify != self.data_eeprom
    }

    fn can_write_to_eeprom(&self) -> bool {
        self.data_memory != self.data_eeprom
    }

    fn write_changes_to_ram(&mut self) -> PageAction {
        match self.write_to_ram() {
            Ok(_) => {
                self.data_memory = self.data_modify.clone();
                PageAction::SendNotification {
                    text: format!("Map {} RAM write OK!", self.eeprom_key),
                    kind: egui_notify::ToastLevel::Success,
                }
            }
            Err(e) => PageAction::SendNotification {
                text: format!("Map {} RAM write failed! {}", self.eeprom_key, e),
                kind: egui_notify::ToastLevel::Error,
            },
        }
    }

    fn write_changes_to_eeprom(&mut self) -> PageAction {
        match self.save_to_eeprom() {
            Ok(_) => {
                if let Ok(new_data) =
                    Self::new(self.meta.id, self.ecu_ref.clone(), self.meta.clone())
                {
                    *self = new_data;
                }
                PageAction::SendNotification {
                    text: format!("Map {} EEPROM save OK!", self.eeprom_key),
                    kind: egui_notify::ToastLevel::Success,
                }
            }
            Err(e) => PageAction::SendNotification {
                text: format!("Map {} EEPROM save failed! {}", self.eeprom_key, e),
                kind: egui_notify::ToastLevel::Error,
            },
        }
    }

    fn handle_write_shortcuts(&mut self, ui: &mut egui::Ui) -> Option<PageAction> {
        if ui.memory(|mem| mem.top_modal_layer().is_some() || mem.focused().is_some()) {
            return None;
        }

        let action = ui.input_mut(|input| {
            MAP_EDIT_SHORTCUTS
                .iter()
                .find(|shortcut| {
                    matches!(
                        shortcut.action,
                        MapShortcutAction::WriteToRam | MapShortcutAction::WriteToEeprom
                    ) && input.consume_shortcut(&shortcut.shortcut)
                })
                .map(|shortcut| shortcut.action)
        });

        match action {
            Some(MapShortcutAction::WriteToRam) if self.can_write_to_ram() => {
                Some(self.write_changes_to_ram())
            }
            Some(MapShortcutAction::WriteToEeprom) if self.can_write_to_eeprom() => {
                Some(self.write_changes_to_eeprom())
            }
            _ => None,
        }
    }

    fn handle_edit_shortcuts(&mut self, ui: &mut egui::Ui) {
        if ui.memory(|mem| mem.top_modal_layer().is_some()) {
            return;
        }
        let has_selection_state = self.selection.is_some() || self.editing_cell.is_some();
        let clear_shortcut = MAP_EDIT_SHORTCUTS
            .iter()
            .find(|shortcut| shortcut.action == MapShortcutAction::ClearSelection)
            .map(|shortcut| shortcut.shortcut)
            .expect("map editor clear shortcut must be registered");
        if has_selection_state && ui.input_mut(|input| input.consume_shortcut(&clear_shortcut)) {
            self.clear_selection();
            return;
        }
        if self.view_type != MapViewType::Modify || self.selection.is_none() {
            return;
        }
        if ui.memory(|mem| mem.focused().is_some()) {
            return;
        }

        if self.handle_selection_navigation(ui) {
            return;
        }

        let action = ui.input_mut(|input| {
            MAP_EDIT_SHORTCUTS
                .iter()
                .find(|shortcut| {
                    matches!(
                        shortcut.action,
                        MapShortcutAction::AdjustSelection(_) | MapShortcutAction::SelectAll
                    ) && input.consume_shortcut(&shortcut.shortcut)
                })
                .map(|shortcut| shortcut.action)
        });

        match action {
            Some(MapShortcutAction::AdjustSelection(delta)) => self.apply_selection_delta(delta),
            Some(MapShortcutAction::SelectAll) => self.select_all_cells(),
            _ => {}
        }
    }

    fn gen_edit_table(&mut self, raw_ui: &mut egui::Ui) {
        let table_id = (self.meta.id as u8, self.view_type);
        let header_color = raw_ui.visuals().warn_fg_color;
        let cell_edit_color = raw_ui.visuals().error_fg_color;
        let max_delta = self
            .data_modify
            .iter()
            .zip(self.data_eeprom.iter())
            .map(|(modified, eeprom)| (*modified as i32 - *eeprom as i32).abs())
            .max()
            .unwrap_or(0);
        let value_font_id = egui::TextStyle::Button.resolve(raw_ui.style());
        let value_cell_width = self
            .data_modify
            .iter()
            .chain(self.data_eeprom.iter())
            .chain(self.data_program.iter())
            .map(|value| format!("{}{}", value, self.meta.value_unit))
            .chain(
                self.data_memory
                    .iter()
                    .zip(self.data_eeprom.iter())
                    .map(|(ram, eeprom)| {
                        format!("{:+}{}", *ram as i32 - *eeprom as i32, self.meta.value_unit)
                    }),
            )
            .map(|text| {
                raw_ui
                    .painter()
                    .layout_no_wrap(text, value_font_id.clone(), Color32::WHITE)
                    .size()
                    .x
            })
            .fold(0.0_f32, f32::max)
            + (raw_ui.spacing().button_padding.x * 2.0)
            + 8.0;
        let value_cell_width = value_cell_width
            .max(raw_ui.spacing().interact_size.x)
            .ceil();
        if self.meta.reset_adaptation {
            raw_ui.strong("Warning. Modifying this map resets adaptation!");
        }
        if self.view_type != MapViewType::Modify {
            self.clear_selection();
        }
        if !raw_ui.input(|input| input.pointer.primary_down()) {
            self.selection_dragging = false;
        }
        let show_ram_eeprom_delta = self.view_type == MapViewType::Modify
            && self.editing_cell.is_none()
            && raw_ui.memory(|mem| mem.focused().is_none())
            && raw_ui.input(|input| input.modifiers.ctrl && input.key_down(egui::Key::D));
        if let Some(h) = self.meta.help {
            raw_ui.label(h);
        }
        if !self.meta.x_desc.is_empty() {
            raw_ui.label(format!("X: {}", self.meta.x_desc));
        }
        if !self.meta.y_desc.is_empty() {
            raw_ui.label(format!("Y: {}", self.meta.y_desc));
        }
        if !self.meta.v_desc.is_empty() {
            raw_ui.label(format!("Values: {}", self.meta.v_desc));
        }
        let mut pointer_over_cell = false;
        raw_ui.push_id(table_id, |ui| {
            let mut table_builder = egui_extras::TableBuilder::new(ui)
                .striped(true)
                .cell_layout(
                    Layout::left_to_right(egui::Align::Center)
                        .with_cross_align(egui::Align::Center),
                )
                .column(Column::initial(60.0).at_least(60.0));
            for _ in 0..self.x_values.len() {
                table_builder = table_builder.column(Column::auto().at_least(value_cell_width));
            }
            table_builder
                .header(15.0, |mut header| {
                    header.col(|_| {}); // Nothing in corner cell
                    if self.x_values.len() == 1 {
                        header.col(|_| {});
                    } else {
                        for v in 0..self.x_values.len() {
                            header.col(|u| {
                                u.label(
                                    RichText::new(format!("{}", self.get_x_label(v)))
                                        .color(header_color),
                                );
                            });
                        }
                    }
                })
                .body(|body| {
                    body.rows(15.0, self.y_values.len(), |mut row| {
                        let row_id = row.index();
                        // Header column
                        row.col(|c| {
                            let row_color = plot_auto_color(row_id);
                            egui::Frame::new()
                                .fill(row_color)
                                .corner_radius(egui::CornerRadius::same(2))
                                .inner_margin(egui::Margin::symmetric(4, 0))
                                .show(c, |c| {
                                    c.label(
                                        RichText::new(format!("{}", self.get_y_label(row_id)))
                                            .color(readable_text_color(row_color)),
                                    );
                                });
                        });

                        // Data columns
                        for x_pos in 0..self.x_values.len() {
                            row.col(|cell| match self.view_type {
                                MapViewType::EEPROM => {
                                    cell.label(format!(
                                        "{}",
                                        self.data_eeprom[(row_id * self.x_values.len()) + x_pos]
                                    ));
                                }
                                MapViewType::Default => {
                                    cell.label(format!(
                                        "{}",
                                        self.data_program[(row_id * self.x_values.len()) + x_pos]
                                    ));
                                }
                                MapViewType::Modify => {
                                    let cell_rect = cell.max_rect();
                                    let map_idx = (row_id * self.x_values.len()) + x_pos;
                                    let modified_value = self.data_modify[map_idx];
                                    let eeprom_value = self.data_eeprom[map_idx];
                                    let delta = modified_value as i32 - eeprom_value as i32;
                                    if delta != 0 {
                                        cell.style_mut().visuals.override_text_color = Some(cell_edit_color)
                                    }
                                    let selected = self.selection
                                        .map(|selection| selection.contains(row_id, x_pos))
                                        .unwrap_or(false);
                                    let is_editing = self.editing_cell == Some((row_id, x_pos));
                                    let response = if is_editing {
                                        let edit = DragValue::new(&mut self.data_modify[map_idx])
                                            .suffix(self.meta.value_unit)
                                            .update_while_editing(false)
                                            .speed(0);
                                        let mut response = cell
                                            .with_layout(
                                                Layout::right_to_left(egui::Align::Center),
                                                |cell| {
                                                    cell.spacing_mut().interact_size.x =
                                                        value_cell_width;
                                                    cell.add(edit)
                                                },
                                            )
                                            .inner;
                                        if delta != 0 {
                                            response = response.on_hover_text(format!(
                                                "EEPROM: {}{}\nCurrent: {}{}\nDelta: {:+}{}",
                                                eeprom_value,
                                                self.meta.value_unit,
                                                modified_value,
                                                self.meta.value_unit,
                                                delta,
                                                self.meta.value_unit,
                                            ));
                                        }
                                        if self.edit_focus_pending {
                                            response.request_focus();
                                            select_all_value_text(
                                                &response,
                                                self.data_modify[map_idx],
                                            );
                                            self.edit_focus_pending = false;
                                        } else if response.gained_focus() {
                                            select_all_value_text(
                                                &response,
                                                self.data_modify[map_idx],
                                            );
                                        }
                                        response
                                    } else {
                                        let mut button_fill = None;
                                        let mut text_color = None;
                                        if delta != 0 && max_delta > 0 {
                                            let intensity = (delta.abs() as f32 / max_delta as f32)
                                                .clamp(0.25, 1.0);
                                            let base_fill = cell.visuals().widgets.inactive.bg_fill;
                                            let delta_fill = if delta > 0 {
                                                blend_color(
                                                    base_fill,
                                                    Color32::from_rgb(34, 197, 94),
                                                    intensity,
                                                )
                                            } else {
                                                blend_color(
                                                    base_fill,
                                                    Color32::from_rgb(239, 68, 68),
                                                    intensity,
                                                )
                                            };
                                            text_color = Some(readable_text_color(delta_fill));
                                            button_fill = Some(delta_fill);
                                        }
                                        if selected {
                                            let visuals = cell.visuals().selection;
                                            text_color = Some(readable_text_color(visuals.bg_fill));
                                            button_fill = Some(visuals.bg_fill);
                                        }
                                        let display_value = if show_ram_eeprom_delta {
                                            format!(
                                                "{:+}{}",
                                                self.data_memory[map_idx] as i32
                                                    - self.data_eeprom[map_idx] as i32,
                                                self.meta.value_unit
                                            )
                                        } else {
                                            format!(
                                                "{}{}",
                                                self.data_modify[map_idx],
                                                self.meta.value_unit
                                            )
                                        };
                                        let mut text = RichText::new(display_value);
                                        if let Some(text_color) = text_color {
                                            text = text.color(text_color);
                                        }
                                        let mut button = egui::Button::new(())
                                            .right_text(text)
                                            .sense(egui::Sense::click_and_drag())
                                            .min_size(egui::vec2(
                                                value_cell_width,
                                                cell.spacing().interact_size.y,
                                            ));
                                        if let Some(fill) = button_fill {
                                            button = button.fill(fill);
                                        }
                                        if selected {
                                            let visuals = cell.visuals().selection;
                                            button = button.stroke(visuals.stroke);
                                        }
                                        cell.add(button)
                                    };
                                    let pointer_over_response = response
                                        .ctx
                                        .input(|input| input.pointer.interact_pos())
                                        .map(|pos| cell_rect.contains(pos))
                                        .unwrap_or(false);
                                    pointer_over_cell |= response.hovered() || pointer_over_response;
                                    if selected {
                                        let visuals = cell.visuals().selection;
                                        cell.painter().rect_stroke(
                                            response.rect.expand(1.0),
                                            egui::CornerRadius::same(2),
                                            egui::Stroke::new(1.0, visuals.stroke.color),
                                            egui::StrokeKind::Outside,
                                        );
                                    }
                                    if is_editing {
                                        let (enter_pressed, escape_pressed) =
                                            response.ctx.input(|input| {
                                                (
                                                    input.key_pressed(egui::Key::Enter),
                                                    input.key_pressed(egui::Key::Escape),
                                                )
                                            });
                                        if enter_pressed {
                                            response.surrender_focus();
                                            response.ctx.input_mut(|input| {
                                                input.consume_key(
                                                    egui::Modifiers::NONE,
                                                    egui::Key::Enter,
                                                );
                                            });
                                        }
                                        if response.lost_focus() || escape_pressed {
                                            self.editing_cell = None;
                                            self.edit_focus_pending = false;
                                        }
                                    }
                                    if response.double_clicked() {
                                        self.set_selection(row_id, x_pos, false);
                                        self.editing_cell = Some((row_id, x_pos));
                                        self.edit_focus_pending = true;
                                    } else if response.clicked() {
                                        let extend = response.ctx.input(|input| input.modifiers.shift);
                                        let enter_activated = response.has_focus()
                                            && response.ctx.input(|input| {
                                                input.key_pressed(egui::Key::Enter)
                                            });
                                        if enter_activated && selected {
                                            self.editing_cell = Some((row_id, x_pos));
                                            self.edit_focus_pending = true;
                                        } else {
                                            self.set_selection(row_id, x_pos, extend);
                                            self.editing_cell = None;
                                            self.edit_focus_pending = false;
                                        }
                                    }
                                    if response.drag_started() {
                                        self.set_selection(row_id, x_pos, false);
                                        self.selection_dragging = true;
                                        self.editing_cell = None;
                                        self.edit_focus_pending = false;
                                    }
                                    if self.selection_dragging
                                        && pointer_over_response
                                        && response.ctx.input(|input| input.pointer.primary_down())
                                    {
                                        self.update_selection_cursor(row_id, x_pos);
                                    }
                                }
                            });
                        }
                    })
                });
        });
        if raw_ui.input(|input| input.pointer.primary_clicked()) && !pointer_over_cell {
            self.clear_selection();
        }
        self.handle_edit_shortcuts(raw_ui);
    }

    fn generate_window_ui(&mut self, raw_ui: &mut egui::Ui) -> Option<PageAction> {
        let mut action = None;
        raw_ui.horizontal(|ui| {
            if ui.button("Load from file").clicked() {
                let mut copy = self.clone();
                if let Some(res) = load_map(&mut copy) {
                    match res {
                        Ok(_) => {
                            *self = copy;
                            action = Some(PageAction::SendNotification { 
                                text: format!("Map loading OK!"), 
                                kind: egui_notify::ToastLevel::Success 
                            });
                        },
                        Err(e) => {
                            action = Some(PageAction::SendNotification { 
                                text: format!("Map loading failed: {e}"), 
                                kind: egui_notify::ToastLevel::Error 
                            });
                        },
                    }
                }
            }
            if ui.button("Save to file").clicked() {
                if self.data_eeprom != self.data_modify || self.data_memory != self.data_eeprom {
                    action = Some(PageAction::SendNotification { 
                        text: "You have unsaved data in the map. Please write to EEPROM before saving".into(), 
                        kind: egui_notify::ToastLevel::Warning 
                    });
                } else {
                    save_map(&self);
                }
            }
        });
        raw_ui.horizontal(|row| {
            row.strong("Viewing:");
            row.selectable_value(&mut self.view_type, MapViewType::Modify, "User changes");
            row.selectable_value(&mut self.view_type, MapViewType::EEPROM, "EEPROM");
            row.selectable_value(&mut self.view_type, MapViewType::Default, "TCU default");
        });
        raw_ui.horizontal(|raw_ui| {
            raw_ui.add_enabled_ui(self.data_modify != self.data_program, |ui| {
                if ui.button("Reset to flash defaults").clicked() {
                    self.data_modify = self.data_program.clone();
                }
            });
            raw_ui.add_enabled_ui(self.can_write_to_ram(), |ui| {
                if ui.button("Undo user changes").clicked() {
                    action = match self.undo_changes() {
                        Ok(_) => {
                            self.data_modify = self.data_eeprom.clone();
                            Some(PageAction::SendNotification {
                                text: format!("Map {} undo OK!", self.eeprom_key),
                                kind: egui_notify::ToastLevel::Success,
                            })
                        }
                        Err(e) => Some(PageAction::SendNotification {
                            text: format!("Map {} undo failed! {}", self.eeprom_key, e),
                            kind: egui_notify::ToastLevel::Error,
                        }),
                    };
                }
                if ui.button("Write changes (To RAM)").clicked() {
                    action = Some(self.write_changes_to_ram());
                }
            });
            raw_ui.add_enabled_ui(self.can_write_to_eeprom(), |ui| {
                if ui.button("Write changes (To EEPROM)").clicked() {
                    action = Some(self.write_changes_to_eeprom());
                }
            });
        });
        if action.is_none() {
            action = self.handle_write_shortcuts(raw_ui);
        }
        self.gen_edit_table(raw_ui);
        ScrollArea::new([true, true])
            .max_height(raw_ui.available_height())
            .show(raw_ui, |raw_ui| {
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
                egui_plot::Plot::new(format!("PLOT-{}", self.eeprom_key))
                    .allow_drag(false)
                    .allow_scroll(false)
                    .allow_zoom(false)
                    .width(raw_ui.available_width())
                    .include_x(0)
                    .include_y((self.y_values.len() + 1) as f64 * 1.5)
                    .show(raw_ui, |plot_ui| plot_ui.bar_chart(BarChart::new("", bars)));
            } else if self.meta.x_replace.is_some() || self.meta.y_replace.is_some() {
                // Line chart
                let mut lines: Vec<Line> = Vec::new();
                for (y_idx, _key) in self.y_values.iter().enumerate() {
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
                    lines.push(Line::new(self.get_y_label(y_idx), points).color(plot_auto_color(y_idx)));
                }
                egui_plot::Plot::new(format!("PLOT-{}", self.eeprom_key))
                    .allow_drag(false)
                    .allow_scroll(false)
                    .allow_zoom(false)
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
                let desired_size = egui::Vec2::new(raw_ui.available_width(), raw_ui.available_height());
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
                let _ = area.fill(&into_rgba_color(vis.extreme_bg_color));
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
                let _ = area.present();
            };
        });
        action
    }

    pub fn current_viewed_data(&self) -> &[i16] {
        match self.view_type {
            MapViewType::EEPROM => &self.data_eeprom,
            MapViewType::Default => &self.data_memory,
            MapViewType::Modify => &self.data_modify,
        }
    }
}

pub fn save_map(map: &Map) {
    let save_data = MapSaveData {
        id: map.meta.id as u8,
        x_values: map.x_values.clone(),
        y_values: map.y_values.clone(),
        state: map.data_eeprom.clone(),
    };
    if let Some(picked) = rfd::FileDialog::new().set_title(format!("Save map {}", map.meta.name)).set_file_name(format!("map_{}.mapbin", map.eeprom_key)).save_file() {
        let bin = bincode::serde::encode_to_vec(&save_data, bincode::config::legacy()).unwrap();
        let mut f = File::create(picked).unwrap();
        let _ = f.write_all(&bin);  
    }
}

pub fn load_map(map: &mut Map) -> Option<Result<(), String>> {
    let path = rfd::FileDialog::new().add_filter("mapbin", &["mapbin"]).set_title(format!("Pick map file for {}", map.meta.name)).pick_file()?;
    let mut f = File::open(path).unwrap();
    let mut contents = Vec::new();
    f.read_to_end(&mut contents).unwrap();
    let save_data = bincode::serde::decode_from_slice::<MapSaveData, _>(&contents, bincode::config::legacy()).map_err(|e| e.to_string());
    match save_data {
        Ok((data, _)) => {
            if data.id != map.meta.id as u8 {
                return Some(Err(format!("Map key is different. Expected {}, got {}", map.meta.id as u8, data.id)));
            }
            if data.x_values != map.x_values {
                return Some(Err(format!("X sizes differ! Map spec has changed. Saved map is no longer valid")));
            }
            if data.y_values != map.y_values {
                return Some(Err(format!("Y sizes differ! Map spec has changed. Saved map is no longer valid")));
            }
            if data.state.len() != map.data_eeprom.len() {
                return Some(Err(format!("Z sizes differ! Map spec has changed. Saved map is no longer valid")));
            }
            // All OK!
            map.data_modify = data.state;
            return Some(Ok(()))
        },
        Err(e) => {
            return Some(Err(e))
        }
    }
}

#[derive(Debug, Clone)]
pub struct MapData {
    id: MapType,
    name: &'static str,
    x_unit: &'static str,
    y_unit: &'static str,
    x_desc: &'static str,
    y_desc: &'static str,
    v_desc: &'static str,
    value_unit: &'static str,
    x_replace: Option<&'static [&'static str]>,
    y_replace: Option<&'static [&'static str]>,
    help: Option<&'static str>,
    reset_adaptation: bool
}

impl MapData {
    pub const fn new(
        id: MapType,
        name: &'static str,
        x_unit: &'static str,
        y_unit: &'static str,
        x_desc: &'static str,
        y_desc: &'static str,
        v_desc: &'static str,
        value_unit: &'static str,
        x_replace: Option<&'static [&'static str]>,
        y_replace: Option<&'static [&'static str]>,
        reset_adaptation: bool,
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
            help: None,
            reset_adaptation
        }
    }

    pub const fn with_help(mut self, s: &'static str) -> Self {
        self.help = Some(s);
        self
    }
}

pub struct MapEditor {
    nag: Nag52Diag,
    loaded_map: Option<Map>,
    error: Option<String>,
    show_shortcuts: bool,
}

impl MapEditor {
    pub fn new(nag: Nag52Diag) -> Self {
        let _ = nag.with_kwp(|server| server.kwp_set_session(KwpSessionTypeByte::Extended(0x93)));
        Self {
            nag,
            loaded_map: None,
            error: None,
            show_shortcuts: false,
        }
    }

    fn show_shortcuts_modal(&mut self, ctx: &egui::Context) {
        if !self.show_shortcuts {
            return;
        }

        let mut close_requested = false;
        let response = egui::Modal::new(egui::Id::new("map_editor_shortcuts_modal")).show(
            ctx,
            |ui| {
                ui.set_min_width(420.0);
                ui.heading("Map tuner shortcuts");
                ui.separator();
                egui::Grid::new("map_editor_shortcuts_grid")
                    .num_columns(2)
                    .striped(true)
                    .spacing([16.0, 6.0])
                    .show(ui, |ui| {
                        ui.strong("Shortcut");
                        ui.strong("Action");
                        ui.end_row();
                        for shortcut in MAP_EDIT_SHORTCUTS {
                            ui.label(ctx.format_shortcut(&shortcut.shortcut));
                            ui.label(shortcut.description);
                            ui.end_row();
                        }
                    });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Close").clicked() {
                            close_requested = true;
                        }
                    });
                });
            },
        );
        if response.should_close() || close_requested {
            self.show_shortcuts = false;
        }
    }
}


impl super::InterfacePage for MapEditor {
    fn make_ui(
        &mut self,
        ui: &mut eframe::egui::Ui,
    ) -> crate::window::PageAction {
        let mut action = None;
        let mut map_to_switch = None;
        MenuBar::new()
        .ui(ui, |ui| {
            ui.menu_button("Select map", |ui| {
                ui.menu_button("Shift points", |ui| {
                    ui.label("(S)tandard mode");
                    if ui.button("Upshift").clicked() {
                        map_to_switch = Some(MapType::UpshiftS);
                    }
                    if ui.button("Downshift").clicked() {
                        map_to_switch = Some(MapType::DnshiftS);
                    }
                    ui.separator();
                    ui.label("(C)omfort mode");
                    if ui.button("Upshift").clicked() {
                        map_to_switch = Some(MapType::UpshiftC);
                    }
                    if ui.button("Downshift").clicked() {
                        map_to_switch = Some(MapType::DnshiftC);
                    }
                    ui.separator();
                    ui.label("(A)gility mode");
                    if ui.button("Upshift").clicked() {
                        map_to_switch = Some(MapType::UpshiftA);
                    }
                    if ui.button("Downshift").clicked() {
                        map_to_switch = Some(MapType::DnshiftA);
                    }
                    
                });
                ui.menu_button("Shift speed", |ui| {
                    ui.label("(S)tandard mode");
                    if ui.button("Upshift").clicked() {
                        map_to_switch = Some(MapType::UpshiftOverlapS);
                    }
                    if ui.button("Downshift").clicked() {
                        map_to_switch = Some(MapType::DnshiftOverlapS);
                    }
                    ui.separator();
                    ui.label("(C)omfort mode");
                    if ui.button("Upshift").clicked() {
                        map_to_switch = Some(MapType::UpshiftOverlapC);
                    }
                    if ui.button("Downshift").clicked() {
                        map_to_switch = Some(MapType::DnshiftOverlapC);
                    }
                    ui.separator();
                    ui.label("(A)gility mode");
                    if ui.button("Upshift").clicked() {
                        map_to_switch = Some(MapType::UpshiftOverlapA);
                    }
                    if ui.button("Downshift").clicked() {
                        map_to_switch = Some(MapType::DnshiftOverlapA);
                    }
                    ui.separator();
                    ui.label("(M)anual mode");
                    if ui.button("Upshift").clicked() {
                        map_to_switch = Some(MapType::UpshiftOverlapM);
                    }
                    if ui.button("Downshift").clicked() {
                        map_to_switch = Some(MapType::DnshiftOverlapM);
                    }
                });
                ui.menu_button("Clutch filling", |ui| {
                    if ui.button("Stage 1 (High) filling pressure").clicked() {
                        map_to_switch = Some(MapType::FillPressure);
                    }
                    if ui.button("Stage 1 (High) filling time").clicked() {
                        map_to_switch = Some(MapType::FillTime);
                    }
                    if ui.button("Stage 2 (Low) filling pressure").clicked() {
                        map_to_switch = Some(MapType::LowFillPressure);
                    }
                });
                ui.menu_button("Torque converter", |ui| {
                    ui.label("Zone pressures (Adaptable)");
                    if ui.button("Slipping pressure").clicked() {
                        map_to_switch = Some(MapType::TccAdaptSlipMap);
                    }
                    if ui.button("Locking pressure").clicked() {
                        map_to_switch = Some(MapType::TccAdaptLockMap);
                    }
                    ui.separator();
                    ui.label("Target slip map");
                    if ui.button("Slip target vs load").clicked() {
                        map_to_switch = Some(MapType::TccRpmSlipMap);
                    }
                    ui.separator();
                    ui.label("Solenoid data");
                    if ui.button("Solenoid PWM").clicked() {
                        map_to_switch = Some(MapType::TccPwm);
                    }
                });
            });
            if ui.button("Keyboard shortcuts").clicked() {
                self.show_shortcuts = true;
            }
        });
        self.show_shortcuts_modal(ui.ctx());
        if let Some(selected) = map_to_switch {
            // Stop user changing maps if they have unsaved changes
            let mut allowed_to_swtich = true;
            if let Some(current_map) = self.loaded_map.as_ref() {
                if current_map.data_modify != current_map.data_eeprom {
                    allowed_to_swtich = false;
                }
            }
            if !allowed_to_swtich {
                action = Some(PageAction::SendNotification { 
                    text: "You have uncommited changes, please reset or write to EEPROM".into(), 
                    kind: egui_notify::ToastLevel::Warning 
                })
            } else {
                if let Some(found_map_info) = MAP_ARRAY.iter().find(|x| x.id == selected) {
                    self.error = None;
                    match Map::new(selected, self.nag.clone(), found_map_info.clone()) {
                        Ok(m) => {
                            self.loaded_map = Some(m)
                        }
                        Err(e) => self.error = Some(e.to_string()),
                    }
                } else {
                    //Error toast
                    action = Some(PageAction::SendNotification { 
                        text: format!("Failed to find map {:?} (0x{:02X}). This is a bug!", selected, selected as u8), 
                        kind: egui_notify::ToastLevel::Error 
                    })
                }
            }
        }
        ui.separator();
        if let Some(loaded_map) = self.loaded_map.as_mut() {
            if let Some(err) = &self.error {
                ui.centered_and_justified(|ui| ui.colored_label(Color32::RED, format!("Map failed to load: {err}")));
            } else {
                if action.is_none() {
                    action = loaded_map.generate_window_ui(ui);
                }
            }
        } else {
            ui.centered_and_justified(|ui| ui.strong("Please select a map"));
        }
        if let Some(act) = action {
            act
        } else {
            PageAction::None
        }
    }

    fn should_show_statusbar(&self) -> bool {
        true
    }
}
