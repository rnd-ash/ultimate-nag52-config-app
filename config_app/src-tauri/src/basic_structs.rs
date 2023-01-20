use serde::{Serialize, Deserialize};


#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeviceType {
    SocketCAN(String),
    Usb(String),
    Passthru(String)
}