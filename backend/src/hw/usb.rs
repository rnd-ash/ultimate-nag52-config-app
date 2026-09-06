use ecu_diagnostics::{
    channel::{ChannelError, IsoTPChannel, PayloadChannel, CanChannel, CanFrame},
    hardware::{HardwareError, HardwareInfo, HardwareResult},
};
use serialport::{SerialPort, UsbPortInfo};
use std::{
    io::Write,
    sync::{
        atomic::{AtomicBool, Ordering, AtomicU32},
        mpsc::{self},
        Arc, Mutex, MutexGuard,
    },
    time::Duration,
};


#[derive(Debug, Clone, Copy)]
pub enum EspLogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone)]
pub struct EspLogMessage {
    pub lvl: EspLogLevel,
    pub timestamp: u128,
    pub tag: String,
    pub msg: String,
}

/// Clears the `is_running` flag once the last [`Nag52USB`] clone is dropped.
///
/// This replaces an earlier `Arc::strong_count(..) <= 1` test in `Drop`, which was racy:
/// two clones dropped concurrently could both observe a count of 2 and neither would
/// signal the worker threads to stop, leaking the threads and the serial port handle.
struct ShutdownGuard(Arc<AtomicBool>);

impl Drop for ShutdownGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Relaxed);
    }
}

/// Shared receiving end of one of the reader threads' channels.
///
/// `mpsc::Receiver` is `!Sync` - it is a single-consumer endpoint - but these are reached
/// from several threads at once (e.g. the CAN logger's reader thread and the UI thread's
/// "clear buffer" action), so each is guarded by its own mutex.
type SharedRx<T> = Arc<Mutex<mpsc::Receiver<T>>>;

#[derive(Clone)]
pub struct Nag52USB {
    port: Arc<Mutex<Option<Box<dyn SerialPort>>>>,
    info: HardwareInfo,
    rx_diag: SharedRx<(u32, Vec<u8>)>,
    rx_log: SharedRx<EspLogMessage>,
    rx_can: SharedRx<CanFrame>,
    is_running: Arc<AtomicBool>,
    tx_id: u32,
    rx_id: u32,
    pub tx_bytes: Arc<AtomicU32>,
    pub rx_bytes: Arc<AtomicU32>,
    _shutdown: Arc<ShutdownGuard>,
}

/// Locks a mutex, recovering the inner value if a previous holder panicked.
///
/// Poisoning carries no meaning for a channel endpoint, so it must not turn into a panic
/// in a background thread.
fn lock_recover<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn between<'a>(source: &'a str, start: &'a str, end: &'a str) -> &'a str {
    let start_position = source.find(start);

    if start_position.is_some() {
        let start_position = start_position.unwrap() + start.len();
        let source = &source[start_position..];
        let end_position = source.find(end).unwrap_or_default();
        return &source[..end_position];
    }
    return "";
}

/// Decodes an even-length ASCII hex string into bytes.
///
/// Returns `None` for odd lengths, non-ASCII input (which would otherwise make the
/// byte-pair slicing below panic on a char boundary) and any non-hex digit.
fn parse_hex_bytes(s: &str) -> Option<Vec<u8>> {
    if !s.is_ascii() || s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

impl Nag52USB {
    pub fn new(path: &str, _info: UsbPortInfo) -> HardwareResult<Self> {
        let port = serialport::new(path, 921600)
            .flow_control(serialport::FlowControl::None)
            .timeout(Duration::from_millis(1500))
            .open()
        .map_err(|e| HardwareError::APIError {
            code: 99,
            desc: e.to_string(),
        })?;

        let (read_tx_log, read_rx_log) = mpsc::channel::<EspLogMessage>();
        let (read_tx_diag, read_rx_diag) = mpsc::channel::<(u32, Vec<u8>)>();
        let (read_tx_can, read_rx_can) = mpsc::channel::<CanFrame>();

        let is_running = Arc::new(AtomicBool::new(true));
        let is_running_r = is_running.clone();
        let is_running_rr = is_running.clone();
        let _ = port.clear(serialport::ClearBuffer::All);
        let mut port_clone = port.try_clone().map_err(|e| HardwareError::APIError {
            code: 99,
            desc: e.to_string(),
        })?;

        let tx_bytes = Arc::new(AtomicU32::new(0));
        let rx_bytes = Arc::new(AtomicU32::new(0));
        let rx_bytes_t = rx_bytes.clone();


        let (tx_line, rx_line) = mpsc::channel::<String>();

        let _process_thread = std::thread::spawn(move || {
            // `rx_line.iter()` blocks until the channel closes, so the shutdown flag is
            // checked per message rather than around the loop.
            for mut line in rx_line.iter() {
                if !is_running_rr.load(Ordering::Relaxed) {
                    break;
                }
                if line.starts_with("#") || line.starts_with("07E9") {
                    // First char is #, diag message
                    // Diag message
                    if line.starts_with("#") {
                        line.remove(0);
                    }
                    // Needs at least the 4 hex digits of the CAN ID, and whole byte pairs.
                    if line.len() < 4 || line.len() % 2 != 0 || !line.is_ascii() {
                        eprintln!("Discarding invalid diag msg '{}'", line);
                        continue;
                    }
                    let can_id = match u32::from_str_radix(&line[0..4], 16) {
                        Ok(id) => id,
                        Err(_) => {
                            eprintln!("Discarding invalid diag msg '{}'", line);
                            continue;
                        }
                    };
                    match parse_hex_bytes(&line[4..]) {
                        Some(payload) => {
                            let _ = read_tx_diag.send((can_id, payload));
                        }
                        None => eprintln!("Discarding invalid diag msg '{}'", line),
                    }
                } else if line.starts_with("CF->") {
                    line = line.replace("CF->0x", "");
                    if line.len() < 4 || line.len() % 2 != 0 || line.len() > 20 || !line.is_ascii() {
                        continue; // Corrupt
                    }
                    let cid = match u16::from_str_radix(&line[0..4], 16) {
                        Ok(id) => id,
                        Err(_) => continue, // Corrupt
                    };
                    let data = match parse_hex_bytes(&line[4..]) {
                        Some(d) => d,
                        None => continue, // Corrupt
                    };
                    let cf = CanFrame::new(cid as u32, &data, false);
                    let _ = read_tx_can.send(cf);
                } else {
                    println!("{line}");
                    let lvl = match line.chars().next().unwrap_or(' ') {
                        'I' => EspLogLevel::Info,
                        'W' => EspLogLevel::Warn,
                        'E' => EspLogLevel::Error,
                        'D' => EspLogLevel::Debug,
                        _ => {
                            //println!("Malformed log line {line}");
                            continue
                        }
                    };
                    let timestamp = match u32::from_str_radix(between(&line, "(", ")"), 10) {
                        Ok(ts) => ts,
                        Err(_) => {
                            //println!("Malformed log line {line}");
                            continue
                        }
                    };

                    let tag = between(&line, ") ", ": ");
                    if tag.is_empty() {
                        // No "<tag>: " separator - not a log line we can split.
                        continue;
                    }
                    let prefix = format!("{tag}: ");
                    let msg = match line.find(&prefix) {
                        Some(idx) => &line[idx + prefix.len()..],
                        None => continue,
                    };
                    let _ = read_tx_log.send(EspLogMessage {
                        lvl,
                        timestamp: timestamp as u128,
                        tag: tag.to_string(),
                        msg: msg.to_string()
                    });
                }
            }
        });

        // Create 2 threads, one to read the port, one to write to it
        let _reader_thread = std::thread::spawn(move || {
            println!("Serial reader start");
            let mut read_buf = Vec::new();
            while is_running_r.load(Ordering::Relaxed) {
                // read again in case value changed
                let btr = port_clone.bytes_to_read().unwrap_or_default() as usize;
                let mut r = vec![0x00; btr];
                let actual_read = port_clone.read(&mut r[..btr]).unwrap_or_default();
                rx_bytes_t.fetch_add(actual_read as u32, Ordering::Relaxed);
                read_buf.extend_from_slice(&r[0..actual_read]);
                if read_buf.len() == 0 {
                    std::thread::sleep(Duration::from_millis(1));
                    continue;
                }

                let mut read_idx = usize::MAX;
                for x in 0..read_buf.len() {
                    if read_buf[x] == 0x0A { // \n
                        read_idx = x;
                        break;
                    }
                }
                if read_idx != usize::MAX {
                    let line = String::from_utf8_lossy(&read_buf[0..read_idx]).to_string();
                    read_buf = read_buf[read_idx+1..].to_vec();
                    // The processing thread is gone (shutdown, or it ended early) - stop
                    // reading rather than panicking in a detached thread.
                    if tx_line.send(line).is_err() {
                        break;
                    }
                }
            }
            println!("Serial reader stop");
        });

        Ok(Self {
            port: Arc::new(Mutex::new(Some(port))),
            _shutdown: Arc::new(ShutdownGuard(is_running.clone())),
            is_running,
            info: HardwareInfo {
                name: path.to_string(),
                vendor: Some("rnd-ash@github.com".to_string()),
                device_fw_version: None,
                api_version: None,
                library_version: None,
                library_location: None,
                capabilities: ecu_diagnostics::hardware::HardwareCapabilities {
                    iso_tp: true,
                    can: false,
                    kline: false,
                    kline_kwp: false,
                    sae_j1850: false,
                    sci: false,
                    ip: false,
                },
            },
            rx_diag: Arc::new(Mutex::new(read_rx_diag)),
            rx_log: Arc::new(Mutex::new(read_rx_log)),
            rx_can: Arc::new(Mutex::new(read_rx_can)),
            tx_id: 0,
            rx_id: 0,
            tx_bytes,
            rx_bytes,
        })
    }

    pub fn is_connected(&self) -> bool {
        self.is_running.load(Ordering::Relaxed)
    }

    pub fn read_msg(&self) -> Option<EspLogMessage> {
        lock_recover(&self.rx_log).try_recv().ok()
    }

    pub fn read_can(&self) -> Option<CanFrame> {
        lock_recover(&self.rx_can).try_recv().ok()
    }
}

impl ecu_diagnostics::hardware::Hardware for Nag52USB {
    fn create_iso_tp_channel(&mut self) -> HardwareResult<Box<dyn IsoTPChannel>>
    {
        Ok(Box::new(self.clone()))
    }

    fn create_can_channel(&mut self) -> HardwareResult<Box<dyn CanChannel>>
    {
        Err(HardwareError::ChannelNotSupported)
    }

    fn is_iso_tp_channel_open(&self) -> bool {
        true
    }

    fn is_can_channel_open(&self) -> bool {
        false
    }

    fn read_battery_voltage(&mut self) -> Option<f32> {
        None
    }

    fn read_ignition_voltage(&mut self) -> Option<f32> {
        None
    }

    fn get_info(&self) -> &ecu_diagnostics::hardware::HardwareInfo {
        &self.info
    }

    fn is_connected(&self) -> bool {
        true
    }
}

impl PayloadChannel for Nag52USB {
    fn open(&mut self) -> ecu_diagnostics::channel::ChannelResult<()> {
        match *lock_recover(&self.port) {
            Some(_) => Ok(()),
            None => Err(ChannelError::InterfaceNotOpen),
        }
    }

    fn close(&mut self) -> ecu_diagnostics::channel::ChannelResult<()> {
        match *lock_recover(&self.port) {
            Some(_) => Ok(()),
            None => Err(ChannelError::InterfaceNotOpen),
        }
    }

    fn set_ids(&mut self, send: u32, recv: u32) -> ecu_diagnostics::channel::ChannelResult<()> {
        self.tx_id = send;
        self.rx_id = recv;
        Ok(())
    }

    fn read_bytes(&mut self, timeout_ms: u32) -> ecu_diagnostics::channel::ChannelResult<Vec<u8>> {
        let res = lock_recover(&self.rx_diag).recv_timeout(Duration::from_millis(timeout_ms as u64));
        if let Ok((id, data)) = res {
            if id == self.rx_id {
                Ok(data)
            } else {
                // Should NEVER happen
                Err(ChannelError::Other(format!(
                    "Expected Rx addr 0x{:04X?} but got 0x{:04X?}",
                    self.rx_id, id
                )))
            }
        } else {
            Err(ChannelError::BufferEmpty)
        }
    }

    fn write_bytes(
        &mut self,
        addr: u32,
        _ext_id: Option<u8>,
        buffer: &[u8],
        _timeout_ms: u32,
    ) -> ecu_diagnostics::channel::ChannelResult<()> {
        // Just write buffer
        match lock_recover(&self.port).as_mut() {
            Some(p) => {
                let mut to_write = Vec::with_capacity(buffer.len() + 4);
                let size: u16 = (buffer.len() + 2) as u16;
                to_write.push((size >> 8) as u8);
                to_write.push((size & 0xFF) as u8);
                to_write.push((addr >> 8) as u8);
                to_write.push((addr & 0xFF) as u8);
                to_write.extend_from_slice(&buffer);
                p.write_all(&to_write)
                    .map_err(|e| ChannelError::IOError(Arc::new(e)))?;
                self.tx_bytes.fetch_add(to_write.len() as u32, Ordering::Relaxed);
                Ok(())
            }
            None => Err(ChannelError::InterfaceNotOpen),
        }
    }

    fn clear_rx_buffer(&mut self) -> ecu_diagnostics::channel::ChannelResult<()> {
        match lock_recover(&self.port).is_some() {
            true => {
                let rx = lock_recover(&self.rx_diag);
                while rx.try_recv().is_ok() {} // Clear rx_diag too!
                Ok(())
            }
            false => Err(ChannelError::InterfaceNotOpen),
        }
    }

    fn clear_tx_buffer(&mut self) -> ecu_diagnostics::channel::ChannelResult<()> {
        Ok(())
    }

    fn read_write_bytes(
        &mut self,
        addr: u32,
        ext_id: Option<u8>,
        buffer: &[u8],
        write_timeout_ms: u32,
        read_timeout_ms: u32,
    ) -> ecu_diagnostics::channel::ChannelResult<Vec<u8>> {
        self.write_bytes(addr, ext_id, buffer, write_timeout_ms)?;
        self.read_bytes(read_timeout_ms)
    }
}

impl IsoTPChannel for Nag52USB {
    fn set_iso_tp_cfg(
        &mut self,
        _cfg: ecu_diagnostics::channel::IsoTPSettings,
    ) -> ecu_diagnostics::channel::ChannelResult<()> {
        Ok(()) // Don't care
    }
}
