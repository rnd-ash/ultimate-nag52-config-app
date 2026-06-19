use std::{
    fs::File,
    io::{Read, Write},
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{Duration, Instant},
};

use backend::{
    diag::Nag52Diag,
    ecu_diagnostics::{
        dynamic_diag::DynamicDiagSession,
        kwp2000::{KwpCommand, KwpSessionTypeByte},
        DiagError, DiagServerResult,
    },
};
use eframe::{
    egui::{self, DragValue, Layout, MenuBar, RichText, ScrollArea},
    epaint::Color32,
};
use egui_extras::Column;
use egui_plot::{Bar, BarChart, GridMark, Line, MarkerShape, Points, VLine};
use plotters::{
    prelude::{ChartBuilder, IntoDrawingArea},
    series::SurfaceSeries,
};
use serde::Serialize;
mod help_view;
mod map_list;
use crate::{
    plot_backend::{into_rgba_color, EguiPlotBackend},
    ui::map_editor::map_list::MapType,
    window::PageAction,
};
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
    GetLookupVals = 0x10,
}

const LOOKUP_CACHE_DEFAULT_POLL_HZ: f32 = 24.0;
const LOOKUP_CACHE_MIN_POLL_HZ: f32 = 0.1;
const LOOKUP_CACHE_MAX_POLL_HZ: f32 = 60.0;
const LOOKUP_CACHE_BACKOFF_INTERVAL: Duration = Duration::from_millis(1000);
// The diagnostic server read timeout is 10s; keep the UI timeout just above it
// so we do not report a visual timeout while the request is still legitimately
// waiting inside the diagnostics layer.
const LOOKUP_CACHE_REQUEST_TIMEOUT: Duration = Duration::from_millis(11000);
const LOOKUP_CACHE_BACKOFF_AFTER_ERRORS: u8 = 3;
const LOOKUP_CACHE_DISABLE_AFTER_ERRORS: u8 = 5;
const LOOKUP_CACHE_ENTRY_SIZE: u8 = 13;
const LOOKUP_CACHE_MAX_SLOTS: u8 = 5;
const LOOKUP_TRACE_FADE_MS: u32 = 2000;
const LOOKUP_TRACE_LINE_WIDTH: f32 = 4.0;
const LOOKUP_TRACE_3D_LINE_WIDTH: u32 = 4;
const LOOKUP_CURSOR_VLINE_WIDTH: f32 = 2.0;
const LOOKUP_CURSOR_CROSS_RADIUS: f32 = 14.0;
const LOOKUP_LAMP_POLL_MS: u128 = 160;
const LOOKUP_LAMP_DATA_MS: u128 = 300;
const LOOKUP_LAMP_ERROR_MS: u128 = 3000;
const RLI_TCU_TIME: u8 = 0x26;
const KWP_POSITIVE_READ_DATA_BY_LOCAL_IDENTIFIER: u8 = 0x61;
const KWP_NRC_SUB_FUNC_NOT_SUPPORTED_INVALID_FORMAT: u8 = 0x12;
const KWP_NRC_REQUEST_OUT_OF_RANGE: u8 = 0x31;
const MAP_EDITOR_ROW_HEADER_WIDTH: f32 = 60.0;

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
    state: Vec<i16>,
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
enum MapControlAction {
    AdjustSelection(i16),
    ClearSelection,
    SelectAll,
    UndoEdit,
    RedoEdit,
    LoadFromFile,
    SaveToFile,
    WriteToRam,
    WriteToEeprom,
    ShowRamEepromDelta,
}

#[derive(Debug, Clone, Copy)]
enum MapControlBinding {
    Keyboard(egui::KeyboardShortcut),
    Hold(egui::KeyboardShortcut),
    Mouse(&'static str),
}

#[derive(Debug, Clone, Copy)]
struct MapControlEntry {
    binding: MapControlBinding,
    action: Option<MapControlAction>,
    description: &'static str,
}

#[derive(Debug, Clone, Default)]
struct LineChartAlignmentState {
    column_count: usize,
    table_data_rect: Option<egui::Rect>,
    plot_frame_rect: Option<egui::Rect>,
    outer_left_margin: Option<f32>,
    plot_total_width: Option<f32>,
}

const ALT_SHIFT: egui::Modifiers = egui::Modifiers::ALT.plus(egui::Modifiers::SHIFT);
const MAP_EDIT_HISTORY_LIMIT: usize = 100;

const MAP_CONTROL_ENTRIES: &[MapControlEntry] = &[
    MapControlEntry {
        binding: MapControlBinding::Keyboard(egui::KeyboardShortcut::new(
            ALT_SHIFT,
            egui::Key::ArrowUp,
        )),
        action: Some(MapControlAction::AdjustSelection(100)),
        description: "Increase selected cells by 100",
    },
    MapControlEntry {
        binding: MapControlBinding::Keyboard(egui::KeyboardShortcut::new(
            ALT_SHIFT,
            egui::Key::Plus,
        )),
        action: Some(MapControlAction::AdjustSelection(100)),
        description: "Increase selected cells by 100",
    },
    MapControlEntry {
        binding: MapControlBinding::Keyboard(egui::KeyboardShortcut::new(
            ALT_SHIFT,
            egui::Key::Equals,
        )),
        action: Some(MapControlAction::AdjustSelection(100)),
        description: "Increase selected cells by 100",
    },
    MapControlEntry {
        binding: MapControlBinding::Keyboard(egui::KeyboardShortcut::new(
            ALT_SHIFT,
            egui::Key::ArrowDown,
        )),
        action: Some(MapControlAction::AdjustSelection(-100)),
        description: "Decrease selected cells by 100",
    },
    MapControlEntry {
        binding: MapControlBinding::Keyboard(egui::KeyboardShortcut::new(
            ALT_SHIFT,
            egui::Key::Minus,
        )),
        action: Some(MapControlAction::AdjustSelection(-100)),
        description: "Decrease selected cells by 100",
    },
    MapControlEntry {
        binding: MapControlBinding::Keyboard(egui::KeyboardShortcut::new(
            egui::Modifiers::SHIFT,
            egui::Key::ArrowUp,
        )),
        action: Some(MapControlAction::AdjustSelection(10)),
        description: "Increase selected cells by 10",
    },
    MapControlEntry {
        binding: MapControlBinding::Keyboard(egui::KeyboardShortcut::new(
            egui::Modifiers::SHIFT,
            egui::Key::Plus,
        )),
        action: Some(MapControlAction::AdjustSelection(10)),
        description: "Increase selected cells by 10",
    },
    MapControlEntry {
        binding: MapControlBinding::Keyboard(egui::KeyboardShortcut::new(
            egui::Modifiers::SHIFT,
            egui::Key::Equals,
        )),
        action: Some(MapControlAction::AdjustSelection(10)),
        description: "Increase selected cells by 10",
    },
    MapControlEntry {
        binding: MapControlBinding::Keyboard(egui::KeyboardShortcut::new(
            egui::Modifiers::SHIFT,
            egui::Key::ArrowDown,
        )),
        action: Some(MapControlAction::AdjustSelection(-10)),
        description: "Decrease selected cells by 10",
    },
    MapControlEntry {
        binding: MapControlBinding::Keyboard(egui::KeyboardShortcut::new(
            egui::Modifiers::SHIFT,
            egui::Key::Minus,
        )),
        action: Some(MapControlAction::AdjustSelection(-10)),
        description: "Decrease selected cells by 10",
    },
    MapControlEntry {
        binding: MapControlBinding::Keyboard(egui::KeyboardShortcut::new(
            egui::Modifiers::ALT,
            egui::Key::ArrowUp,
        )),
        action: Some(MapControlAction::AdjustSelection(1)),
        description: "Increase selected cells by 1",
    },
    MapControlEntry {
        binding: MapControlBinding::Keyboard(egui::KeyboardShortcut::new(
            egui::Modifiers::ALT,
            egui::Key::Plus,
        )),
        action: Some(MapControlAction::AdjustSelection(1)),
        description: "Increase selected cells by 1",
    },
    MapControlEntry {
        binding: MapControlBinding::Keyboard(egui::KeyboardShortcut::new(
            egui::Modifiers::ALT,
            egui::Key::Equals,
        )),
        action: Some(MapControlAction::AdjustSelection(1)),
        description: "Increase selected cells by 1",
    },
    MapControlEntry {
        binding: MapControlBinding::Keyboard(egui::KeyboardShortcut::new(
            egui::Modifiers::ALT,
            egui::Key::ArrowDown,
        )),
        action: Some(MapControlAction::AdjustSelection(-1)),
        description: "Decrease selected cells by 1",
    },
    MapControlEntry {
        binding: MapControlBinding::Keyboard(egui::KeyboardShortcut::new(
            egui::Modifiers::ALT,
            egui::Key::Minus,
        )),
        action: Some(MapControlAction::AdjustSelection(-1)),
        description: "Decrease selected cells by 1",
    },
    MapControlEntry {
        binding: MapControlBinding::Keyboard(egui::KeyboardShortcut::new(
            egui::Modifiers::NONE,
            egui::Key::Escape,
        )),
        action: Some(MapControlAction::ClearSelection),
        description: "Collapse selection, then clear",
    },
    MapControlEntry {
        binding: MapControlBinding::Keyboard(egui::KeyboardShortcut::new(
            egui::Modifiers::NONE,
            egui::Key::Home,
        )),
        action: None,
        description: "Select the first map cell",
    },
    MapControlEntry {
        binding: MapControlBinding::Keyboard(egui::KeyboardShortcut::new(
            egui::Modifiers::CTRL,
            egui::Key::A,
        )),
        action: Some(MapControlAction::SelectAll),
        description: "Select all cells in current map",
    },
    MapControlEntry {
        binding: MapControlBinding::Keyboard(egui::KeyboardShortcut::new(
            egui::Modifiers::CTRL.plus(egui::Modifiers::SHIFT),
            egui::Key::Z,
        )),
        action: Some(MapControlAction::RedoEdit),
        description: "Redo last undone map edit",
    },
    MapControlEntry {
        binding: MapControlBinding::Keyboard(egui::KeyboardShortcut::new(
            egui::Modifiers::CTRL,
            egui::Key::Z,
        )),
        action: Some(MapControlAction::UndoEdit),
        description: "Undo last map edit",
    },
    MapControlEntry {
        binding: MapControlBinding::Keyboard(egui::KeyboardShortcut::new(
            egui::Modifiers::CTRL,
            egui::Key::Y,
        )),
        action: Some(MapControlAction::RedoEdit),
        description: "Redo last undone map edit",
    },
    MapControlEntry {
        binding: MapControlBinding::Keyboard(egui::KeyboardShortcut::new(
            egui::Modifiers::CTRL,
            egui::Key::O,
        )),
        action: Some(MapControlAction::LoadFromFile),
        description: "Load map data from file",
    },
    MapControlEntry {
        binding: MapControlBinding::Keyboard(egui::KeyboardShortcut::new(
            egui::Modifiers::CTRL,
            egui::Key::S,
        )),
        action: Some(MapControlAction::SaveToFile),
        description: "Save map data to file",
    },
    MapControlEntry {
        binding: MapControlBinding::Keyboard(egui::KeyboardShortcut::new(
            egui::Modifiers::NONE,
            egui::Key::F4,
        )),
        action: Some(MapControlAction::WriteToRam),
        description: "Write changes to RAM when available",
    },
    MapControlEntry {
        binding: MapControlBinding::Keyboard(egui::KeyboardShortcut::new(
            egui::Modifiers::NONE,
            egui::Key::F5,
        )),
        action: Some(MapControlAction::WriteToEeprom),
        description: "Write changes to EEPROM when available",
    },
    MapControlEntry {
        binding: MapControlBinding::Hold(egui::KeyboardShortcut::new(
            egui::Modifiers::CTRL,
            egui::Key::D,
        )),
        action: Some(MapControlAction::ShowRamEepromDelta),
        description: "Hold to show RAM - EEPROM deltas",
    },
    MapControlEntry {
        binding: MapControlBinding::Keyboard(egui::KeyboardShortcut::new(
            egui::Modifiers::CTRL,
            egui::Key::ArrowUp,
        )),
        action: None,
        description: "Resize selection upward from anchor",
    },
    MapControlEntry {
        binding: MapControlBinding::Keyboard(egui::KeyboardShortcut::new(
            egui::Modifiers::CTRL,
            egui::Key::ArrowDown,
        )),
        action: None,
        description: "Resize selection downward from anchor",
    },
    MapControlEntry {
        binding: MapControlBinding::Keyboard(egui::KeyboardShortcut::new(
            egui::Modifiers::CTRL,
            egui::Key::ArrowLeft,
        )),
        action: None,
        description: "Resize selection left from anchor",
    },
    MapControlEntry {
        binding: MapControlBinding::Keyboard(egui::KeyboardShortcut::new(
            egui::Modifiers::CTRL,
            egui::Key::ArrowRight,
        )),
        action: None,
        description: "Resize selection right from anchor",
    },
    MapControlEntry {
        binding: MapControlBinding::Mouse("Click"),
        action: None,
        description: "Select one cell",
    },
    MapControlEntry {
        binding: MapControlBinding::Mouse("Click, then Shift + Click"),
        action: None,
        description: "Select rectangular area",
    },
    MapControlEntry {
        binding: MapControlBinding::Mouse("Drag"),
        action: None,
        description: "Select rectangular area",
    },
    MapControlEntry {
        binding: MapControlBinding::Mouse("Double click"),
        action: None,
        description: "Edit cell",
    },
];

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
    undo_stack: Vec<Vec<i16>>,
    redo_stack: Vec<Vec<i16>>,
    line_chart_alignment: LineChartAlignmentState,
    lookup_cache: LookupCacheState,
    pending_write: Option<PendingMapWrite>,
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

fn read_u32(a: &[u8]) -> DiagServerResult<(&[u8], u32)> {
    if a.len() < 4 {
        return Err(DiagError::InvalidResponseLength);
    }
    let r = u32::from_le_bytes(a[0..4].try_into().unwrap());
    Ok((&a[4..], r))
}

fn read_f32(a: &[u8]) -> DiagServerResult<(&[u8], f32)> {
    if a.len() < 4 {
        return Err(DiagError::InvalidResponseLength);
    }
    let r = f32::from_le_bytes(a[0..4].try_into().unwrap());
    Ok((&a[4..], r))
}

fn nearest_index(values: &[i16], value: f32) -> Option<usize> {
    values
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            let a_delta = ((**a as f32) - value).abs();
            let b_delta = ((**b as f32) - value).abs();
            a_delta.total_cmp(&b_delta)
        })
        .map(|(idx, _)| idx)
}

fn axis_position(values: &[i16], value: f32) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    if values.len() == 1 {
        return Some(0.0);
    }

    let value = value as f64;
    let first = values[0] as f64;
    let last = values[values.len() - 1] as f64;
    if first <= last {
        if value <= first {
            return Some(0.0);
        }
        if value >= last {
            return Some((values.len() - 1) as f64);
        }
    } else {
        if value >= first {
            return Some(0.0);
        }
        if value <= last {
            return Some((values.len() - 1) as f64);
        }
    }

    values.windows(2).enumerate().find_map(|(idx, pair)| {
        let start = pair[0] as f64;
        let end = pair[1] as f64;
        let between = if start <= end {
            value >= start && value <= end
        } else {
            value <= start && value >= end
        };
        if !between {
            return None;
        }
        if (end - start).abs() <= f64::EPSILON {
            return Some(idx as f64);
        }
        Some(idx as f64 + ((value - start) / (end - start)))
    })
}

fn lerp(start: f64, end: f64, factor: f64) -> f64 {
    start + ((end - start) * factor)
}

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

fn keyboard_control_action<F>(
    input: &mut egui::InputState,
    predicate: F,
) -> Option<MapControlAction>
where
    F: Fn(MapControlAction) -> bool,
{
    MAP_CONTROL_ENTRIES.iter().find_map(|entry| {
        let (MapControlBinding::Keyboard(shortcut), Some(action)) = (&entry.binding, entry.action)
        else {
            return None;
        };
        if predicate(action) && input.consume_shortcut(shortcut) {
            Some(action)
        } else {
            None
        }
    })
}

fn hold_control_active(input: &egui::InputState, target: MapControlAction) -> bool {
    MAP_CONTROL_ENTRIES.iter().any(|entry| {
        let (MapControlBinding::Hold(shortcut), Some(action)) = (&entry.binding, entry.action)
        else {
            return false;
        };
        action == target
            && input.modifiers.matches_logically(shortcut.modifiers)
            && input.key_down(shortcut.logical_key)
    })
}

#[derive(Debug, Clone, Copy)]
pub struct LookupCacheEntry {
    slot_id: u8,
    x: f32,
    y: f32,
    timestamp_ms: u32,
}

#[derive(Debug, Clone)]
pub struct LookupCacheResponse {
    entries: Vec<LookupCacheEntry>,
}

#[derive(Debug, Clone, Copy)]
struct TcuTimeSync {
    tcu_ms: u32,
    host_instant: Instant,
}

struct ActiveLookupCachePoint {
    slot: usize,
    x: f32,
    y: f32,
    age_ms: u32,
    alpha: u8,
    x_idx: usize,
    y_idx: usize,
}

#[derive(Debug, Clone, Copy)]
struct LookupTraceSample {
    slot_id: u8,
    x: f32,
    y: f32,
    timestamp_ms: u32,
}

impl TcuTimeSync {
    fn new(tcu_ms: u32) -> Self {
        Self {
            tcu_ms,
            host_instant: Instant::now(),
        }
    }

    fn estimated_tcu_now_ms(&self) -> u32 {
        self.tcu_ms
            .wrapping_add(self.host_instant.elapsed().as_millis() as u32)
    }
}

enum LookupCacheReadResult {
    Data {
        cache: LookupCacheResponse,
        time_sync: Option<TcuTimeSync>,
    },
    Busy,
    Unsupported,
    Error(String),
}

enum LookupCacheWorkerCommand {
    Read { manual: bool, sync_time: bool },
}

struct LookupCacheWorkerResult {
    manual: bool,
    result: LookupCacheReadResult,
}

#[derive(Debug, Clone, Copy)]
enum PendingMapWrite {
    Ram,
    Eeprom,
}

#[derive(Debug)]
struct LookupCacheState {
    enabled: bool,
    disabled: bool,
    next_poll: Instant,
    worker_tx: Option<Sender<LookupCacheWorkerCommand>>,
    worker_rx: Option<Receiver<LookupCacheWorkerResult>>,
    in_flight: Option<(Instant, bool)>,
    latest: Option<LookupCacheResponse>,
    trace: Vec<LookupTraceSample>,
    time_sync: Option<TcuTimeSync>,
    poll_hz: f32,
    consecutive_errors: u8,
    status: &'static str,
    last_error: Option<String>,
    timeout_reported: bool,
    last_poll_started: Option<Instant>,
    last_data_received: Option<Instant>,
    last_error_at: Option<Instant>,
}

#[derive(Debug, Clone, Copy)]
struct LookupCacheUiSettings {
    enabled: bool,
    poll_hz: f32,
}

impl Clone for LookupCacheState {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl Default for LookupCacheState {
    fn default() -> Self {
        Self {
            enabled: false,
            disabled: false,
            next_poll: Instant::now(),
            worker_tx: None,
            worker_rx: None,
            in_flight: None,
            latest: None,
            trace: Vec::new(),
            time_sync: None,
            poll_hz: LOOKUP_CACHE_DEFAULT_POLL_HZ,
            consecutive_errors: 0,
            status: "idle",
            last_error: None,
            timeout_reported: false,
            last_poll_started: None,
            last_data_received: None,
            last_error_at: None,
        }
    }
}

impl LookupCacheState {
    fn clamp_poll_hz(&mut self) {
        self.poll_hz = self
            .poll_hz
            .clamp(LOOKUP_CACHE_MIN_POLL_HZ, LOOKUP_CACHE_MAX_POLL_HZ);
    }

    fn poll_interval(&self) -> Duration {
        Duration::from_secs_f32(
            1.0 / self
                .poll_hz
                .clamp(LOOKUP_CACHE_MIN_POLL_HZ, LOOKUP_CACHE_MAX_POLL_HZ),
        )
    }

    fn ui_settings(&self) -> LookupCacheUiSettings {
        LookupCacheUiSettings {
            enabled: self.enabled,
            poll_hz: self.poll_hz,
        }
    }

    fn apply_ui_settings(&mut self, settings: LookupCacheUiSettings) {
        self.enabled = settings.enabled;
        self.poll_hz = settings.poll_hz;
        self.clamp_poll_hz();
        self.next_poll = Instant::now();
    }
}

impl Map {
    pub fn new(map_id: MapType, nag: Nag52Diag, meta: MapData) -> DiagServerResult<Self> {
        // Read metadata

        let ecu_response = nag.with_kwp(|server| {
            server
                .send_byte_array_with_response(
                    &[
                        KwpCommand::ReadDataByLocalIdentifier.into(),
                        0x19,
                        map_id as u8,
                        MapCmd::ReadMeta as u8,
                        0x00,
                        0x00,
                    ],
                    None,
                )
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
                .send_byte_array_with_response(
                    &[
                        KwpCommand::ReadDataByLocalIdentifier.into(),
                        0x19,
                        map_id as u8,
                        MapCmd::Read as u8,
                        0x00,
                        0x00,
                    ],
                    None,
                )
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
                .send_byte_array_with_response(
                    &[
                        KwpCommand::ReadDataByLocalIdentifier.into(),
                        0x19,
                        map_id as u8,
                        MapCmd::ReadDefault as u8,
                        0x00,
                        0x00,
                    ],
                    None,
                )
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
                .send_byte_array_with_response(
                    &[
                        KwpCommand::ReadDataByLocalIdentifier.into(),
                        0x19,
                        map_id as u8,
                        MapCmd::ReadEEPROM as u8,
                        0x00,
                        0x00,
                    ],
                    None,
                )
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

        let time_sync = Self::read_tcu_time_sync_blocking(&nag).ok();

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
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            line_chart_alignment: LineChartAlignmentState::default(),
            lookup_cache: LookupCacheState {
                time_sync,
                ..LookupCacheState::default()
            },
            pending_write: None,
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

    fn read_tcu_time_from_server(server: &DynamicDiagSession) -> DiagServerResult<TcuTimeSync> {
        let response = server.kwp_read_custom_local_identifier(RLI_TCU_TIME)?;
        if response.len() != 4 {
            return Err(DiagError::InvalidResponseLength);
        }
        let (remaining, tcu_ms) = read_u32(&response)?;
        if !remaining.is_empty() {
            return Err(DiagError::InvalidResponseLength);
        }
        Ok(TcuTimeSync::new(tcu_ms))
    }

    fn read_tcu_time_sync_blocking(nag: &Nag52Diag) -> DiagServerResult<TcuTimeSync> {
        nag.with_kwp(Self::read_tcu_time_from_server)
    }

    fn parse_lookup_cache_response(ecu_response: Vec<u8>) -> DiagServerResult<LookupCacheResponse> {
        let (payload, payload_len) = read_u16(&ecu_response)?;
        if payload.len() != payload_len as usize || payload_len < 4 {
            return Err(DiagError::InvalidResponseLength);
        }
        let entry_count = payload[0];
        let entry_size = payload[1];
        if entry_size != LOOKUP_CACHE_ENTRY_SIZE || entry_count > LOOKUP_CACHE_MAX_SLOTS {
            return Err(DiagError::InvalidResponseLength);
        }
        if payload[2] != 0 || payload[3] != 0 {
            return Err(DiagError::InvalidResponseLength);
        }
        let expected_len = 4usize + (entry_count as usize * entry_size as usize);
        if payload.len() != expected_len {
            return Err(DiagError::InvalidResponseLength);
        }
        let mut data = &payload[4..];
        let mut entries = Vec::with_capacity(entry_count as usize);
        let mut seen_slots = [false; LOOKUP_CACHE_MAX_SLOTS as usize];
        for _ in 0..entry_count {
            if data.is_empty() {
                return Err(DiagError::InvalidResponseLength);
            }
            let slot_id = data[0];
            if slot_id >= LOOKUP_CACHE_MAX_SLOTS || seen_slots[slot_id as usize] {
                return Err(DiagError::InvalidResponseLength);
            }
            seen_slots[slot_id as usize] = true;
            let (d, x) = read_f32(&data[1..])?;
            let (d, y) = read_f32(d)?;
            let (d, timestamp_ms) = read_u32(d)?;
            entries.push(LookupCacheEntry {
                slot_id,
                x,
                y,
                timestamp_ms,
            });
            data = d;
        }
        Ok(LookupCacheResponse { entries })
    }

    fn read_lookup_cache_once(
        nag: Nag52Diag,
        map_id: MapType,
        sync_time: bool,
    ) -> LookupCacheReadResult {
        match nag.try_with_kwp(|server| {
            let time_sync = if sync_time {
                Some(Self::read_tcu_time_from_server(server)?)
            } else {
                None
            };
            server
                .send_byte_array_with_response(
                    &[
                        KwpCommand::ReadDataByLocalIdentifier.into(),
                        0x19,
                        map_id as u8,
                        MapCmd::GetLookupVals as u8,
                        0x00,
                        0x00,
                    ],
                    None,
                )
                .and_then(|mut x| {
                    if x.is_empty() {
                        return Err(DiagError::InvalidResponseLength);
                    }
                    if x[0] != KWP_POSITIVE_READ_DATA_BY_LOCAL_IDENTIFIER {
                        return Err(DiagError::InvalidResponseLength);
                    }
                    x.drain(0..1);
                    Self::parse_lookup_cache_response(x)
                })
                .map(|cache| (cache, time_sync))
        }) {
            Ok(Some((cache, time_sync))) => LookupCacheReadResult::Data { cache, time_sync },
            Ok(None) => LookupCacheReadResult::Busy,
            Err(DiagError::ECUError {
                code: KWP_NRC_SUB_FUNC_NOT_SUPPORTED_INVALID_FORMAT,
                ..
            })
            | Err(DiagError::ECUError {
                code: KWP_NRC_REQUEST_OUT_OF_RANGE,
                ..
            })
            | Err(DiagError::NotSupported) => LookupCacheReadResult::Unsupported,
            Err(e) => LookupCacheReadResult::Error(e.to_string()),
        }
    }

    fn ensure_lookup_cache_worker(&mut self) {
        if self.lookup_cache.worker_tx.is_some() {
            return;
        }
        let nag = self.ecu_ref.clone();
        let map_id = self.meta.id;
        let (cmd_tx, cmd_rx) = mpsc::channel::<LookupCacheWorkerCommand>();
        let (result_tx, result_rx) = mpsc::channel::<LookupCacheWorkerResult>();
        let _ = thread::Builder::new()
            .name("map-lookup-cache-poll".into())
            .spawn(move || {
                while let Ok(cmd) = cmd_rx.recv() {
                    match cmd {
                        LookupCacheWorkerCommand::Read { manual, sync_time } => {
                            let result =
                                Self::read_lookup_cache_once(nag.clone(), map_id, sync_time);
                            if result_tx
                                .send(LookupCacheWorkerResult { manual, result })
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                }
            });
        self.lookup_cache.worker_tx = Some(cmd_tx);
        self.lookup_cache.worker_rx = Some(result_rx);
    }

    fn start_lookup_cache_request(&mut self, manual: bool) {
        self.ensure_lookup_cache_worker();
        let Some(worker_tx) = &self.lookup_cache.worker_tx else {
            self.handle_lookup_cache_result(
                LookupCacheReadResult::Error("Lookup cache worker unavailable".into()),
                manual,
            );
            return;
        };
        let sync_time = manual || self.lookup_cache.time_sync.is_none();
        if worker_tx
            .send(LookupCacheWorkerCommand::Read { manual, sync_time })
            .is_err()
        {
            self.lookup_cache.worker_tx = None;
            self.lookup_cache.worker_rx = None;
            self.handle_lookup_cache_result(
                LookupCacheReadResult::Error("Lookup cache worker disconnected".into()),
                manual,
            );
            return;
        }
        self.lookup_cache.in_flight = Some((Instant::now(), manual));
        self.lookup_cache.last_poll_started = Some(Instant::now());
        self.lookup_cache.timeout_reported = false;
        self.lookup_cache.status = "in flight";
    }

    fn handle_lookup_cache_result(&mut self, result: LookupCacheReadResult, manual: bool) {
        match result {
            LookupCacheReadResult::Data { cache, time_sync } => {
                let poll_interval = self.lookup_cache.poll_interval();
                if let Some(time_sync) = time_sync {
                    self.lookup_cache.time_sync = Some(time_sync);
                }
                self.record_lookup_trace(&cache);
                self.lookup_cache.latest = Some(cache);
                self.lookup_cache.consecutive_errors = 0;
                self.lookup_cache.disabled = false;
                self.lookup_cache.status = "live";
                self.lookup_cache.last_error = None;
                self.lookup_cache.last_data_received = Some(Instant::now());
                self.lookup_cache.next_poll = Instant::now() + poll_interval;
            }
            LookupCacheReadResult::Busy => {
                self.lookup_cache.status = "diagnostics busy";
                self.lookup_cache.next_poll = Instant::now() + self.lookup_cache.poll_interval();
            }
            LookupCacheReadResult::Unsupported => {
                self.lookup_cache.disabled = true;
                self.lookup_cache.status = "unsupported";
                self.lookup_cache.last_error = Some("Live cursor unsupported by firmware".into());
                self.lookup_cache.last_error_at = Some(Instant::now());
                self.lookup_cache.next_poll = Instant::now() + LOOKUP_CACHE_BACKOFF_INTERVAL;
            }
            LookupCacheReadResult::Error(err) => {
                self.lookup_cache.consecutive_errors =
                    self.lookup_cache.consecutive_errors.saturating_add(1);
                self.lookup_cache.status = "error";
                self.lookup_cache.last_error = Some(err);
                self.lookup_cache.last_error_at = Some(Instant::now());
                if self.lookup_cache.consecutive_errors >= LOOKUP_CACHE_DISABLE_AFTER_ERRORS
                    && !manual
                {
                    self.lookup_cache.disabled = true;
                    self.lookup_cache.status = "disabled";
                }
                let delay =
                    if self.lookup_cache.consecutive_errors >= LOOKUP_CACHE_BACKOFF_AFTER_ERRORS {
                        LOOKUP_CACHE_BACKOFF_INTERVAL
                    } else {
                        self.lookup_cache.poll_interval()
                    };
                self.lookup_cache.next_poll = Instant::now() + delay;
            }
        }
    }

    fn record_lookup_trace(&mut self, cache: &LookupCacheResponse) {
        let tcu_now_ms = self
            .lookup_cache
            .time_sync
            .map(|sync| sync.estimated_tcu_now_ms());
        if let Some(tcu_now_ms) = tcu_now_ms {
            self.lookup_cache.trace.retain(|sample| {
                tcu_now_ms.wrapping_sub(sample.timestamp_ms) <= LOOKUP_TRACE_FADE_MS
            });
        }

        for entry in &cache.entries {
            if self.lookup_cache.trace.iter().any(|sample| {
                sample.slot_id == entry.slot_id && sample.timestamp_ms == entry.timestamp_ms
            }) {
                continue;
            }
            self.lookup_cache.trace.push(LookupTraceSample {
                slot_id: entry.slot_id,
                x: entry.x,
                y: entry.y,
                timestamp_ms: entry.timestamp_ms,
            });
        }
    }

    fn update_lookup_cache_poll(&mut self, ctx: &egui::Context, skip_start: bool) {
        if let Some(rx) = self.lookup_cache.worker_rx.take() {
            match rx.try_recv() {
                Ok(worker_result) => {
                    self.lookup_cache.in_flight = None;
                    self.lookup_cache.worker_rx = Some(rx);
                    self.handle_lookup_cache_result(worker_result.result, worker_result.manual);
                }
                Err(mpsc::TryRecvError::Empty) => {
                    self.lookup_cache.worker_rx = Some(rx);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    let manual = self
                        .lookup_cache
                        .in_flight
                        .map(|(_, manual)| manual)
                        .unwrap_or(false);
                    self.lookup_cache.in_flight = None;
                    self.lookup_cache.worker_tx = None;
                    self.handle_lookup_cache_result(
                        LookupCacheReadResult::Error("Lookup cache worker disconnected".into()),
                        manual,
                    );
                }
            }
        }

        if let Some((started, _manual)) = self.lookup_cache.in_flight {
            if !self.lookup_cache.timeout_reported
                && started.elapsed() > LOOKUP_CACHE_REQUEST_TIMEOUT
            {
                self.lookup_cache.timeout_reported = true;
                self.lookup_cache.in_flight = None;
                self.lookup_cache.worker_tx = None;
                self.lookup_cache.worker_rx = None;
                self.lookup_cache.disabled = true;
                self.lookup_cache.status = "timeout";
                self.lookup_cache.last_error = Some(
                    "Lookup cache request timed out; live cursor disabled until refresh".into(),
                );
                self.lookup_cache.last_error_at = Some(Instant::now());
                self.lookup_cache.next_poll = Instant::now() + LOOKUP_CACHE_BACKOFF_INTERVAL;
                return;
            }
        }

        if self.lookup_cache.enabled {
            ctx.request_repaint_after(self.lookup_cache.poll_interval());
        }

        if !self.lookup_cache.enabled
            || self.lookup_cache.disabled
            || skip_start
            || self.pending_write.is_some()
            || self.lookup_cache.in_flight.is_some()
        {
            return;
        }

        if Instant::now() >= self.lookup_cache.next_poll {
            self.start_lookup_cache_request(false);
        }
    }

    fn show_lookup_cache_controls(
        &mut self,
        ui: &mut egui::Ui,
        active_lookup_points: &[ActiveLookupCachePoint],
    ) {
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.lookup_cache.enabled, "Live cursor");
            ui.label("Hz");
            let hz_response = ui.add(
                DragValue::new(&mut self.lookup_cache.poll_hz)
                    .range(LOOKUP_CACHE_MIN_POLL_HZ..=LOOKUP_CACHE_MAX_POLL_HZ)
                    .speed(0.1)
                    .max_decimals(1),
            );
            self.lookup_cache.clamp_poll_hz();
            if hz_response.changed() {
                self.lookup_cache.next_poll = Instant::now() + self.lookup_cache.poll_interval();
            }
            let refresh = ui.button("Refresh").clicked();
            if refresh {
                self.lookup_cache.disabled = false;
                self.lookup_cache.consecutive_errors = 0;
                if self.lookup_cache.in_flight.is_none() {
                    self.start_lookup_cache_request(true);
                }
            }
            let now = Instant::now();
            Self::lookup_cache_lamp(
                ui,
                "Poll",
                self.lookup_cache
                    .last_poll_started
                    .map(|instant| now.duration_since(instant).as_millis() <= LOOKUP_LAMP_POLL_MS)
                    .unwrap_or(false),
                Color32::from_rgb(70, 180, 255),
            );
            Self::lookup_cache_lamp(
                ui,
                "Data",
                self.lookup_cache
                    .last_data_received
                    .map(|instant| now.duration_since(instant).as_millis() <= LOOKUP_LAMP_DATA_MS)
                    .unwrap_or(false),
                Color32::from_rgb(80, 220, 120),
            );
            Self::lookup_cache_lamp(
                ui,
                "Cache",
                !active_lookup_points.is_empty(),
                Self::lookup_cache_marker_color(ui.visuals().dark_mode, 96),
            );
            Self::lookup_cache_lamp(
                ui,
                "Err",
                self.lookup_cache
                    .last_error_at
                    .map(|instant| now.duration_since(instant).as_millis() <= LOOKUP_LAMP_ERROR_MS)
                    .unwrap_or(false),
                Color32::from_rgb(255, 80, 70),
            );
            if matches!(
                self.lookup_cache.status,
                "diagnostics busy" | "unsupported" | "disabled" | "timeout" | "write queued"
            ) {
                ui.label(self.lookup_cache.status);
            }
            ui.label(format!("Trace {}", active_lookup_points.len()));
        });
        if let Some(err) = &self.lookup_cache.last_error {
            ui.colored_label(ui.visuals().warn_fg_color, err);
        }
    }

    fn lookup_cache_lamp(ui: &mut egui::Ui, label: &str, active: bool, color: Color32) {
        let size = egui::Vec2::splat(8.0);
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
        let color = if active {
            color
        } else {
            Color32::from_gray(45)
        };
        ui.painter().circle_filled(rect.center(), 3.5, color);
        response.on_hover_text(label);
        ui.label(label);
    }

    fn active_lookup_cache_points(&self) -> Vec<ActiveLookupCachePoint> {
        let Some(time_sync) = self.lookup_cache.time_sync else {
            return Vec::new();
        };
        let tcu_now_ms = time_sync.estimated_tcu_now_ms();
        self.lookup_cache
            .trace
            .iter()
            .filter_map(|sample| {
                let age_ms = tcu_now_ms.wrapping_sub(sample.timestamp_ms);
                if age_ms > LOOKUP_TRACE_FADE_MS {
                    return None;
                }
                let fade = 1.0 - (age_ms as f32 / LOOKUP_TRACE_FADE_MS as f32);
                let alpha = ((fade * fade * 128.0).round() as u8).clamp(16, 128);
                Some(ActiveLookupCachePoint {
                    slot: sample.slot_id as usize,
                    x: sample.x,
                    y: sample.y,
                    age_ms,
                    alpha,
                    x_idx: nearest_index(&self.x_values, sample.x)?,
                    y_idx: nearest_index(&self.y_values, sample.y)?,
                })
            })
            .collect()
    }

    fn active_lookup_cache_points_for_slot<'a>(
        points: &'a [ActiveLookupCachePoint],
        slot: usize,
    ) -> Vec<&'a ActiveLookupCachePoint> {
        let mut points: Vec<_> = points.iter().filter(|point| point.slot == slot).collect();
        points.sort_by_key(|point| point.age_ms);
        points.reverse();
        points
    }

    fn latest_lookup_cache_points<'a>(
        points: &'a [ActiveLookupCachePoint],
    ) -> Vec<&'a ActiveLookupCachePoint> {
        let mut latest_points: Vec<&ActiveLookupCachePoint> = Vec::new();
        for slot in 0..LOOKUP_CACHE_MAX_SLOTS as usize {
            if let Some(point) = points
                .iter()
                .filter(|point| point.slot == slot)
                .min_by_key(|point| point.age_ms)
            {
                latest_points.push(point);
            }
        }
        latest_points
    }

    fn lookup_cache_cell_info(
        &self,
        points: &[ActiveLookupCachePoint],
        x_pos: usize,
        y_pos: usize,
    ) -> (u8, Option<String>) {
        let mut alpha = 0u8;
        let mut lines = Vec::new();
        for point in points {
            if point.x_idx == x_pos && point.y_idx == y_pos {
                alpha = alpha.max(point.alpha);
                lines.push(format!(
                    "Live cursor slot {}: X={:.2} {}, Y={:.2} {}, age={} ms",
                    point.slot, point.x, self.meta.x_unit, point.y, self.meta.y_unit, point.age_ms
                ));
            }
        }
        let tooltip = if lines.is_empty() {
            None
        } else {
            Some(lines.join("\n"))
        };
        (alpha, tooltip)
    }

    fn lookup_cache_rgb(dark_mode: bool) -> (u8, u8, u8) {
        if dark_mode {
            (60, 230, 255)
        } else {
            (0, 110, 170)
        }
    }

    fn lookup_cache_marker_color(dark_mode: bool, alpha: u8) -> Color32 {
        let (r, g, b) = Self::lookup_cache_rgb(dark_mode);
        Color32::from_rgba_unmultiplied(r, g, b, alpha.saturating_add(80))
    }

    fn lookup_cache_fill_color(dark_mode: bool, alpha: u8) -> Color32 {
        let (r, g, b) = Self::lookup_cache_rgb(dark_mode);
        Color32::from_rgba_unmultiplied(r, g, b, alpha / 3)
    }

    fn decorate_lookup_cache_cell(
        ui: &egui::Ui,
        response: egui::Response,
        dark_mode: bool,
        alpha: u8,
        tooltip: Option<&String>,
    ) {
        let response = if let Some(tooltip) = tooltip {
            response.on_hover_text(tooltip)
        } else {
            response
        };
        if alpha > 0 {
            ui.painter().rect_stroke(
                response.rect.expand(2.0),
                2.0,
                egui::Stroke::new(1.5, Self::lookup_cache_marker_color(dark_mode, alpha)),
                egui::StrokeKind::Outside,
            );
        }
    }

    fn data_value_for(&self, src: &[i16], x_idx: usize, y_idx: usize) -> f64 {
        src[(y_idx * self.x_values.len()) + x_idx] as f64
    }

    fn interpolated_data_value_for(&self, src: &[i16], x: f32, y: f32) -> Option<f64> {
        let x_pos = axis_position(&self.x_values, x)?;
        let y_pos = axis_position(&self.y_values, y)?;
        let x0 = x_pos.floor() as usize;
        let y0 = y_pos.floor() as usize;
        let x1 = (x0 + 1).min(self.x_values.len().saturating_sub(1));
        let y1 = (y0 + 1).min(self.y_values.len().saturating_sub(1));
        let x_factor = x_pos - x0 as f64;
        let y_factor = y_pos - y0 as f64;

        let top = lerp(
            self.data_value_for(src, x0, y0),
            self.data_value_for(src, x1, y0),
            x_factor,
        );
        let bottom = lerp(
            self.data_value_for(src, x0, y1),
            self.data_value_for(src, x1, y1),
            x_factor,
        );
        Some(lerp(top, bottom, y_factor))
    }

    fn perform_map_write(&mut self, write: PendingMapWrite) -> PageAction {
        match write {
            PendingMapWrite::Ram => match self.write_to_ram() {
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
            },
            PendingMapWrite::Eeprom => match self.save_to_eeprom() {
                Ok(_) => {
                    let eeprom_key = self.eeprom_key.clone();
                    let lookup_cache_ui_settings = self.lookup_cache.ui_settings();
                    if let Ok(new_data) =
                        Self::new(self.meta.id, self.ecu_ref.clone(), self.meta.clone())
                    {
                        *self = new_data;
                        self.lookup_cache
                            .apply_ui_settings(lookup_cache_ui_settings);
                    }
                    PageAction::SendNotification {
                        text: format!("Map {} EEPROM save OK!", eeprom_key),
                        kind: egui_notify::ToastLevel::Success,
                    }
                }
                Err(e) => PageAction::SendNotification {
                    text: format!("Map {} EEPROM save failed! {}", self.eeprom_key, e),
                    kind: egui_notify::ToastLevel::Error,
                },
            },
        }
    }

    fn request_map_write(&mut self, write: PendingMapWrite) -> PageAction {
        if self.lookup_cache.in_flight.is_some() {
            self.pending_write = Some(write);
            self.lookup_cache.status = "write queued";
            PageAction::SendNotification {
                text: format!(
                    "Map {} write queued until live cursor request finishes.",
                    self.eeprom_key
                ),
                kind: egui_notify::ToastLevel::Info,
            }
        } else {
            self.pending_write = None;
            self.perform_map_write(write)
        }
    }

    fn execute_pending_write(&mut self) -> Option<PageAction> {
        if self.lookup_cache.in_flight.is_some() {
            return None;
        }
        let write = self.pending_write.take()?;
        Some(self.perform_map_write(write))
    }

    fn set_selection(&mut self, row: usize, col: usize, extend: bool) {
        self.selection = Some(if extend {
            MapSelection {
                anchor: self
                    .selection
                    .map(|selection| selection.anchor)
                    .unwrap_or((row, col)),
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

    fn select_first_cell(&mut self) {
        if self.y_values.is_empty() || self.x_values.is_empty() {
            return;
        }
        self.selection = Some(MapSelection {
            anchor: (0, 0),
            cursor: (0, 0),
        });
        self.selection_dragging = false;
        self.editing_cell = None;
        self.edit_focus_pending = false;
    }

    fn collapse_or_clear_selection(&mut self) {
        let Some(selection) = self.selection else {
            return;
        };
        if selection.anchor != selection.cursor {
            let (min_row, min_col, _, _) = selection.bounds();
            self.selection = Some(MapSelection {
                anchor: (min_row, min_col),
                cursor: (min_row, min_col),
            });
            self.selection_dragging = false;
            self.editing_cell = None;
            self.edit_focus_pending = false;
        } else {
            self.clear_selection();
        }
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

    fn push_undo_state(&mut self, previous: Vec<i16>) {
        if previous == self.data_modify {
            return;
        }
        if self.undo_stack.last() != Some(&previous) {
            self.undo_stack.push(previous);
            if self.undo_stack.len() > MAP_EDIT_HISTORY_LIMIT {
                self.undo_stack.remove(0);
            }
        }
        self.redo_stack.clear();
    }

    fn apply_modified_data_change(&mut self, next: Vec<i16>) {
        if next == self.data_modify {
            return;
        }
        let previous = std::mem::replace(&mut self.data_modify, next);
        self.push_undo_state(previous);
        self.editing_cell = None;
        self.edit_focus_pending = false;
    }

    fn apply_undo_edit(&mut self) {
        let Some(previous) = self.undo_stack.pop() else {
            return;
        };
        let current = std::mem::replace(&mut self.data_modify, previous);
        self.redo_stack.push(current);
        self.editing_cell = None;
        self.edit_focus_pending = false;
    }

    fn apply_redo_edit(&mut self) {
        let Some(next) = self.redo_stack.pop() else {
            return;
        };
        let current = std::mem::replace(&mut self.data_modify, next);
        self.undo_stack.push(current);
        if self.undo_stack.len() > MAP_EDIT_HISTORY_LIMIT {
            self.undo_stack.remove(0);
        }
        self.editing_cell = None;
        self.edit_focus_pending = false;
    }

    fn apply_selection_delta(&mut self, delta: i16) {
        let Some(selection) = self.selection else {
            return;
        };
        let previous = self.data_modify.clone();
        let (min_row, min_col, max_row, max_col) = selection.bounds();
        let x_len = self.x_values.len();
        for row in min_row..=max_row {
            for col in min_col..=max_col {
                let idx = (row * x_len) + col;
                self.data_modify[idx] = self.data_modify[idx].saturating_add(delta);
            }
        }
        self.push_undo_state(previous);
    }

    fn resize_selection(&mut self, row_delta: isize, col_delta: isize) {
        if self.selection.is_none() {
            self.select_first_cell();
        }
        let Some(selection) = self.selection else {
            return;
        };
        let (min_row, min_col, max_row, max_col) = selection.bounds();
        let limit_row = self.y_values.len().saturating_sub(1);
        let limit_col = self.x_values.len().saturating_sub(1);
        let next_max_row = max_row
            .saturating_add_signed(row_delta)
            .clamp(min_row, limit_row);
        let next_max_col = max_col
            .saturating_add_signed(col_delta)
            .clamp(min_col, limit_col);
        if next_max_row == max_row && next_max_col == max_col {
            return;
        }
        self.selection = Some(MapSelection {
            anchor: (min_row, min_col),
            cursor: (next_max_row, next_max_col),
        });
        self.selection_dragging = false;
        self.editing_cell = None;
        self.edit_focus_pending = false;
    }

    fn handle_selection_navigation(&mut self, ui: &mut egui::Ui) -> bool {
        if self.editing_cell.is_some() {
            return false;
        }

        let shortcut = |modifiers, key| egui::KeyboardShortcut::new(modifiers, key);
        let action = ui.input_mut(|input| {
            let is_ctrl_only = input.modifiers.ctrl
                && !input.modifiers.alt
                && !input.modifiers.shift
                && !input.modifiers.mac_cmd;
            if input.consume_shortcut(&shortcut(egui::Modifiers::NONE, egui::Key::Home)) {
                Some((true, 0, 0, false))
            } else if is_ctrl_only {
                if input.consume_shortcut(&shortcut(egui::Modifiers::CTRL, egui::Key::ArrowUp)) {
                    Some((false, -1, 0, true))
                } else if input
                    .consume_shortcut(&shortcut(egui::Modifiers::CTRL, egui::Key::ArrowDown))
                {
                    Some((false, 1, 0, true))
                } else if input
                    .consume_shortcut(&shortcut(egui::Modifiers::CTRL, egui::Key::ArrowLeft))
                {
                    Some((false, 0, -1, true))
                } else if input
                    .consume_shortcut(&shortcut(egui::Modifiers::CTRL, egui::Key::ArrowRight))
                {
                    Some((false, 0, 1, true))
                } else {
                    None
                }
            } else if input.modifiers == egui::Modifiers::NONE {
                if input.consume_shortcut(&shortcut(egui::Modifiers::NONE, egui::Key::ArrowUp)) {
                    Some((false, -1, 0, false))
                } else if input
                    .consume_shortcut(&shortcut(egui::Modifiers::NONE, egui::Key::ArrowDown))
                {
                    Some((false, 1, 0, false))
                } else if input
                    .consume_shortcut(&shortcut(egui::Modifiers::NONE, egui::Key::ArrowLeft))
                {
                    Some((false, 0, -1, false))
                } else if input
                    .consume_shortcut(&shortcut(egui::Modifiers::NONE, egui::Key::ArrowRight))
                {
                    Some((false, 0, 1, false))
                } else if input
                    .consume_shortcut(&shortcut(egui::Modifiers::NONE, egui::Key::Enter))
                {
                    Some((false, 0, 0, false))
                } else {
                    None
                }
            } else {
                None
            }
        });

        match action {
            Some((true, _, _, _)) => {
                self.select_first_cell();
                true
            }
            Some((false, 0, 0, _)) => {
                if self.selection.is_none() {
                    self.select_first_cell();
                }
                self.edit_selection_start();
                true
            }
            Some((false, row_delta, col_delta, true)) => {
                self.resize_selection(row_delta, col_delta);
                true
            }
            Some((false, row_delta, col_delta, false)) => {
                if self.selection.is_none() {
                    self.select_first_cell();
                } else {
                    self.move_selection(row_delta, col_delta);
                }
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

    fn load_from_file_action(&mut self) -> Option<PageAction> {
        let previous = self.data_modify.clone();
        let res = load_map(self)?;
        Some(match res {
            Ok(_) => {
                if self.data_modify != previous {
                    self.push_undo_state(previous);
                }
                PageAction::SendNotification {
                    text: "Map loading OK!".into(),
                    kind: egui_notify::ToastLevel::Success,
                }
            }
            Err(e) => PageAction::SendNotification {
                text: format!("Map loading failed: {e}"),
                kind: egui_notify::ToastLevel::Error,
            },
        })
    }

    fn save_to_file_action(&self) -> Option<PageAction> {
        if self.data_eeprom != self.data_modify || self.data_memory != self.data_eeprom {
            Some(PageAction::SendNotification {
                text: "You have unsaved data in the map. Please write to EEPROM before saving"
                    .into(),
                kind: egui_notify::ToastLevel::Warning,
            })
        } else {
            save_map(self);
            None
        }
    }

    fn handle_file_shortcuts(&mut self, ui: &mut egui::Ui) -> Option<PageAction> {
        if ui.memory(|mem| mem.top_modal_layer().is_some() || mem.focused().is_some()) {
            return None;
        }

        let action = ui.input_mut(|input| {
            keyboard_control_action(input, |action| {
                matches!(
                    action,
                    MapControlAction::LoadFromFile | MapControlAction::SaveToFile
                )
            })
        });

        match action {
            Some(MapControlAction::LoadFromFile) => self.load_from_file_action(),
            Some(MapControlAction::SaveToFile) => self.save_to_file_action(),
            _ => None,
        }
    }

    fn handle_write_shortcuts(&mut self, ui: &mut egui::Ui) -> Option<PageAction> {
        if ui.memory(|mem| mem.top_modal_layer().is_some() || mem.focused().is_some()) {
            return None;
        }

        let action = ui.input_mut(|input| {
            keyboard_control_action(input, |action| {
                matches!(
                    action,
                    MapControlAction::WriteToRam | MapControlAction::WriteToEeprom
                )
            })
        });

        match action {
            Some(MapControlAction::WriteToRam) if self.can_write_to_ram() => {
                Some(self.request_map_write(PendingMapWrite::Ram))
            }
            Some(MapControlAction::WriteToEeprom) if self.can_write_to_eeprom() => {
                Some(self.request_map_write(PendingMapWrite::Eeprom))
            }
            _ => None,
        }
    }

    fn handle_edit_shortcuts(&mut self, ui: &mut egui::Ui) {
        if ui.memory(|mem| mem.top_modal_layer().is_some()) {
            return;
        }
        let has_selection_state = self.selection.is_some() || self.editing_cell.is_some();
        let clear_selection = ui.input_mut(|input| {
            keyboard_control_action(input, |action| action == MapControlAction::ClearSelection)
        });
        if has_selection_state && matches!(clear_selection, Some(MapControlAction::ClearSelection))
        {
            self.collapse_or_clear_selection();
            return;
        }
        if self.view_type != MapViewType::Modify {
            return;
        }
        if ui.memory(|mem| mem.focused().is_some()) {
            return;
        }

        if self.handle_selection_navigation(ui) {
            return;
        }

        let action = ui.input_mut(|input| {
            keyboard_control_action(input, |action| {
                matches!(
                    action,
                    MapControlAction::AdjustSelection(_)
                        | MapControlAction::SelectAll
                        | MapControlAction::UndoEdit
                        | MapControlAction::RedoEdit
                )
            })
        });

        match action {
            Some(MapControlAction::AdjustSelection(delta)) => self.apply_selection_delta(delta),
            Some(MapControlAction::SelectAll) => self.select_all_cells(),
            Some(MapControlAction::UndoEdit) => self.apply_undo_edit(),
            Some(MapControlAction::RedoEdit) => self.apply_redo_edit(),
            _ => {}
        }
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

    fn value_cell_width(&self, ui: &egui::Ui) -> f32 {
        let value_font_id = egui::TextStyle::Button.resolve(ui.style());
        let width = self
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
                ui.painter()
                    .layout_no_wrap(text, value_font_id.clone(), Color32::WHITE)
                    .size()
                    .x
            })
            .fold(0.0_f32, f32::max)
            + (ui.spacing().button_padding.x * 2.0)
            + 8.0;
        width.max(ui.spacing().interact_size.x).ceil()
    }

    fn plot_y_axis_width(&self, ui: &egui::Ui, data: &[i16]) -> f32 {
        let axis_font_id = egui::TextStyle::Body.resolve(ui.style());
        let max_label_width = data
            .iter()
            .map(|value| value.to_string())
            .map(|text| {
                ui.painter()
                    .layout_no_wrap(text, axis_font_id.clone(), ui.visuals().text_color())
                    .size()
                    .x
            })
            .fold(0.0_f32, f32::max);
        (max_label_width + 8.0).ceil()
    }

    fn reset_line_chart_alignment(&mut self) {
        self.line_chart_alignment = LineChartAlignmentState::default();
    }

    fn set_line_chart_table_rect(&mut self, rect: Option<egui::Rect>) {
        self.line_chart_alignment.table_data_rect = rect;
        let column_count = self.x_values.len();
        if self.line_chart_alignment.column_count != column_count {
            self.reset_line_chart_alignment();
            self.line_chart_alignment.column_count = column_count;
            self.line_chart_alignment.table_data_rect = rect;
        }
    }

    fn line_chart_layout(
        &self,
        default_outer_left_margin: f32,
        default_plot_total_width: f32,
    ) -> (f32, f32) {
        let outer_left_margin = self
            .line_chart_alignment
            .outer_left_margin
            .unwrap_or(default_outer_left_margin)
            .max(0.0);
        let plot_total_width = self
            .line_chart_alignment
            .plot_total_width
            .unwrap_or(default_plot_total_width)
            .max(1.0);
        (outer_left_margin, plot_total_width)
    }

    fn update_line_chart_alignment(
        &mut self,
        ctx: &egui::Context,
        target_rect: egui::Rect,
        plot_frame_rect: egui::Rect,
        default_outer_left_margin: f32,
        default_plot_total_width: f32,
    ) {
        let current_outer_left_margin = self
            .line_chart_alignment
            .outer_left_margin
            .unwrap_or(default_outer_left_margin);
        let current_plot_total_width = self
            .line_chart_alignment
            .plot_total_width
            .unwrap_or(default_plot_total_width);

        let left_delta = target_rect.left() - plot_frame_rect.left();
        let width_delta = target_rect.width() - plot_frame_rect.width();

        let next_outer_left_margin = (current_outer_left_margin + left_delta).max(0.0);
        let next_plot_total_width = (current_plot_total_width + width_delta).max(1.0);

        self.line_chart_alignment.plot_frame_rect = Some(plot_frame_rect);
        self.line_chart_alignment.outer_left_margin = Some(next_outer_left_margin);
        self.line_chart_alignment.plot_total_width = Some(next_plot_total_width);

        if left_delta.abs() > 0.5 || width_delta.abs() > 0.5 {
            ctx.request_repaint();
        }
    }

    fn readonly_cell(
        &self,
        cell: &mut egui::Ui,
        row_id: usize,
        x_pos: usize,
        data: &[i16],
        dark_mode: bool,
        lookup_cache_alpha: u8,
        lookup_cache_tooltip: Option<&String>,
    ) {
        if lookup_cache_alpha > 0 {
            cell.painter().rect_filled(
                cell.max_rect().shrink(1.0),
                2.0,
                Self::lookup_cache_fill_color(dark_mode, lookup_cache_alpha),
            );
        }
        let response = cell.label(format!("{}", data[(row_id * self.x_values.len()) + x_pos]));
        Self::decorate_lookup_cache_cell(
            cell,
            response,
            dark_mode,
            lookup_cache_alpha,
            lookup_cache_tooltip,
        );
    }

    fn gen_edit_table(
        &mut self,
        raw_ui: &mut egui::Ui,
        active_lookup_points: &[ActiveLookupCachePoint],
    ) {
        let table_id = (self.meta.id as u8, self.view_type);
        let dark_mode = raw_ui.visuals().dark_mode;
        let header_color = raw_ui.visuals().warn_fg_color;
        let cell_edit_color = raw_ui.visuals().error_fg_color;
        let max_delta = self
            .data_modify
            .iter()
            .zip(self.data_eeprom.iter())
            .map(|(modified, eeprom)| (*modified as i32 - *eeprom as i32).abs())
            .max()
            .unwrap_or(0);
        let value_cell_width = self.value_cell_width(raw_ui);
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
            && raw_ui
                .input(|input| hold_control_active(input, MapControlAction::ShowRamEepromDelta));
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
        let mut first_data_cell_rect = None;
        let mut last_data_cell_rect = None;
        raw_ui.push_id(table_id, |ui| {
            let mut table_builder = egui_extras::TableBuilder::new(ui)
                .striped(true)
                .cell_layout(
                    Layout::left_to_right(egui::Align::Center)
                        .with_cross_align(egui::Align::Center),
                )
                .column(
                    Column::initial(MAP_EDITOR_ROW_HEADER_WIDTH)
                        .at_least(MAP_EDITOR_ROW_HEADER_WIDTH),
                );
            for _ in 0..self.x_values.len() {
                table_builder = table_builder.column(Column::exact(value_cell_width));
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
                            let (lookup_cache_alpha, lookup_cache_tooltip) =
                                self.lookup_cache_cell_info(active_lookup_points, x_pos, row_id);
                            row.col(|cell| match self.view_type {
                                MapViewType::EEPROM => {
                                    if row_id == 0 && x_pos == 0 {
                                        first_data_cell_rect = Some(cell.max_rect());
                                    }
                                    if row_id == 0 && x_pos + 1 == self.x_values.len() {
                                        last_data_cell_rect = Some(cell.max_rect());
                                    }
                                    self.readonly_cell(
                                        cell,
                                        row_id,
                                        x_pos,
                                        &self.data_eeprom,
                                        dark_mode,
                                        lookup_cache_alpha,
                                        lookup_cache_tooltip.as_ref(),
                                    );
                                }
                                MapViewType::Default => {
                                    if row_id == 0 && x_pos == 0 {
                                        first_data_cell_rect = Some(cell.max_rect());
                                    }
                                    if row_id == 0 && x_pos + 1 == self.x_values.len() {
                                        last_data_cell_rect = Some(cell.max_rect());
                                    }
                                    self.readonly_cell(
                                        cell,
                                        row_id,
                                        x_pos,
                                        &self.data_program,
                                        dark_mode,
                                        lookup_cache_alpha,
                                        lookup_cache_tooltip.as_ref(),
                                    );
                                }
                                MapViewType::Modify => {
                                    let cell_rect = cell.max_rect();
                                    if row_id == 0 && x_pos == 0 {
                                        first_data_cell_rect = Some(cell_rect);
                                    }
                                    if row_id == 0 && x_pos + 1 == self.x_values.len() {
                                        last_data_cell_rect = Some(cell_rect);
                                    }
                                    let map_idx = (row_id * self.x_values.len()) + x_pos;
                                    let modified_value = self.data_modify[map_idx];
                                    let eeprom_value = self.data_eeprom[map_idx];
                                    let delta = modified_value as i32 - eeprom_value as i32;
                                    if delta != 0 {
                                        cell.style_mut().visuals.override_text_color =
                                            Some(cell_edit_color)
                                    }
                                    let selected = self
                                        .selection
                                        .map(|selection| selection.contains(row_id, x_pos))
                                        .unwrap_or(false);
                                    let is_editing = self.editing_cell == Some((row_id, x_pos));
                                    let pre_edit_state =
                                        is_editing.then(|| self.data_modify.clone());
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
                                        } else if lookup_cache_alpha > 0 {
                                            button_fill = Some(Self::lookup_cache_fill_color(
                                                dark_mode,
                                                lookup_cache_alpha,
                                            ));
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
                                                self.data_modify[map_idx], self.meta.value_unit
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
                                    if let Some(previous) = pre_edit_state {
                                        if response.changed() {
                                            self.push_undo_state(previous);
                                        }
                                    }
                                    let pointer_over_response = response
                                        .ctx
                                        .input(|input| input.pointer.interact_pos())
                                        .map(|pos| cell_rect.contains(pos))
                                        .unwrap_or(false);
                                    pointer_over_cell |=
                                        response.hovered() || pointer_over_response;
                                    Self::decorate_lookup_cache_cell(
                                        cell,
                                        response.clone(),
                                        dark_mode,
                                        lookup_cache_alpha,
                                        lookup_cache_tooltip.as_ref(),
                                    );
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
                                        let extend =
                                            response.ctx.input(|input| input.modifiers.shift);
                                        let enter_activated = response.has_focus()
                                            && response
                                                .ctx
                                                .input(|input| input.key_pressed(egui::Key::Enter));
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
        self.set_line_chart_table_rect(
            first_data_cell_rect.zip(last_data_cell_rect).map(|(first, last)| {
                egui::Rect::from_min_max(first.min, last.max)
            }),
        );
        if raw_ui.input(|input| input.pointer.primary_clicked()) && !pointer_over_cell {
            self.clear_selection();
        }
        self.handle_edit_shortcuts(raw_ui);
    }

    fn generate_window_ui(&mut self, raw_ui: &mut egui::Ui) -> Option<PageAction> {
        let mut action = None;
        let dark_mode = raw_ui.visuals().dark_mode;
        raw_ui.horizontal(|ui| {
            if ui.button("Load from file").clicked() {
                action = self.load_from_file_action();
            }
            if ui.button("Save to file").clicked() {
                action = self.save_to_file_action();
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
                    self.apply_modified_data_change(self.data_program.clone());
                }
            });
            raw_ui.add_enabled_ui(self.can_write_to_ram(), |ui| {
                if ui.button("Undo user changes").clicked() {
                    action = match self.undo_changes() {
                        Ok(_) => {
                            self.apply_modified_data_change(self.data_eeprom.clone());
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
                    action = Some(self.request_map_write(PendingMapWrite::Ram));
                }
            });
            raw_ui.add_enabled_ui(self.can_write_to_eeprom(), |ui| {
                if ui.button("Write changes (To EEPROM)").clicked() {
                    action = Some(self.request_map_write(PendingMapWrite::Eeprom));
                }
            });
        });
        if action.is_none() {
            action = self.handle_file_shortcuts(raw_ui);
        }
        if action.is_none() {
            action = self.handle_write_shortcuts(raw_ui);
        }
        self.update_lookup_cache_poll(raw_ui.ctx(), action.is_some());
        if action.is_none() {
            action = self.execute_pending_write();
        }
        let active_lookup_points = self.active_lookup_cache_points();
        self.show_lookup_cache_controls(raw_ui, &active_lookup_points);
        self.gen_edit_table(raw_ui, &active_lookup_points);
        ScrollArea::new([true, true])
            .max_height(raw_ui.available_height())
            .show(raw_ui, |raw_ui| {
                // Generate display chart
                if self.x_values.len() == 1 {
                    // Bar chart
                    let src = match self.view_type {
                        MapViewType::Default => &self.data_program,
                        MapViewType::EEPROM => &self.data_eeprom,
                        MapViewType::Modify => &self.data_modify,
                    };
                    let mut bars = Vec::new();
                    for x in 0..self.y_values.len() {
                        // Distinct points
                        let value = src[x];
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
                        .show(raw_ui, |plot_ui| {
                            plot_ui.bar_chart(BarChart::new("", bars));
                            for slot in 0..LOOKUP_CACHE_MAX_SLOTS as usize {
                                let points: Vec<([f64; 2], u8)> =
                                    Self::active_lookup_cache_points_for_slot(
                                        &active_lookup_points,
                                        slot,
                                    )
                                    .into_iter()
                                    .filter_map(|point| {
                                        let x = axis_position(&self.y_values, point.y)
                                            .unwrap_or(point.y_idx as f64);
                                        let value = self
                                            .interpolated_data_value_for(src, point.x, point.y)
                                            .unwrap_or_else(|| src[point.y_idx] as f64);
                                        Some(([x, value], point.alpha))
                                    })
                                    .collect();
                                for segment in points.windows(2) {
                                    let alpha =
                                        ((segment[0].1 as u16 + segment[1].1 as u16) / 2) as u8;
                                    plot_ui.line(
                                        Line::new(
                                            format!("Live cursor trace slot {}", slot),
                                            vec![segment[0].0, segment[1].0],
                                        )
                                        .width(LOOKUP_TRACE_LINE_WIDTH)
                                        .color(Self::lookup_cache_marker_color(dark_mode, alpha)),
                                    );
                                }
                            }
                            for point in Self::latest_lookup_cache_points(&active_lookup_points) {
                                let x = axis_position(&self.y_values, point.y)
                                    .unwrap_or(point.y_idx as f64);
                                let value = self
                                    .interpolated_data_value_for(src, point.x, point.y)
                                    .unwrap_or_else(|| src[point.y_idx] as f64);
                                plot_ui.points(
                                    Points::new(
                                        format!("Live cursor slot {}", point.slot),
                                        vec![[x, value]],
                                    )
                                    .shape(MarkerShape::Cross)
                                    .radius(LOOKUP_CURSOR_CROSS_RADIUS)
                                    .color(Self::lookup_cache_marker_color(dark_mode, point.alpha)),
                                );
                            }
                        });
                } else if self.meta.x_replace.is_some() || self.meta.y_replace.is_some() {
                    // Line chart
                    let src = match self.view_type {
                        MapViewType::Default => self.data_program.clone(),
                        MapViewType::EEPROM => self.data_eeprom.clone(),
                        MapViewType::Modify => self.data_modify.clone(),
                    };
                    let value_cell_width = self.value_cell_width(raw_ui);
                    let plot_y_axis_width = self.plot_y_axis_width(raw_ui, &src);
                    let x_max_idx = self.x_values.len().saturating_sub(1) as f64;
                    let x_plot_min = -0.5;
                    let x_plot_max = x_max_idx + 0.5;
                    let plot_data_width = self.x_values.len() as f32 * value_cell_width;
                    let default_plot_outer_left_margin =
                        (MAP_EDITOR_ROW_HEADER_WIDTH - plot_y_axis_width).max(0.0);
                    let default_plot_total_width = plot_y_axis_width + plot_data_width;
                    let (plot_outer_left_margin, plot_total_width) =
                        self.line_chart_layout(
                            default_plot_outer_left_margin,
                            default_plot_total_width,
                        );
                    let x_labels: Vec<String> = (0..self.x_values.len())
                        .map(|idx| self.get_x_label(idx))
                        .collect();
                    let mut lines: Vec<Line> = Vec::new();
                    for (y_idx, _key) in self.y_values.iter().enumerate() {
                        let mut points: Vec<[f64; 2]> = Vec::new();
                        for x_idx in 0..self.x_values.len() {
                            let data = self.data_value_for(&src, x_idx, y_idx);
                            points.push([x_idx as f64, data as f64]);
                        }
                        lines.push(Line::new(self.get_y_label(y_idx), points));
                    }
                    let x_axis_labels = x_labels.clone();
                    egui::Frame::new()
                        .inner_margin(egui::Margin {
                            left: plot_outer_left_margin.round() as i8,
                            right: 0,
                            top: 0,
                            bottom: 0,
                        })
                        .show(raw_ui, |ui| {
                            let plot_response =
                                egui_plot::Plot::new(format!("PLOT-{}", self.eeprom_key))
                                .allow_drag(false)
                                .allow_scroll(false)
                                .allow_zoom(false)
                                .width(plot_total_width)
                                .y_axis_min_width(plot_y_axis_width)
                                .default_x_bounds(x_plot_min, x_plot_max)
                                .set_margin_fraction(egui::vec2(0.0, 0.05))
                                .x_grid_spacer(move |input| {
                                    let min_idx = input.bounds.0.ceil().max(0.0) as usize;
                                    let max_idx = input.bounds.1.floor().min(x_max_idx) as usize;
                                    (min_idx..=max_idx)
                                        .map(|idx| GridMark {
                                            value: idx as f64,
                                            step_size: 1.0,
                                        })
                                        .collect()
                                })
                                .x_axis_formatter(move |mark, _| {
                                    let idx = mark.value.round();
                                    if (mark.value - idx).abs() > 0.001 {
                                        return String::new();
                                    }
                                    let idx = idx as isize;
                                    if idx < 0 || idx as usize >= x_axis_labels.len() {
                                        return String::new();
                                    }
                                    x_axis_labels[idx as usize].clone()
                                })
                                .show(ui, |plot_ui| {
                                    for l in lines {
                                        plot_ui.line(l);
                                    }
                                    for slot in 0..LOOKUP_CACHE_MAX_SLOTS as usize {
                                        let points: Vec<([f64; 2], u8)> =
                                            Self::active_lookup_cache_points_for_slot(
                                                &active_lookup_points,
                                                slot,
                                            )
                                            .into_iter()
                                            .map(|point| {
                                                let x = axis_position(&self.x_values, point.x)
                                                    .unwrap_or(point.x_idx as f64);
                                                let value = self
                                                    .interpolated_data_value_for(
                                                        &src, point.x, point.y,
                                                    )
                                                    .unwrap_or_else(|| {
                                                        self.data_value_for(
                                                            &src,
                                                            point.x_idx,
                                                            point.y_idx,
                                                        )
                                                    });
                                                ([x, value], point.alpha)
                                            })
                                            .collect();
                                        for segment in points.windows(2) {
                                            let alpha =
                                                ((segment[0].1 as u16 + segment[1].1 as u16) / 2)
                                                    as u8;
                                            plot_ui.line(
                                                Line::new(
                                                    format!("Live cursor trace slot {}", slot),
                                                    vec![segment[0].0, segment[1].0],
                                                )
                                                .width(LOOKUP_TRACE_LINE_WIDTH)
                                                .color(Self::lookup_cache_marker_color(
                                                    dark_mode, alpha,
                                                )),
                                            );
                                        }
                                    }
                                    for point in
                                        Self::latest_lookup_cache_points(&active_lookup_points)
                                    {
                                        let x = axis_position(&self.x_values, point.x)
                                            .unwrap_or(point.x_idx as f64);
                                        let value = self
                                            .interpolated_data_value_for(&src, point.x, point.y)
                                            .unwrap_or_else(|| {
                                                self.data_value_for(
                                                    &src,
                                                    point.x_idx,
                                                    point.y_idx,
                                                )
                                            });
                                        let color =
                                            Self::lookup_cache_marker_color(dark_mode, point.alpha);
                                        plot_ui.vline(
                                            VLine::new(
                                                format!("Live cursor X slot {}", point.slot),
                                                x,
                                            )
                                            .width(LOOKUP_CURSOR_VLINE_WIDTH)
                                            .color(color),
                                        );
                                        plot_ui.points(
                                            Points::new(
                                                format!("Live cursor slot {}", point.slot),
                                                vec![[x, value]],
                                            )
                                            .shape(MarkerShape::Cross)
                                            .radius(LOOKUP_CURSOR_CROSS_RADIUS)
                                            .color(color),
                                        );
                                    }
                                });
                            if let Some(target_rect) = self.line_chart_alignment.table_data_rect {
                                self.update_line_chart_alignment(
                                    ui.ctx(),
                                    target_rect,
                                    *plot_response.transform.frame(),
                                    default_plot_outer_left_margin,
                                    default_plot_total_width,
                                );
                            }
                        });
                } else {
                    let src = match self.view_type {
                        MapViewType::Default => &self.data_program,
                        MapViewType::EEPROM => &self.data_eeprom,
                        MapViewType::Modify => &self.data_modify,
                    };
                    let desired_size =
                        egui::Vec2::new(raw_ui.available_width(), raw_ui.available_height());
                    let (rect, response) =
                        raw_ui.allocate_exact_size(desired_size, egui::Sense::drag());
                    let painter = raw_ui.painter_at(rect);
                    let area = EguiPlotBackend::new(painter, raw_ui.style().to_owned())
                        .into_drawing_area();

                    let x_min = *self.x_values.iter().min().unwrap() as f64;
                    let x_max = *self.x_values.iter().max().unwrap() as f64;
                    let z_min = *self.y_values.iter().min().unwrap() as f64;
                    let z_max = *self.y_values.iter().max().unwrap() as f64;

                    let y_min = *src.iter().min().unwrap() as f64;
                    let y_max = *src.iter().max().unwrap() as f64;

                    self.pitch += response.drag_delta().y as f64 / 30.0;
                    self.rot += response.drag_delta().x as f64 / 30.0;
                    if self.pitch < 0.0 {
                        self.pitch = 0.0;
                    } else if self.pitch > 1.57 {
                        self.pitch = 1.57;
                    }
                    let vis = &raw_ui.ctx().style().visuals;
                    let _ = area.fill(&into_rgba_color(vis.extreme_bg_color));
                    let mut chart = ChartBuilder::on(&area)
                        .build_cartesian_3d(x_min..x_max, y_min..y_max, z_min..z_max)
                        .unwrap();
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
                        .draw()
                        .unwrap();

                    chart
                        .draw_series(
                            SurfaceSeries::xoz(
                                self.x_values.iter().map(|x| *x as f64),
                                self.y_values.iter().map(|y| *y as f64),
                                |x, y| {
                                    let x_v = x as i16;
                                    let y_v = y as i16;
                                    let x_idx =
                                        self.x_values.iter().position(|s| *s == x_v).unwrap();
                                    let y_idx =
                                        self.y_values.iter().position(|s| *s == y_v).unwrap();
                                    let len = self.x_values.len();
                                    src[(len * y_idx) + x_idx] as f64
                                },
                            )
                            .style_func(&|&v| (&HSLColor((v / y_max) * 0.3, 1.0, 0.5)).into()),
                        )
                        .unwrap();
                    let x_step = ((x_max - x_min) * 0.015).max(1.0);
                    let z_step = ((z_max - z_min) * 0.015).max(1.0);
                    for slot in 0..LOOKUP_CACHE_MAX_SLOTS as usize {
                        let points: Vec<((f64, f64, f64), u8)> =
                            Self::active_lookup_cache_points_for_slot(&active_lookup_points, slot)
                                .into_iter()
                                .map(|point| {
                                    let x = point.x as f64;
                                    let z = point.y as f64;
                                    let y = self
                                        .interpolated_data_value_for(src, point.x, point.y)
                                        .unwrap_or_else(|| {
                                            self.data_value_for(src, point.x_idx, point.y_idx)
                                        });
                                    ((x, y, z), point.alpha)
                                })
                                .collect();
                        for segment in points.windows(2) {
                            let alpha =
                                ((segment[0].1 as u16 + segment[1].1 as u16) / 2) as f64 / 255.0;
                            let (r, g, b) = Self::lookup_cache_rgb(dark_mode);
                            let color = RGBColor(r, g, b).mix(alpha);
                            let style =
                                ShapeStyle::from(&color).stroke_width(LOOKUP_TRACE_3D_LINE_WIDTH);
                            let _ = chart.draw_series(LineSeries::new(
                                vec![segment[0].0, segment[1].0],
                                style,
                            ));
                        }
                    }
                    for point in Self::latest_lookup_cache_points(&active_lookup_points) {
                        let x = point.x as f64;
                        let z = point.y as f64;
                        let y = self
                            .interpolated_data_value_for(src, point.x, point.y)
                            .unwrap_or_else(|| self.data_value_for(src, point.x_idx, point.y_idx));
                        let (r, g, b) = Self::lookup_cache_rgb(dark_mode);
                        let color = RGBColor(r, g, b).mix(point.alpha as f64 / 255.0);
                        let _ = chart
                            .draw_series(LineSeries::new(vec![(x, y_min, z), (x, y, z)], &color));
                        let _ = chart.draw_series(LineSeries::new(
                            vec![(x - x_step, y, z), (x + x_step, y, z)],
                            &color,
                        ));
                        let _ = chart.draw_series(LineSeries::new(
                            vec![(x, y, z - z_step), (x, y, z + z_step)],
                            &color,
                        ));
                    }
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
    if let Some(picked) = rfd::FileDialog::new()
        .set_title(format!("Save map {}", map.meta.name))
        .set_file_name(format!("map_{}.mapbin", map.eeprom_key))
        .save_file()
    {
        let bin = bincode::serde::encode_to_vec(&save_data, bincode::config::legacy()).unwrap();
        let mut f = File::create(picked).unwrap();
        let _ = f.write_all(&bin);
    }
}

pub fn load_map(map: &mut Map) -> Option<Result<(), String>> {
    let path = rfd::FileDialog::new()
        .add_filter("mapbin", &["mapbin"])
        .set_title(format!("Pick map file for {}", map.meta.name))
        .pick_file()?;
    let mut f = File::open(path).unwrap();
    let mut contents = Vec::new();
    f.read_to_end(&mut contents).unwrap();
    let save_data =
        bincode::serde::decode_from_slice::<MapSaveData, _>(&contents, bincode::config::legacy())
            .map_err(|e| e.to_string());
    match save_data {
        Ok((data, _)) => {
            if data.id != map.meta.id as u8 {
                return Some(Err(format!(
                    "Map key is different. Expected {}, got {}",
                    map.meta.id as u8, data.id
                )));
            }
            if data.x_values != map.x_values {
                return Some(Err(format!(
                    "X sizes differ! Map spec has changed. Saved map is no longer valid"
                )));
            }
            if data.y_values != map.y_values {
                return Some(Err(format!(
                    "Y sizes differ! Map spec has changed. Saved map is no longer valid"
                )));
            }
            if data.state.len() != map.data_eeprom.len() {
                return Some(Err(format!(
                    "Z sizes differ! Map spec has changed. Saved map is no longer valid"
                )));
            }
            // All OK!
            map.data_modify = data.state;
            return Some(Ok(()));
        }
        Err(e) => return Some(Err(e)),
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
    reset_adaptation: bool,
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
            reset_adaptation,
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

        let mut keyboard_rows: Vec<(String, &'static str)> = Vec::new();
        let mut mouse_rows: Vec<(String, &'static str)> = Vec::new();
        for entry in MAP_CONTROL_ENTRIES {
            match entry.binding {
                MapControlBinding::Keyboard(shortcut) | MapControlBinding::Hold(shortcut) => {
                    let shortcut = ctx.format_shortcut(&shortcut);
                    if let Some((shortcuts, _)) = keyboard_rows
                        .iter_mut()
                        .find(|(_, description)| *description == entry.description)
                    {
                        shortcuts.push_str(", ");
                        shortcuts.push_str(&shortcut);
                    } else {
                        keyboard_rows.push((shortcut, entry.description));
                    }
                }
                MapControlBinding::Mouse(input) => {
                    if let Some((inputs, _)) = mouse_rows
                        .iter_mut()
                        .find(|(_, description)| *description == entry.description)
                    {
                        inputs.push_str(", ");
                        inputs.push_str(input);
                    } else {
                        mouse_rows.push((input.to_owned(), entry.description));
                    }
                }
            }
        }

        let mut close_requested = false;
        let response =
            egui::Modal::new(egui::Id::new("map_editor_shortcuts_modal")).show(ctx, |ui| {
                ui.set_min_width(760.0);
                ui.heading("Map tuner controls");
                ui.separator();
                ui.columns(2, |columns| {
                    columns[0].strong("Keyboard");
                    egui::Grid::new("map_editor_shortcuts_grid")
                        .num_columns(2)
                        .striped(true)
                        .spacing([16.0, 6.0])
                        .show(&mut columns[0], |ui| {
                            ui.strong("Shortcut");
                            ui.strong("Action");
                            ui.end_row();
                            for (shortcut, description) in &keyboard_rows {
                                ui.label(shortcut);
                                ui.label(*description);
                                ui.end_row();
                            }
                        });

                    columns[1].strong("Mouse");
                    egui::Grid::new("map_editor_mouse_actions_grid")
                        .num_columns(2)
                        .striped(true)
                        .spacing([16.0, 6.0])
                        .show(&mut columns[1], |ui| {
                            ui.strong("Input");
                            ui.strong("Action");
                            ui.end_row();
                            for (input, description) in &mouse_rows {
                                ui.label(input);
                                ui.label(*description);
                                ui.end_row();
                            }
                        });
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Close").clicked() {
                            close_requested = true;
                        }
                    });
                });
            });
        if response.should_close() || close_requested {
            self.show_shortcuts = false;
        }
    }
}

impl super::InterfacePage for MapEditor {
    fn make_ui(&mut self, ui: &mut eframe::egui::Ui) -> crate::window::PageAction {
        let mut action = None;
        let mut map_to_switch = None;
        MenuBar::new().ui(ui, |ui| {
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
                ui.menu_button("Shift Adaptations", |ui| {
                    if ui.button("Clutch filling time offset").clicked() {
                        map_to_switch = Some(MapType::ShiftAdaptFillTMap);
                    }
                    if ui.button("Shift circuit pressure offset").clicked() {
                        map_to_switch = Some(MapType::ShiftAdaptFillPMap);
                    }
                    if ui.button("Applying clutch torque offset").clicked() {
                        map_to_switch = Some(MapType::ShiftAdaptTrqApplMap);
                    }
                    if ui.button("Releasing clutch torque offset").clicked() {
                        map_to_switch = Some(MapType::ShiftAdaptTrqFreeMap);
                    }
                });
                ui.menu_button("HFM CAN Specific maps", |ui| {
                    if ui.button("HFM Torque map").clicked() {
                        map_to_switch = Some(MapType::HfmTrqMap);
                    }
                    if ui.button("HFM Mass air flow map").clicked() {
                        map_to_switch = Some(MapType::HfmMafMap);
                    }
                    if ui.button("HFM Max mass air flow map").clicked() {
                        map_to_switch = Some(MapType::HfmMaxMap);
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
            ui.menu_button("Edit", |ui| {
                if let Some(current_map) = self.loaded_map.as_mut() {
                    if ui
                        .add(egui::Button::new("Load from file").shortcut_text("Ctrl+O"))
                        .clicked()
                    {
                        action = current_map.load_from_file_action();
                        ui.close();
                    }
                    if ui
                        .add(egui::Button::new("Save to file").shortcut_text("Ctrl+S"))
                        .clicked()
                    {
                        action = current_map.save_to_file_action();
                        ui.close();
                    }
                    ui.separator();
                    ui.add_enabled_ui(!current_map.undo_stack.is_empty(), |ui| {
                        if ui
                            .add(egui::Button::new("Undo").shortcut_text("Ctrl+Z"))
                            .clicked()
                        {
                            current_map.apply_undo_edit();
                            ui.close();
                        }
                    });
                    ui.add_enabled_ui(!current_map.redo_stack.is_empty(), |ui| {
                        if ui
                            .add(
                                egui::Button::new("Redo")
                                    .shortcut_text("Ctrl+Y / Ctrl+Shift+Z"),
                            )
                            .clicked()
                        {
                            current_map.apply_redo_edit();
                            ui.close();
                        }
                    });
                } else {
                    ui.add_enabled(
                        false,
                        egui::Button::new("Load from file").shortcut_text("Ctrl+O"),
                    );
                    ui.add_enabled(
                        false,
                        egui::Button::new("Save to file").shortcut_text("Ctrl+S"),
                    );
                    ui.separator();
                    ui.add_enabled(false, egui::Button::new("Undo").shortcut_text("Ctrl+Z"));
                    ui.add_enabled(
                        false,
                        egui::Button::new("Redo").shortcut_text("Ctrl+Y / Ctrl+Shift+Z"),
                    );
                }
            });
            if ui.button("Map controls").clicked() {
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
                    kind: egui_notify::ToastLevel::Warning,
                })
            } else {
                if let Some(found_map_info) = MAP_ARRAY.iter().find(|x| x.id == selected) {
                    self.error = None;
                    match Map::new(selected, self.nag.clone(), found_map_info.clone()) {
                        Ok(m) => self.loaded_map = Some(m),
                        Err(e) => {
                            action = Some(PageAction::SendNotification {
                                text: format!("Failed to read map {:?}. {}", selected, e),
                                kind: egui_notify::ToastLevel::Error,
                            })
                        }
                    }
                } else {
                    //Error toast
                    action = Some(PageAction::SendNotification {
                        text: format!(
                            "Failed to find map {:?} (0x{:02X}). This is a bug!",
                            selected, selected as u8
                        ),
                        kind: egui_notify::ToastLevel::Error,
                    })
                }
            }
        }
        ui.separator();
        if let Some(loaded_map) = self.loaded_map.as_mut() {
            if let Some(err) = &self.error {
                ui.centered_and_justified(|ui| {
                    ui.colored_label(Color32::RED, format!("Map failed to load: {err}"))
                });
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
