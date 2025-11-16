
use ecu_diagnostics::hardware::{HardwareCapabilities, HardwareError, HardwareInfo};
use serialport::SerialPortType;

use super::usb::Nag52USB;

pub struct Nag52UsbScanner {
    ports: Vec<(HardwareInfo, serialport::UsbPortInfo)>,
}

impl Nag52UsbScanner {
    pub fn new() -> Self {
        let tmp = match serialport::available_ports() {
            Ok(ports) => {
                let mut ret = Vec::new();
                
                for p in ports {
                    if let SerialPortType::UsbPort(usb_info) = p.port_type {
                        ret.push((
                            HardwareInfo {
                                name: p.port_name.clone(),
                                vendor: Some(usb_info.manufacturer.clone().unwrap_or_default()),
                                device_fw_version: None,
                                api_version: None,
                                library_version: None,
                                library_location: None,
                                capabilities: HardwareCapabilities {
                                    iso_tp: true,
                                    can: false,
                                    kline: false,
                                    kline_kwp: false,
                                    sae_j1850: false,
                                    sci: false,
                                    ip: false,
                                },
                            },
                            usb_info
                        ));
                    }
                }
                
                ret

            },
            Err(_) => Vec::new(),
        };
        Self { ports: tmp }
    }
}

impl ecu_diagnostics::hardware::HardwareScanner<Nag52USB> for Nag52UsbScanner {
    fn list_devices(&self) -> Vec<ecu_diagnostics::hardware::HardwareInfo> {
        let mut ports = self.ports.iter().map(|(info, _)| info.clone()).collect::<Vec<HardwareInfo>>();

        #[cfg(target_os="linux")]
        {
            ports = ports.into_iter().filter(|x| x.name.contains("USB")).collect();
        }

        ports
    }

    fn open_device_by_index(
        &self,
        idx: usize,
    ) -> ecu_diagnostics::hardware::HardwareResult<Nag52USB> {
        match self.ports.get(idx) {
            Some((p, port)) => Ok(Nag52USB::new(&p.name, port.clone())?),
            None => Err(HardwareError::DeviceNotFound),
        }
    }

    fn open_device_by_name(
        &self,
        name: &str,
    ) -> ecu_diagnostics::hardware::HardwareResult<Nag52USB> {
        match self.ports.iter().find(|(i, _p)| i.name == name) {
            Some((p, port)) => Ok(Nag52USB::new(&p.name, port.clone())?),
            None => Err(HardwareError::DeviceNotFound),
        }
    }
}
