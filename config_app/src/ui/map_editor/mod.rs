use std::{fs::File, io::{Read, Write}, sync::mpsc::{self, Receiver, Sender}, thread, time::{Duration, Instant}};

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
use egui_extras::Column;
use egui_plot::{Bar, BarChart, Line, MarkerShape, Points, VLine};
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
    GetLookupVals = 0x10,
}

const LOOKUP_CACHE_POLL_INTERVAL: Duration = Duration::from_millis(250);
const LOOKUP_CACHE_BACKOFF_INTERVAL: Duration = Duration::from_millis(1000);
// The diagnostic server read timeout is 10s; keep the UI timeout just above it
// so we do not report a visual timeout while the request is still legitimately
// waiting inside the diagnostics layer.
const LOOKUP_CACHE_REQUEST_TIMEOUT: Duration = Duration::from_millis(11000);
const LOOKUP_CACHE_BACKOFF_AFTER_ERRORS: u8 = 3;
const LOOKUP_CACHE_DISABLE_AFTER_ERRORS: u8 = 5;
const LOOKUP_CACHE_ENTRY_SIZE: u8 = 12;
const LOOKUP_CACHE_MAX_SLOTS: u8 = 5;
const KWP_POSITIVE_READ_DATA_BY_LOCAL_IDENTIFIER: u8 = 0x61;
const KWP_NRC_SUB_FUNC_NOT_SUPPORTED_INVALID_FORMAT: u8 = 0x12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
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

#[derive(Debug, Clone, Copy)]
pub struct LookupCacheEntry {
    x: f32,
    y: f32,
    timestamp_ms: u32,
}

#[derive(Debug, Clone)]
pub struct LookupCacheResponse {
    firmware_now_ms: u32,
    entries: Vec<LookupCacheEntry>,
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

impl LookupCacheResponse {
    fn age_ms(&self, slot: usize) -> Option<u32> {
        let entry = self.entries.get(slot)?;
        Some(self.firmware_now_ms.wrapping_sub(entry.timestamp_ms))
    }
}

enum LookupCacheReadResult {
    Data(LookupCacheResponse),
    Busy,
    Unsupported,
    Error(String),
}

enum LookupCacheWorkerCommand {
    Read { manual: bool },
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
    consecutive_errors: u8,
    status: &'static str,
    last_error: Option<String>,
    timeout_reported: bool,
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
            consecutive_errors: 0,
            status: "idle",
            last_error: None,
            timeout_reported: false,
        }
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
            lookup_cache: LookupCacheState::default(),
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

    fn parse_lookup_cache_response(ecu_response: Vec<u8>) -> DiagServerResult<LookupCacheResponse> {
        let (payload, payload_len) = read_u16(&ecu_response)?;
        if payload.len() != payload_len as usize || payload_len < 8 {
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
        let expected_len = 8usize + (entry_count as usize * entry_size as usize);
        if payload.len() != expected_len {
            return Err(DiagError::InvalidResponseLength);
        }
        let (mut data, firmware_now_ms) = read_u32(&payload[4..])?;
        let mut entries = Vec::with_capacity(entry_count as usize);
        for _ in 0..entry_count {
            let (d, x) = read_f32(data)?;
            let (d, y) = read_f32(d)?;
            let (d, timestamp_ms) = read_u32(d)?;
            entries.push(LookupCacheEntry { x, y, timestamp_ms });
            data = d;
        }
        Ok(LookupCacheResponse {
            firmware_now_ms,
            entries,
        })
    }

    fn read_lookup_cache_once(nag: Nag52Diag, map_id: MapType) -> LookupCacheReadResult {
        match nag.try_with_kwp(|server| {
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
        }) {
            Ok(Some(cache)) => LookupCacheReadResult::Data(cache),
            Ok(None) => LookupCacheReadResult::Busy,
            Err(DiagError::ECUError {
                code: KWP_NRC_SUB_FUNC_NOT_SUPPORTED_INVALID_FORMAT,
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
                        LookupCacheWorkerCommand::Read { manual } => {
                            let result = Self::read_lookup_cache_once(nag.clone(), map_id);
                            if result_tx.send(LookupCacheWorkerResult { manual, result }).is_err() {
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
        if worker_tx
            .send(LookupCacheWorkerCommand::Read { manual })
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
        self.lookup_cache.timeout_reported = false;
        self.lookup_cache.status = "in flight";
    }

    fn handle_lookup_cache_result(&mut self, result: LookupCacheReadResult, manual: bool) {
        match result {
            LookupCacheReadResult::Data(cache) => {
                self.lookup_cache.latest = Some(cache);
                self.lookup_cache.consecutive_errors = 0;
                self.lookup_cache.disabled = false;
                self.lookup_cache.status = "live";
                self.lookup_cache.last_error = None;
                self.lookup_cache.next_poll = Instant::now() + LOOKUP_CACHE_POLL_INTERVAL;
            }
            LookupCacheReadResult::Busy => {
                self.lookup_cache.status = "diagnostics busy";
                self.lookup_cache.next_poll = Instant::now() + LOOKUP_CACHE_POLL_INTERVAL;
            }
            LookupCacheReadResult::Unsupported => {
                self.lookup_cache.disabled = true;
                self.lookup_cache.status = "unsupported";
                self.lookup_cache.last_error = Some("Live cursor unsupported by firmware".into());
                self.lookup_cache.next_poll = Instant::now() + LOOKUP_CACHE_BACKOFF_INTERVAL;
            }
            LookupCacheReadResult::Error(err) => {
                self.lookup_cache.consecutive_errors = self.lookup_cache.consecutive_errors.saturating_add(1);
                self.lookup_cache.status = "error";
                self.lookup_cache.last_error = Some(err);
                if self.lookup_cache.consecutive_errors >= LOOKUP_CACHE_DISABLE_AFTER_ERRORS && !manual {
                    self.lookup_cache.disabled = true;
                    self.lookup_cache.status = "disabled";
                }
                let delay = if self.lookup_cache.consecutive_errors >= LOOKUP_CACHE_BACKOFF_AFTER_ERRORS {
                    LOOKUP_CACHE_BACKOFF_INTERVAL
                } else {
                    LOOKUP_CACHE_POLL_INTERVAL
                };
                self.lookup_cache.next_poll = Instant::now() + delay;
            }
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
                    let manual = self.lookup_cache.in_flight.map(|(_, manual)| manual).unwrap_or(false);
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
            if !self.lookup_cache.timeout_reported && started.elapsed() > LOOKUP_CACHE_REQUEST_TIMEOUT {
                self.lookup_cache.timeout_reported = true;
                self.lookup_cache.in_flight = None;
                self.lookup_cache.worker_tx = None;
                self.lookup_cache.worker_rx = None;
                self.lookup_cache.disabled = true;
                self.lookup_cache.status = "timeout";
                self.lookup_cache.last_error =
                    Some("Lookup cache request timed out; live cursor disabled until refresh".into());
                self.lookup_cache.next_poll = Instant::now() + LOOKUP_CACHE_BACKOFF_INTERVAL;
                return;
            }
        }

        if self.lookup_cache.enabled {
            ctx.request_repaint_after(LOOKUP_CACHE_POLL_INTERVAL);
        }

        if !self.lookup_cache.enabled
            || self.lookup_cache.disabled
            || skip_start
            || self.lookup_cache.in_flight.is_some()
        {
            return;
        }

        if Instant::now() >= self.lookup_cache.next_poll {
            self.start_lookup_cache_request(false);
        }
    }

    fn show_lookup_cache_controls(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.lookup_cache.enabled, "Live cursor");
            let refresh = ui.button("Refresh").clicked();
            if refresh {
                self.lookup_cache.disabled = false;
                self.lookup_cache.consecutive_errors = 0;
                if self.lookup_cache.in_flight.is_none() {
                    self.start_lookup_cache_request(true);
                }
            }
            ui.label(format!("Lookup cache: {}", self.lookup_cache.status));
            if let Some(cache) = &self.lookup_cache.latest {
                ui.label(format!("{} cached point(s)", cache.entries.len()));
            }
        });
        if let Some(err) = &self.lookup_cache.last_error {
            ui.colored_label(ui.visuals().warn_fg_color, err);
        }
    }

    fn active_lookup_cache_points(&self) -> Vec<ActiveLookupCachePoint> {
        let Some(cache) = &self.lookup_cache.latest else {
            return Vec::new();
        };
        cache
            .entries
            .iter()
            .enumerate()
            .filter_map(|(slot, entry)| {
                let age_ms = cache.age_ms(slot)?;
                if age_ms > 2000 {
                    return None;
                }
                let alpha = if age_ms <= 250 {
                    96
                } else if age_ms <= 1000 {
                    72
                } else {
                    44
                };
                Some(ActiveLookupCachePoint {
                    slot,
                    x: entry.x,
                    y: entry.y,
                    age_ms,
                    alpha,
                    x_idx: nearest_index(&self.x_values, entry.x)?,
                    y_idx: nearest_index(&self.y_values, entry.y)?,
                })
            })
            .collect()
    }

    fn lookup_cache_cell_info(&self, x_pos: usize, y_pos: usize) -> (u8, Option<String>) {
        let points = self.active_lookup_cache_points();
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

    fn lookup_cache_marker_color(alpha: u8) -> Color32 {
        Color32::from_rgba_unmultiplied(255, 215, 0, alpha.saturating_add(80))
    }

    fn decorate_lookup_cache_cell(
        ui: &egui::Ui,
        response: egui::Response,
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
                egui::Stroke::new(1.5, Self::lookup_cache_marker_color(alpha)),
                egui::StrokeKind::Outside,
            );
        }
    }

    fn data_value_for(&self, src: &[i16], x_idx: usize, y_idx: usize) -> f64 {
        src[(y_idx * self.x_values.len()) + x_idx] as f64
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
                    if let Ok(new_data) =
                        Self::new(self.meta.id, self.ecu_ref.clone(), self.meta.clone())
                    {
                        *self = new_data;
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
            self.lookup_cache.enabled = false;
            self.lookup_cache.disabled = true;
            self.lookup_cache.status = "write queued";
            self.lookup_cache.last_error = Some("Live cursor paused until queued write completes".into());
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
        }
        .clone();
        let header_color = raw_ui.visuals().warn_fg_color;
        let cell_edit_color = raw_ui.visuals().error_fg_color;
        if self.meta.reset_adaptation {
            raw_ui.strong("Warning. Modifying this map resets adaptation!");
        }
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
        raw_ui.push_id(&hash, |ui| {
            let mut table_builder = egui_extras::TableBuilder::new(ui)
                .striped(true)
                .cell_layout(
                    Layout::left_to_right(egui::Align::Center)
                        .with_cross_align(egui::Align::Center),
                )
                .column(Column::initial(60.0).at_least(60.0));
            for _ in 0..self.x_values.len() {
                table_builder = table_builder.column(Column::auto().at_least(80.0));
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
                            c.label(
                                RichText::new(format!("{}", self.get_y_label(row_id)))
                                    .color(header_color),
                            );
                        });

                        // Data columns
                        for x_pos in 0..self.x_values.len() {
                            let (lookup_cache_alpha, lookup_cache_tooltip) =
                                self.lookup_cache_cell_info(x_pos, row_id);
                            row.col(|cell| {
                                if lookup_cache_alpha > 0 {
                                    cell.painter().rect_filled(
                                        cell.max_rect().shrink(1.0),
                                        2.0,
                                        Color32::from_rgba_unmultiplied(
                                            255,
                                            215,
                                            0,
                                            lookup_cache_alpha / 3,
                                        ),
                                    );
                                }
                                match self.view_type {
                                    MapViewType::EEPROM => {
                                        let response = cell.label(format!(
                                            "{}",
                                            self.data_eeprom
                                                [(row_id * self.x_values.len()) + x_pos]
                                        ));
                                        Self::decorate_lookup_cache_cell(
                                            cell,
                                            response,
                                            lookup_cache_alpha,
                                            lookup_cache_tooltip.as_ref(),
                                        );
                                    }
                                    MapViewType::Default => {
                                        let response = cell.label(format!(
                                            "{}",
                                            self.data_program
                                                [(row_id * self.x_values.len()) + x_pos]
                                        ));
                                        Self::decorate_lookup_cache_cell(
                                            cell,
                                            response,
                                            lookup_cache_alpha,
                                            lookup_cache_tooltip.as_ref(),
                                        );
                                    }
                                    MapViewType::Modify => {
                                        let map_idx = (row_id * self.x_values.len()) + x_pos;
                                        if self.data_modify[map_idx] != self.data_eeprom[map_idx] {
                                            cell.style_mut().visuals.override_text_color =
                                                Some(cell_edit_color)
                                        }
                                        let edit = DragValue::new(&mut self.data_modify[map_idx])
                                            .suffix(self.meta.value_unit)
                                            .update_while_editing(false)
                                            .speed(0);
                                        let response = cell.add(edit);
                                        Self::decorate_lookup_cache_cell(
                                            cell,
                                            response,
                                            lookup_cache_alpha,
                                            lookup_cache_tooltip.as_ref(),
                                        );
                                    }
                                }
                            });
                        }
                    })
                });
        });
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
                                kind: egui_notify::ToastLevel::Success,
                            });
                        }
                        Err(e) => {
                            action = Some(PageAction::SendNotification {
                                text: format!("Map loading failed: {e}"),
                                kind: egui_notify::ToastLevel::Error,
                            });
                        }
                    }
                }
            }
            if ui.button("Save to file").clicked() {
                if self.data_eeprom != self.data_modify || self.data_memory != self.data_eeprom {
                    action = Some(PageAction::SendNotification {
                        text:
                            "You have unsaved data in the map. Please write to EEPROM before saving"
                                .into(),
                        kind: egui_notify::ToastLevel::Warning,
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
            raw_ui.add_enabled_ui(self.data_modify != self.data_eeprom, |ui| {
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
                    action = Some(self.request_map_write(PendingMapWrite::Ram));
                }
            });
            raw_ui.add_enabled_ui(self.data_memory != self.data_eeprom, |ui| {
                if ui.button("Write changes (To EEPROM)").clicked() {
                    action = Some(self.request_map_write(PendingMapWrite::Eeprom));
                }
            });
        });
        self.show_lookup_cache_controls(raw_ui);
        self.update_lookup_cache_poll(raw_ui.ctx(), action.is_some());
        if action.is_none() {
            action = self.execute_pending_write();
        }
        self.gen_edit_table(raw_ui);
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
                            for point in self.active_lookup_cache_points() {
                                let value = src[point.y_idx] as f64;
                                plot_ui.points(
                                    Points::new(
                                        format!("Live cursor slot {}", point.slot),
                                        vec![[point.y_idx as f64, value]],
                                    )
                                    .shape(MarkerShape::Cross)
                                    .radius(7.0)
                                    .color(Self::lookup_cache_marker_color(point.alpha)),
                                );
                            }
                        });
                } else if self.meta.x_replace.is_some() || self.meta.y_replace.is_some() {
                    // Line chart
                    let src = match self.view_type {
                        MapViewType::Default => &self.data_program,
                        MapViewType::EEPROM => &self.data_eeprom,
                        MapViewType::Modify => &self.data_modify,
                    };
                    let mut lines: Vec<Line> = Vec::new();
                    for (y_idx, _key) in self.y_values.iter().enumerate() {
                        let mut points: Vec<[f64; 2]> = Vec::new();
                        for (x_idx, key) in self.x_values.iter().enumerate() {
                            let data = self.data_value_for(src, x_idx, y_idx);
                            points.push([*key as f64, data as f64]);
                        }
                        lines.push(Line::new(self.get_y_label(y_idx), points));
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
                            for point in self.active_lookup_cache_points() {
                                let x = point.x as f64;
                                let value = self.data_value_for(src, point.x_idx, point.y_idx);
                                let color = Self::lookup_cache_marker_color(point.alpha);
                                plot_ui.vline(
                                    VLine::new(format!("Live cursor X slot {}", point.slot), x)
                                        .color(color),
                                );
                                plot_ui.points(
                                    Points::new(
                                        format!("Live cursor slot {}", point.slot),
                                        vec![[x, value]],
                                    )
                                    .shape(MarkerShape::Cross)
                                    .radius(7.0)
                                    .color(color),
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
                    for point in self.active_lookup_cache_points() {
                        let x = point.x as f64;
                        let z = point.y as f64;
                        let y = self.data_value_for(src, point.x_idx, point.y_idx);
                        let color = RGBColor(255, 215, 0).mix(point.alpha as f64 / 255.0);
                        let _ = chart.draw_series(LineSeries::new(
                            vec![(x, y_min, z), (x, y, z)],
                            &color,
                        ));
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
}

impl MapEditor {
    pub fn new(nag: Nag52Diag) -> Self {
        let _ = nag.with_kwp(|server| server.kwp_set_session(KwpSessionTypeByte::Extended(0x93)));
        Self {
            nag,
            loaded_map: None,
            error: None,
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
        });
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
