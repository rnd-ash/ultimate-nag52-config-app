use ecu_diagnostics::{
    channel::{ChannelError, IsoTPChannel, PayloadChannel, CanChannel, CanFrame},
    hardware::{HardwareError, HardwareInfo, HardwareResult},
};
use serial_rs::{FlowControl, PortInfo, SerialPort, SerialPortSettings};
use std::{
    io::{BufRead, BufReader, Write},
    panic::catch_unwind,
    sync::{
        atomic::{AtomicBool, Ordering, AtomicU32},
        mpsc::{self, Receiver},
        Arc, RwLock, Mutex,
    },
    time::{Duration, Instant},
};

use super::usb_scanner::Nag52UsbScanner;

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

#[derive(Clone)]
pub struct Nag52USB {
    port: Arc<Mutex<Option<Box<dyn SerialPort>>>>,
    info: HardwareInfo,
    rx_diag: Arc<mpsc::Receiver<(u32, Vec<u8>)>>,
    rx_log: Arc<mpsc::Receiver<EspLogMessage>>,
    rx_can: Arc<mpsc::Receiver<CanFrame>>,
    is_running: Arc<AtomicBool>,
    tx_id: u32,
    rx_id: u32,
    pub tx_bytes: Arc<AtomicU32>,
    pub rx_bytes: Arc<AtomicU32>
}

unsafe impl Sync for Nag52USB {}
unsafe impl Send for Nag52USB {}

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

impl Nag52USB {
    pub fn new(path: &str, _info: PortInfo) -> HardwareResult<Self> {
        let mut port = serial_rs::new_from_path(
            path,
            Some(
                SerialPortSettings::default()
                    .baud(921600)
                    .read_timeout(Some(1500))
                    .write_timeout(Some(1500))
                    .set_flow_control(FlowControl::None)
                    .set_blocking(false),
            ),
        )
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
        port.clear_input_buffer();
        port.clear_output_buffer();
        let mut port_clone = port.try_clone().unwrap();

        let tx_bytes = Arc::new(AtomicU32::new(0));
        let rx_bytes = Arc::new(AtomicU32::new(0));
        let rx_bytes_t = rx_bytes.clone();


        let (tx_line, rx_line) = mpsc::channel::<String>();

        let process_thread = std::thread::spawn(move || {
            while is_running_rr.load(Ordering::Relaxed) {
                for mut line in rx_line.iter() {
                    if line.starts_with("#") || line.starts_with("07E9") {
                        // First char is #, diag message
                        // Diag message
                        if line.starts_with("#") {
                            line.remove(0);
                        }
                        if line.len() % 2 != 0 {
                            eprintln!("Discarding invalid diag msg '{}'", line);
                        } else {
                            let can_id = u32::from_str_radix(&line[0..4], 16).unwrap();
                            if let Ok(p) = catch_unwind(|| {
                                let payload: Vec<u8> = (4..line.len())
                                    .step_by(2)
                                    .map(|i| u8::from_str_radix(&line[i..i + 2], 16).unwrap())
                                    .collect();
                                payload
                            }) {
                                read_tx_diag.send((can_id, p));
                            }
                        }
                    } else if line.starts_with("CF->") {
                        line = line.replace("CF->0x", "");
                        if line.len() % 2 != 0 || line.len() > 20 {
                            continue; // Corrupt
                        }
                        let cid = u16::from_str_radix(&line[0..4], 16).unwrap();
                        let data: Vec<u8> = (4..line.len())
                                    .step_by(2)
                                    .map(|i| u8::from_str_radix(&line[i..i + 2], 16).unwrap())
                                    .collect();
                        let cf = CanFrame::new(cid as u32, &data, false);
                        let _ = read_tx_can.send(cf);
                    } else {
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
                                println!("Malformed log line {line}");
                                continue
                            }
                        };

                        let tag = between(&line, ") ", ": ");
                        let msg = &line[line.find(&format!("{tag}: ")).unwrap()+(tag.len()+2)..];
                        let _ = read_tx_log.send(EspLogMessage { 
                            lvl, 
                            timestamp: timestamp as u128, 
                            tag: tag.to_string(), 
                            msg: msg.to_string()
                        });
                    }
                }
            }
        });

        // Create 2 threads, one to read the port, one to write to it
        let reader_thread = std::thread::spawn(move || {
            println!("Serial reader start");
            let mut read_buf = Vec::new();
            while is_running_r.load(Ordering::Relaxed) {
                // read again in case value changed
                let btr = port_clone.bytes_to_read().unwrap_or_default();
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
                    tx_line.send(line).unwrap();
                }
            }
            println!("Serial reader stop");
        });

        Ok(Self {
            port: Arc::new(Mutex::new(Some(port))),
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
            rx_diag: Arc::new(read_rx_diag),
            rx_log: Arc::new(read_rx_log),
            rx_can: Arc::new(read_rx_can),
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
        self.rx_log.try_recv().ok()
    }

    pub fn read_can(&self) -> Option<CanFrame> {
        self.rx_can.try_recv().ok()
    }
}

impl Drop for Nag52USB {
    fn drop(&mut self) {
        if Arc::strong_count(&self.port) <= 1 {
            self.is_running.store(false, Ordering::Relaxed);
        }
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
        match *self.port.lock().unwrap() {
            Some(_) => Ok(()),
            None => Err(ChannelError::InterfaceNotOpen),
        }
    }

    fn close(&mut self) -> ecu_diagnostics::channel::ChannelResult<()> {
        match *self.port.lock().unwrap() {
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
        if let Ok((id, data)) = self
            .rx_diag
            .recv_timeout(Duration::from_millis(timeout_ms as u64))
        {
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
        match self.port.lock().unwrap().as_mut() {
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
        match self.port.lock().unwrap().is_some() {
            true => {
                while self.rx_diag.try_recv().is_ok() {} // Clear rx_diag too!
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
