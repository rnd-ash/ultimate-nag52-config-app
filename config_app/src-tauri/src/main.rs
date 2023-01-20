#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use backend::{hw::usb_scanner::Nag52UsbScanner, ecu_diagnostics::hardware::{socketcan::SocketCanScanner, passthru::PassthruScanner}};
use basic_structs::DeviceType;
pub mod basic_structs;
use backend::ecu_diagnostics::{hardware::HardwareScanner};


// Learn more about Tauri commands at https://tauri.app/v1/guides/features/command
#[tauri::command]
fn list_devices() -> Vec<DeviceType> {
    let mut res = Vec::new();
    for hw in Nag52UsbScanner::new().list_devices() {
        res.push(DeviceType::Usb(hw.name))   
    }
    for hw in SocketCanScanner::new().list_devices() {
        res.push(DeviceType::SocketCAN(hw.name))   
    }
    for hw in PassthruScanner::new().list_devices() {
        res.push(DeviceType::Passthru(hw.name))   
    }
    res
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(
            tauri::generate_handler![
                list_devices
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
