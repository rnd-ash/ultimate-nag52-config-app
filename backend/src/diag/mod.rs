use core::fmt;
use std::{
    borrow::{Borrow, BorrowMut},
    sync::{Arc, Mutex, RwLock, mpsc::{Receiver, self}},
};

use ecu_diagnostics::{hardware::{
    passthru::*, Hardware, HardwareError, HardwareInfo, HardwareResult, HardwareScanner,
}, dynamic_diag::{ServerEvent, DiagServerLogger}, DiagError};
use ecu_diagnostics::{
    channel::*,
    dynamic_diag::{
        DiagProtocol, DiagServerAdvancedOptions, DiagServerBasicOptions, DiagSessionMode,
        DynamicDiagSession, TimeoutConfig,
    },
};
use ecu_diagnostics::{kwp2000::*, DiagServerResult};

#[cfg(unix)]
use ecu_diagnostics::hardware::socketcan::{SocketCanDevice, SocketCanScanner};

use crate::hw::{
    usb::{EspLogMessage, Nag52USB},
    usb_scanner::Nag52UsbScanner,
};

pub mod flash;
pub mod ident;
pub mod settings;
pub mod nvs;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AdapterType {
    USB,
    Passthru,
    #[cfg(unix)]
    SocketCAN,
}

#[derive(Debug, Clone)]
pub enum DataState<T> {
    LoadOk(T),
    Unint,
    LoadErr(String)
}

impl<T> DataState<T> {
    pub fn is_ok(&self) -> bool {
        matches!(self, Self::LoadOk(_))
    }

    pub fn get_err(&self) -> String {
        match self {
            DataState::LoadOk(_) => "".into(),
            DataState::Unint => "Uninitialized".into(),
            DataState::LoadErr(e) => e.clone(),
        }
    }
}

#[derive(Clone)]
pub enum AdapterHw {
    Usb(Nag52USB),
    Passthru(PassthruDevice),
    #[cfg(unix)]
    SocketCAN(SocketCanDevice),
}

impl fmt::Debug for AdapterHw {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usb(_) => f.debug_tuple("Usb").finish(),
            Self::Passthru(_) => f.debug_tuple("Passthru").finish(),
            #[cfg(unix)]
            Self::SocketCAN(_) => f.debug_tuple("SocketCAN").finish(),
        }
    }
}

impl AdapterHw {
    pub fn try_connect(info: &HardwareInfo, ty: AdapterType) -> HardwareResult<Self> {
        Ok(match ty {
            AdapterType::USB => Self::Usb(Nag52USB::try_connect(info)?),
            AdapterType::Passthru => Self::Passthru(PassthruDevice::try_connect(info)?),
            #[cfg(unix)]
            AdapterType::SocketCAN => Self::SocketCAN(SocketCanDevice::try_connect(info)?),
        })
    }

    fn get_type(&self) -> AdapterType {
        match self {
            Self::Usb(_) => AdapterType::USB,
            Self::Passthru(_) => AdapterType::Passthru,
            #[cfg(unix)]
            Self::SocketCAN(_) => AdapterType::SocketCAN,
        }
    }

    pub fn create_isotp_channel(&mut self) -> HardwareResult<Box<dyn IsoTPChannel>> {
        match self.borrow_mut() {
            Self::Usb(u) => u.create_iso_tp_channel(),
            Self::Passthru(p) => p.create_iso_tp_channel(),
            #[cfg(unix)]
            Self::SocketCAN(s) => s.create_iso_tp_channel(),
        }
    }

    pub fn get_hw_info(&self) -> HardwareInfo {
        match self {
            Self::Usb(u) => u.get_info().clone(),
            Self::Passthru(p) => p.get_info().clone(),
            #[cfg(unix)]
            Self::SocketCAN(s) => s.get_info().clone(),
        }
    }

    pub fn get_data_rate(&self) -> Option<(u32, u32)> {
        match self {
            Self::Usb(u) => u.get_data_rate(),
            Self::Passthru(p) => p.get_data_rate(),
            #[cfg(unix)]
            Self::SocketCAN(s) => s.get_data_rate(),
        }
    }

    pub fn read_log_msg(&self) -> Option<EspLogMessage> {
        if let Self::Usb(nag) = self {
            nag.read_msg()
        } else {
            None
        }
    }
}

pub trait Nag52Endpoint: Hardware {
    fn is_connected(&self) -> bool;
    fn try_connect(info: &HardwareInfo) -> HardwareResult<Self> where Self: Sized;
    fn get_device_desc(&self) -> String;
    fn get_data_rate(&self) -> Option<(u32, u32)> {
        None
    }
}

#[cfg(unix)]
impl Nag52Endpoint for SocketCanDevice {

    fn is_connected(&self) -> bool {
        self.is_iso_tp_channel_open()
    }

    fn try_connect(info: &HardwareInfo) -> HardwareResult<Self> {
        SocketCanScanner::new().open_device_by_name(&info.name)
    }

    fn get_device_desc(&self) -> String {
        self.get_info().name.clone()
    }
}

impl Nag52Endpoint for PassthruDevice {

    fn is_connected(&self) -> bool {
        self.is_iso_tp_channel_open()
    }

    fn try_connect(info: &HardwareInfo) -> HardwareResult<Self> {
        PassthruScanner::new().open_device_by_name(&info.name)
    }

    fn get_device_desc(&self) -> String {
        self.get_info().name.clone()
    }
}

impl Nag52Endpoint for Nag52USB {

    fn is_connected(&self) -> bool {
        self.is_connected()
    }

    fn try_connect(info: &HardwareInfo) -> HardwareResult<Self> {
        Nag52UsbScanner::new().open_device_by_name(&info.name)
    }

    fn get_device_desc(&self) -> String {
        let info_name = self.get_info().name.clone();
        format!("Ultimate-NAG52 USB on {}", info_name)
    }

    fn get_data_rate(&self) -> Option<(u32, u32)> {
        Some(
            (
                self.tx_bytes.swap(0, std::sync::atomic::Ordering::Relaxed),
                self.rx_bytes.swap(0, std::sync::atomic::Ordering::Relaxed)
            )
        )
    }
}


#[derive(Debug, Clone)]
pub struct NagAppLoggerInner {
    sender: mpsc::Sender<ServerEvent>
}

unsafe impl Send for NagAppLoggerInner{}
unsafe impl Sync for NagAppLoggerInner{}

impl NagAppLoggerInner {
    pub fn new() -> (Self, mpsc::Receiver<ServerEvent>) {
        let (tx, rx) = mpsc::channel::<ServerEvent>();
        (
            Self {
                sender: tx
            },
            rx
        )
    }
}

impl DiagServerLogger for NagAppLoggerInner {
    fn on_event(&self, evt: ServerEvent) {
        self.sender.send(evt);
    }
}

#[derive(Clone, Debug)]
pub struct NagAppLogger {
    recv: Arc<mpsc::Receiver<ServerEvent>>
}

impl NagAppLogger {
    pub fn new() -> (Self, NagAppLoggerInner) {
        let (inner, recv) = NagAppLoggerInner::new();
        (
            Self {
                recv: Arc::new(recv)
            },
            inner
        )
    }
}

#[derive(Debug, Clone)]
pub struct Nag52Diag {
    info: HardwareInfo,
    endpoint: Option<AdapterHw>,
    endpoint_type: AdapterType,
    server: Option<Arc<DynamicDiagSession>>,
    logger: NagAppLogger,
    server_mutex: Arc<Mutex<()>>
}

unsafe impl Sync for Nag52Diag {}
unsafe impl Send for Nag52Diag {}

impl Nag52Diag {
    pub fn new(mut hw: AdapterHw) -> DiagServerResult<Self> {

        let mut channel_cfg = IsoTPSettings {
            block_size: 0,
            st_min: 0,
            extended_addresses: None,
            pad_frame: true,
            can_speed: 500_000,
            can_use_ext_addr: false,
        };

        #[cfg(unix)]
        if let AdapterHw::SocketCAN(_) = hw {
            channel_cfg.block_size = 8;
            channel_cfg.st_min = 0x20;
        }

        let basic_opts = DiagServerBasicOptions {
            send_id: 0x07E1,
            recv_id: 0x07E9,
            timeout_cfg: TimeoutConfig {
                read_timeout_ms: 10000,
                write_timeout_ms: 10000,
            },
        };

        let adv_opts = DiagServerAdvancedOptions {
            global_tp_id: 0,
            tester_present_interval_ms: 2000,
            tester_present_require_response: true,
            global_session_control: false,
            tp_ext_id: None,
            command_cooldown_ms: 0,
        };

        let mut protocol = Kwp2000Protocol::default();
        protocol.register_session_type(DiagSessionMode {
            id: 0x93,
            tp_require: true,
            name: "UN52DevMode".into(),
        });

        let (logger, inner_logger) = NagAppLogger::new();

        let kwp = DynamicDiagSession::new_over_iso_tp(
            protocol,
            hw.create_isotp_channel().map_err(|e| DiagError::from(Arc::new(e)))?,
            channel_cfg,
            basic_opts,
            Some(adv_opts),
            inner_logger
        )?;

        Ok(Self {
            info: hw.get_hw_info(),
            endpoint_type: hw.get_type(),
            endpoint: Some(hw),
            server: Some(Arc::new(kwp)),
            logger,
            server_mutex: Arc::new(Mutex::new(()))
        })
    }

    pub fn try_reconnect(&mut self) -> DiagServerResult<()> {
        {
            let _ = self.server.take();
            let _ = self.endpoint.take();
        }
        // Now try to reconnect

        println!("Trying to find {}", self.info.name);
        let dev = AdapterHw::try_connect(&self.info, self.endpoint_type).map_err(|e| DiagError::from(Arc::new(e)))?;
        *self = Self::new(dev)?;
        Ok(())
    }

    pub fn with_kwp<F, X>(&self, mut kwp_fn: F) -> DiagServerResult<X>
    where
        F: FnMut(&DynamicDiagSession) -> DiagServerResult<X>,
    {
        if self.server_mutex.lock().is_ok() {
            match self.server.borrow() {
                None => Err(DiagError::from(Arc::new(HardwareError::DeviceNotOpen))),
                Some(s) => kwp_fn(&s),
            }
        } else {
            Err(DiagError::ServerNotRunning)
        }
    }

    pub fn get_data_rate(&self) -> Option<(u32, u32)> {
        self.endpoint.as_ref().map(|x| x.get_data_rate()).unwrap_or_else(|| None)
    }

    pub fn read_log_msg(&self) -> Option<EspLogMessage> {
        self.endpoint.as_ref().map(|x| x.read_log_msg()).flatten()
    }

    pub fn has_logger(&self) -> bool {
        self.endpoint_type == AdapterType::USB
    }

    pub fn get_server_event(&self) -> Option<ServerEvent> {
        self.logger.recv.try_recv().ok()
    }

}

#[cfg(test)]
pub mod test_diag {
    use ecu_diagnostics::{hardware::HardwareScanner, DiagError};

    use crate::{diag::AdapterHw, hw::usb_scanner::Nag52UsbScanner};

    use super::Nag52Diag;

    #[ignore]
    #[test]
    pub fn test_kwp_reconnect() {
        let scanner = Nag52UsbScanner::new();
        let dev = scanner.open_device_by_name("/dev/ttyUSB0").unwrap();
        let mut kwp = match Nag52Diag::new(AdapterHw::Usb(dev)) {
            Ok(kwp) => kwp,
            Err(e) => {
                eprintln!("Error starting KWP {e}");
                return;
            }
        };
        println!("{:?}", kwp.query_ecu_data());
        println!("Please unplug NAG");
        std::thread::sleep(std::time::Duration::from_millis(5000));
        let failable = kwp.with_kwp(|k| k.kwp_read_daimler_identification());
        assert!(failable.is_err());
        println!("{:?}", failable);
        let e = failable.err().unwrap();
        if let DiagError::ECUError { code: _, def: _ } = e {
        } else {
            for i in 0..5 {
                println!("Reconnect attempt {}/5", i + 1);
                match kwp.try_reconnect() {
                    Ok(_) => {
                        println!("Reconnect OK!");
                        break;
                    }
                    Err(e) => {
                        println!("Reconnect failed! {e}!");
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(2000));
            }
        }
        let must_ok = kwp.with_kwp(|k| k.kwp_read_daimler_identification());
        assert!(must_ok.is_ok());
    }
}
