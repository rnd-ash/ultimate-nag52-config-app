use std::{fs::File, io::{Read, Write}, sync::mpsc::{self, Receiver, Sender}, thread, time::{Duration, Instant}};

use backend::{
    diag::Nag52Diag,
    ecu_diagnostics::{
        dynamic_diag::DynamicDiagSession,
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
        Duration::from_secs_f32(1.0 / self.poll_hz.clamp(LOOKUP_CACHE_MIN_POLL_HZ, LOOKUP_CACHE_MAX_POLL_HZ))
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
            entries.push(LookupCacheEntry { slot_id, x, y, timestamp_ms });
            data = d;
        }
        Ok(LookupCacheResponse { entries })
    }

    fn read_lookup_cache_once(nag: Nag52Diag, map_id: MapType, sync_time: bool) -> LookupCacheReadResult {
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
                            let result = Self::read_lookup_cache_once(nag.clone(), map_id, sync_time);
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
                self.lookup_cache.consecutive_errors = self.lookup_cache.consecutive_errors.saturating_add(1);
                self.lookup_cache.status = "error";
                self.lookup_cache.last_error = Some(err);
                self.lookup_cache.last_error_at = Some(Instant::now());
                if self.lookup_cache.consecutive_errors >= LOOKUP_CACHE_DISABLE_AFTER_ERRORS && !manual {
                    self.lookup_cache.disabled = true;
                    self.lookup_cache.status = "disabled";
                }
                let delay = if self.lookup_cache.consecutive_errors >= LOOKUP_CACHE_BACKOFF_AFTER_ERRORS {
                    LOOKUP_CACHE_BACKOFF_INTERVAL
                } else {
                    self.lookup_cache.poll_interval()
                };
                self.lookup_cache.next_poll = Instant::now() + delay;
            }
        }
    }

    fn record_lookup_trace(&mut self, cache: &LookupCacheResponse) {
        let tcu_now_ms = self.lookup_cache.time_sync.map(|sync| sync.estimated_tcu_now_ms());
        if let Some(tcu_now_ms) = tcu_now_ms {
            self.lookup_cache
                .trace
                .retain(|sample| tcu_now_ms.wrapping_sub(sample.timestamp_ms) <= LOOKUP_TRACE_FADE_MS);
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
                "diagnostics busy" | "unsupported" | "disabled" | "timeout" | "write queued" | "write active"
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
                        self.lookup_cache.apply_ui_settings(lookup_cache_ui_settings);
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

    fn gen_edit_table(&mut self, raw_ui: &mut egui::Ui, active_lookup_points: &[ActiveLookupCachePoint]) {
        let hash = match self.view_type {
            MapViewType::EEPROM => &self.data_eeprom,
            MapViewType::Default => &self.data_program,
            MapViewType::Modify => &self.data_modify,
        }
        .clone();
        let dark_mode = raw_ui.visuals().dark_mode;
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
                                self.lookup_cache_cell_info(active_lookup_points, x_pos, row_id);
                            row.col(|cell| {
                                if lookup_cache_alpha > 0 {
                                    cell.painter().rect_filled(
                                        cell.max_rect().shrink(1.0),
                                        2.0,
                                        Self::lookup_cache_fill_color(dark_mode, lookup_cache_alpha),
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
                                            dark_mode,
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
                                            dark_mode,
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
                                            dark_mode,
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
        let dark_mode = raw_ui.visuals().dark_mode;
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
                                let points: Vec<([f64; 2], u8)> = Self::active_lookup_cache_points_for_slot(
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
                                    let alpha = ((segment[0].1 as u16 + segment[1].1 as u16) / 2) as u8;
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
                            for slot in 0..LOOKUP_CACHE_MAX_SLOTS as usize {
                                let points: Vec<([f64; 2], u8)> = Self::active_lookup_cache_points_for_slot(
                                    &active_lookup_points,
                                    slot,
                                )
                                    .into_iter()
                                    .map(|point| {
                                        let x = point.x as f64;
                                        let value = self
                                            .interpolated_data_value_for(src, point.x, point.y)
                                            .unwrap_or_else(|| self.data_value_for(src, point.x_idx, point.y_idx));
                                        ([x, value], point.alpha)
                                    })
                                    .collect();
                                for segment in points.windows(2) {
                                    let alpha = ((segment[0].1 as u16 + segment[1].1 as u16) / 2) as u8;
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
                                let x = point.x as f64;
                                let value = self
                                    .interpolated_data_value_for(src, point.x, point.y)
                                    .unwrap_or_else(|| self.data_value_for(src, point.x_idx, point.y_idx));
                                let color = Self::lookup_cache_marker_color(dark_mode, point.alpha);
                                plot_ui.vline(
                                    VLine::new(format!("Live cursor X slot {}", point.slot), x)
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
                        let points: Vec<((f64, f64, f64), u8)> = Self::active_lookup_cache_points_for_slot(
                            &active_lookup_points,
                            slot,
                        )
                            .into_iter()
                            .map(|point| {
                                let x = point.x as f64;
                                let z = point.y as f64;
                                let y = self
                                    .interpolated_data_value_for(src, point.x, point.y)
                                    .unwrap_or_else(|| self.data_value_for(src, point.x_idx, point.y_idx));
                                ((x, y, z), point.alpha)
                            })
                            .collect();
                        for segment in points.windows(2) {
                            let alpha = ((segment[0].1 as u16 + segment[1].1 as u16) / 2) as f64 / 255.0;
                            let (r, g, b) = Self::lookup_cache_rgb(dark_mode);
                            let color = RGBColor(r, g, b).mix(alpha);
                            let style = ShapeStyle::from(&color).stroke_width(LOOKUP_TRACE_3D_LINE_WIDTH);
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
