use core::fmt;
use std::{
    borrow::{Borrow, BorrowMut},
    sync::{Arc, Mutex, TryLockError, mpsc::{self}},
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

#[cfg(target_os="linux")]
use ecu_diagnostics::hardware::socketcan::{SocketCanDevice, SocketCanScanner};

use crate::hw::{
    usb::{EspLogMessage, Nag52USB},
    usb_scanner::Nag52UsbScanner,
};

use self::device_modes::TcuDeviceMode;

pub mod flash;
pub mod ident;
pub mod settings;
pub mod nvs;
pub mod device_modes;
pub mod module_settings_flash_store;
pub mod calibration;
pub mod memory;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AdapterType {
    USB,
    Passthru,
    #[cfg(target_os="linux")]
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

    pub fn data(&self) -> Option<&T> {
        if let Self::LoadOk(data) = self {
            Some(data)
        } else {
            None
        }
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
    #[cfg(target_os="linux")]
    SocketCAN(SocketCanDevice),
}

impl fmt::Debug for AdapterHw {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usb(_) => f.debug_tuple("Usb").finish(),
            Self::Passthru(_) => f.debug_tuple("Passthru").finish(),
            #[cfg(target_os="linux")]
            Self::SocketCAN(_) => f.debug_tuple("SocketCAN").finish(),
        }
    }
}

impl AdapterHw {
    pub fn try_connect(info: &HardwareInfo, ty: AdapterType) -> HardwareResult<Self> {
        Ok(match ty {
            AdapterType::USB => Self::Usb(Nag52USB::try_connect(info)?),
            AdapterType::Passthru => Self::Passthru(PassthruDevice::try_connect(info)?),
            #[cfg(target_os="linux")]
            AdapterType::SocketCAN => Self::SocketCAN(SocketCanDevice::try_connect(info)?),
        })
    }

    fn get_type(&self) -> AdapterType {
        match self {
            Self::Usb(_) => AdapterType::USB,
            Self::Passthru(_) => AdapterType::Passthru,
            #[cfg(target_os="linux")]
            Self::SocketCAN(_) => AdapterType::SocketCAN,
        }
    }

    pub fn create_isotp_channel(&mut self) -> HardwareResult<Box<dyn IsoTPChannel>> {
        match self.borrow_mut() {
            Self::Usb(u) => u.create_iso_tp_channel(),
            Self::Passthru(p) => p.create_iso_tp_channel(),
            #[cfg(target_os="linux")]
            Self::SocketCAN(s) => s.create_iso_tp_channel(),
        }
    }

    pub fn get_hw_info(&self) -> HardwareInfo {
        match self {
            Self::Usb(u) => u.get_info().clone(),
            Self::Passthru(p) => p.get_info().clone(),
            #[cfg(target_os="linux")]
            Self::SocketCAN(s) => s.get_info().clone(),
        }
    }

    pub fn get_data_rate(&self) -> Option<(u32, u32)> {
        match self {
            Self::Usb(u) => u.get_data_rate(),
            Self::Passthru(p) => p.get_data_rate(),
            #[cfg(target_os="linux")]
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

#[cfg(target_os="linux")]
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


/// Locks a mutex, recovering the inner value if a previous holder panicked.
///
/// Poisoning carries no meaning for a channel endpoint, so it must not become a panic.
fn lock_recover<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Debug, Clone)]
pub struct NagAppLoggerInner {
    // Guarded so this type is `Send + Sync` by construction rather than by assertion.
    sender: Arc<Mutex<mpsc::Sender<ServerEvent>>>
}

impl NagAppLoggerInner {
    pub fn new() -> (Self, mpsc::Receiver<ServerEvent>) {
        let (tx, rx) = mpsc::channel::<ServerEvent>();
        (
            Self {
                sender: Arc::new(Mutex::new(tx))
            },
            rx
        )
    }
}

impl DiagServerLogger for NagAppLoggerInner {
    fn on_event(&self, evt: ServerEvent) {
        // The UI may have stopped draining events; dropping them is expected.
        let _ = lock_recover(&self.sender).send(evt);
    }
}

#[derive(Clone, Debug)]
pub struct NagAppLogger {
    // `mpsc::Receiver` is `!Sync`; the UI thread polls this while diag worker threads are
    // running, so it needs real synchronisation rather than an `unsafe impl`.
    recv: Arc<Mutex<mpsc::Receiver<ServerEvent>>>
}

impl NagAppLogger {
    pub fn new() -> (Self, NagAppLoggerInner) {
        let (inner, recv) = NagAppLoggerInner::new();
        (
            Self {
                recv: Arc::new(Mutex::new(recv))
            },
            inner
        )
    }
}

#[derive(Debug, Clone)]
pub struct Nag52Diag {
    device_mode: TcuDeviceMode,
    info: HardwareInfo,
    endpoint: Option<AdapterHw>,
    endpoint_type: AdapterType,
    server: Option<Arc<DynamicDiagSession>>,
    logger: NagAppLogger,
    server_mutex: Arc<Mutex<()>>
}

// `Nag52Diag` derives `Send + Sync` on its own now that the logger channel endpoints are
// mutex-guarded, so the previous `unsafe impl Send`/`unsafe impl Sync` are gone: the
// compiler verifies the sharing model instead of us asserting it.

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

        #[cfg(target_os="linux")]
        if let AdapterHw::SocketCAN(_) = hw {
            channel_cfg.block_size = 8;
            channel_cfg.st_min = 10;
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

        let mut isotp_channel = hw.create_isotp_channel().map_err(|e| DiagError::from(Arc::new(e)))?;
        isotp_channel.set_iso_tp_cfg(channel_cfg)?;
        isotp_channel.set_ids(0x07E1, 0x07E9)?;
        isotp_channel.open()?;

        let kwp = DynamicDiagSession::new(
            protocol,
            isotp_channel,
            basic_opts,
            Some(adv_opts),
            inner_logger
        )?;

        let mut s = Self {
            device_mode: TcuDeviceMode::NORMAL,
            info: hw.get_hw_info(),
            endpoint_type: hw.get_type(),
            endpoint: Some(hw),
            server: Some(Arc::new(kwp)),
            logger,
            server_mutex: Arc::new(Mutex::new(()))
        };

        if let Ok(mode) = s.read_device_mode() {
            s.device_mode = mode;
        }
        Ok(s)
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
        let _guard = self.server_mutex.lock().map_err(|_| DiagError::ServerNotRunning)?;
        match self.server.borrow() {
            None => Err(DiagError::from(Arc::new(HardwareError::DeviceNotOpen))),
            Some(s) => kwp_fn(&s),
        }
    }

    pub fn try_with_kwp<F, X>(&self, mut kwp_fn: F) -> DiagServerResult<Option<X>>
    where
        F: FnMut(&DynamicDiagSession) -> DiagServerResult<X>,
    {
        let _guard = match self.server_mutex.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::WouldBlock) => return Ok(None),
            Err(TryLockError::Poisoned(_)) => return Err(DiagError::ServerNotRunning),
        };
        match self.server.borrow() {
            None => Err(DiagError::from(Arc::new(HardwareError::DeviceNotOpen))),
            Some(s) => kwp_fn(&s).map(Some),
        }
    }

    pub fn get_data_rate(&self) -> Option<(u32, u32)> {
        self.endpoint.as_ref().map(|x| x.get_data_rate()).unwrap_or_else(|| None)
    }

    pub fn read_log_msg(&self) -> Option<EspLogMessage> {
        self.endpoint.as_ref().map(|x| x.read_log_msg()).flatten()
    }

    pub fn read_can_msg(&self) -> Option<CanFrame> {
        let hw = self.endpoint.as_ref()?;
        if let AdapterHw::Usb(usb) = hw {
            return usb.read_can();
        }
        None
    }

    pub fn clear_can_buffer(&self) {
        if let Some(hw) = self.endpoint.as_ref() {
            if let AdapterHw::Usb(usb) = hw {
                while usb.read_can().is_some(){}
            }
        }
    }

    pub fn has_logger(&self) -> bool {
        self.endpoint_type == AdapterType::USB
    }

    pub fn get_server_event(&self) -> Option<ServerEvent> {
        lock_recover(&self.logger.recv).try_recv().ok()
    }

}

/// The app clones `Nag52Diag` into worker threads and touches it from the UI thread, so
/// these bounds must hold. They are asserted here rather than forced with `unsafe impl`,
/// so that a future field which is not thread-safe becomes a compile error.
const _: () = {
    static_assertions::assert_impl_all!(Nag52Diag: Send, Sync);
    static_assertions::assert_impl_all!(Nag52USB: Send, Sync);
    static_assertions::assert_impl_all!(NagAppLoggerInner: Send, Sync);
};

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
