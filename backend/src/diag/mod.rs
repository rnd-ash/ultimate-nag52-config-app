use core::fmt;
use std::{
    borrow::BorrowMut,
    sync::{Arc, Mutex},
};

use ecu_diagnostics::channel::*;
use ecu_diagnostics::hardware::{
    passthru::*, Hardware, HardwareError, HardwareInfo, HardwareResult, HardwareScanner,
};
use ecu_diagnostics::{kwp2000::*, DiagServerResult};

#[cfg(unix)]
use ecu_diagnostics::hardware::socketcan::{SocketCanDevice, SocketCanScanner};

use crate::hw::{
    usb::{EspLogMessage, Nag52USB},
    usb_scanner::Nag52UsbScanner,
};
pub mod ident;
pub mod flash;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AdapterType {
    USB,
    Passthru,
    #[cfg(unix)]
    SocketCAN,
}

#[derive(Clone)]
pub enum AdapterHw {
    Usb(Arc<Mutex<Nag52USB>>),
    Passthru(Arc<Mutex<PassthruDevice>>),
    #[cfg(unix)]
    SocketCAN(Arc<Mutex<SocketCanDevice>>),
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

    pub fn create_isotp_channel(&self) -> HardwareResult<Box<dyn IsoTPChannel>> {
        match self {
            Self::Usb(u) => Hardware::create_iso_tp_channel(u.clone()),
            Self::Passthru(p) => Hardware::create_iso_tp_channel(p.clone()),
            #[cfg(unix)]
            Self::SocketCAN(s) => Hardware::create_iso_tp_channel(s.clone()),
        }
    }

    pub fn get_hw_info(&self) -> HardwareInfo {
        match self {
            Self::Usb(u) => u.lock().unwrap().get_info().clone(),
            Self::Passthru(p) => p.lock().unwrap().get_info().clone(),
            #[cfg(unix)]
            Self::SocketCAN(s) => s.lock().unwrap().get_info().clone(),
        }
    }
}

pub trait Nag52Endpoint: Hardware {
    fn read_log_message(this: Arc<Mutex<Self>>) -> Option<EspLogMessage>;
    fn is_connected(&self) -> bool;
    fn try_connect(info: &HardwareInfo) -> HardwareResult<Arc<Mutex<Self>>>;
    fn get_device_desc(this: Arc<Mutex<Self>>) -> String;
}

#[cfg(unix)]
impl Nag52Endpoint for SocketCanDevice {
    fn read_log_message(_this: Arc<Mutex<Self>>) -> Option<EspLogMessage> {
        None
    }

    fn is_connected(&self) -> bool {
        self.is_iso_tp_channel_open()
    }

    fn try_connect(info: &HardwareInfo) -> HardwareResult<Arc<Mutex<Self>>> {
        SocketCanScanner::new().open_device_by_name(&info.name)
    }

    fn get_device_desc(this: Arc<Mutex<Self>>) -> String {
        this.lock().unwrap().get_info().name.clone()
    }
}

impl Nag52Endpoint for PassthruDevice {
    fn read_log_message(_this: Arc<Mutex<Self>>) -> Option<EspLogMessage> {
        None
    }

    fn is_connected(&self) -> bool {
        self.is_iso_tp_channel_open()
    }

    fn try_connect(info: &HardwareInfo) -> HardwareResult<Arc<Mutex<Self>>> {
        PassthruScanner::new().open_device_by_name(&info.name)
    }

    fn get_device_desc(this: Arc<Mutex<Self>>) -> String {
        this.lock().unwrap().get_info().name.clone()
    }
}

impl Nag52Endpoint for Nag52USB {
    fn read_log_message(this: Arc<Mutex<Self>>) -> Option<EspLogMessage> {
        this.lock().unwrap().get_log_msg()
    }

    fn is_connected(&self) -> bool {
        self.is_connected()
    }

    fn try_connect(info: &HardwareInfo) -> HardwareResult<Arc<Mutex<Self>>> {
        Nag52UsbScanner::new().open_device_by_name(&info.name)
    }

    fn get_device_desc(this: Arc<Mutex<Self>>) -> String {
        let info_name = this.lock().unwrap().get_info().name.clone();
        format!("Ultimate-NAG52 USB on {}", info_name)
    }
}

#[derive(Debug, Clone)]
pub struct Nag52Diag {
    info: HardwareInfo,
    endpoint: Option<AdapterHw>,
    endpoint_type: AdapterType,
    server: Option<Arc<Mutex<Kwp2000DiagnosticServer>>>,
}

unsafe impl Sync for Nag52Diag {}
unsafe impl Send for Nag52Diag {}

impl Nag52Diag {
    pub fn new(endpoint_type: AdapterHw) -> DiagServerResult<Self> {
        let iso_tp = endpoint_type.create_isotp_channel()?;

        let channel_cfg = IsoTPSettings {
            block_size: 0,
            st_min: 0,
            extended_addresses: None,
            pad_frame: true,
            can_speed: 500_000,
            can_use_ext_addr: false,
        };

        let server_settings = Kwp2000ServerOptions {
            send_id: 0x07E1,
            recv_id: 0x07E9,
            read_timeout_ms: 2500,
            write_timeout_ms: 2500,
            global_tp_id: 0,
            tester_present_interval_ms: 2000,
            tester_present_require_response: true,
            global_session_control: false,
        };

        let kwp = Kwp2000DiagnosticServer::new_over_iso_tp(
            server_settings,
            iso_tp,
            channel_cfg,
            Kwp2000VoidHandler,
        )?;

        let info = endpoint_type.get_hw_info();
        Ok(Self {
            info,
            endpoint_type: endpoint_type.get_type(),
            endpoint: Some(endpoint_type),
            server: Some(Arc::new(Mutex::new(kwp))),
        })
    }

    pub fn try_reconnect(&mut self) -> DiagServerResult<()> {
        {
            let _ = self.server.take();
            let _ = self.endpoint.take();
        }
        // Now try to reconnect
        println!("Trying to find {}", self.info.name);
        let dev = AdapterHw::try_connect(&self.info, self.endpoint_type)?;
        *self = Self::new(dev)?;
        Ok(())
    }

    pub fn with_kwp<F, X>(&mut self, mut kwp_fn: F) -> DiagServerResult<X>
    where
        F: FnMut(&mut Kwp2000DiagnosticServer) -> DiagServerResult<X>,
    {
        match self.server.borrow_mut() {
            None => Err(HardwareError::DeviceNotOpen.into()),
            Some(s) => {
                let mut lock = s.lock().unwrap();
                kwp_fn(&mut lock)
            }
        }
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
        let failable = kwp.with_kwp(|k| k.read_daimler_identification());
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
        let must_ok = kwp.with_kwp(|k| k.read_daimler_identification());
        assert!(must_ok.is_ok());
    }
}
