use std::{
    ops::RangeInclusive,
    sync::{mpsc, Arc, Mutex},
};

use backend::{ecu_diagnostics::{hardware::{socketcan::SocketCanScanner, passthru::PassthruScanner, HardwareResult, Hardware, HardwareScanner, HardwareInfo}, DiagServerResult, DiagError}, hw::usb_scanner::Nag52UsbScanner, diag::{AdapterHw, AdapterType, Nag52Diag}};
use eframe::egui;
use eframe::egui::*;

use crate::{
    ui::main::MainPage,
    window::{InterfacePage, PageAction},
};

use super::widgets::range_display::range_display;

type ScanResult = std::result::Result<Vec<String>, String>;

pub struct Launcher {
    selected: String,
    old_selected: String,
    launch_err: Option<String>,
    usb_scanner: Nag52UsbScanner,
    pt_scanner: PassthruScanner,
    #[cfg(unix)]
    scan_scanner: SocketCanScanner,
    selected_device: String,
    curr_api_type: AdapterType,
    curr_dev_list: Vec<HardwareInfo>
}

impl Launcher {
    pub fn new() -> Self {
        Self {
            selected: "".into(),
            old_selected: "".into(),
            launch_err: None,
            usb_scanner: Nag52UsbScanner::new(),
            pt_scanner: PassthruScanner::new(),
            #[cfg(unix)]
            scan_scanner: SocketCanScanner::new(),
            selected_device: String::new(),
            curr_api_type: AdapterType::USB,
            curr_dev_list: vec![]
        }
    }
}

impl Launcher {
    pub fn open_device(&self, name: &str) -> DiagServerResult<Nag52Diag> {
        println!("Opening '{}'", name);
        let hw_info = self.curr_dev_list.iter().find(|x| x.name == name)
            .ok_or(DiagError::ParameterInvalid)?;
        let hw = AdapterHw::try_connect(hw_info, self.curr_api_type)?;
        Nag52Diag::new(hw)
    }

    pub fn get_device_list<T, X: Hardware>(scanner: &T) -> Vec<HardwareInfo>
    where
        T: HardwareScanner<X>,
    {
        return scanner.list_devices()
            
    }
}

impl InterfacePage for Launcher {
    fn make_ui(&mut self, ui: &mut Ui, frame: &eframe::Frame) -> crate::window::PageAction {
        ui.label("Ultimate-Nag52 configuration utility!");
        ui.label(
            "Please plug in your TCM via USB and select the correct port, or select another API",
        );

        ui.radio_value(&mut self.curr_api_type, AdapterType::USB, "USB connection");
        ui.radio_value(
            &mut self.curr_api_type,
            AdapterType::Passthru,
            "Passthru OBD adapter",
        );
        #[cfg(unix)]
        {
            ui.radio_value(
                &mut self.curr_api_type,
                AdapterType::SocketCAN,
                "SocketCAN device",
            );
        }
        ui.heading("Devices");

        let dev_list = match self.curr_api_type {
            AdapterType::Passthru => Self::get_device_list(&self.pt_scanner),
            #[cfg(unix)]
            AdapterType::SocketCAN => Self::get_device_list(&self.scan_scanner),
            AdapterType::USB => Self::get_device_list(&self.usb_scanner),
        };
        self.curr_dev_list = dev_list.clone();

        if dev_list.len() == 0 {
        } else {
            egui::ComboBox::from_label("Select device")
                .width(400.0)
                .selected_text(&self.selected_device)
                .show_ui(ui, |cb_ui| {
                    for dev in dev_list {
                        cb_ui.selectable_value(&mut self.selected_device, dev.name.clone(), dev.name);
                    }
                });
        }

        if !self.selected_device.is_empty() && ui.button("Launch configuration app").clicked() {
            match self.open_device(&self.selected_device) {
                Ok(mut dev) => {
                    return PageAction::Overwrite(Box::new(MainPage::new(dev)));
                }
                Err(e) => self.launch_err = Some(format!("Cannot open device: {}", e)),
            }
        }

        if ui.button("Refresh device list").clicked() {
            self.pt_scanner = PassthruScanner::new();
            self.usb_scanner = Nag52UsbScanner::new();
            #[cfg(unix)]
            {
                self.scan_scanner = SocketCanScanner::new();
            }
            self.selected_device.clear();
        }

        if let Some(e) = &self.launch_err {
            ui.label(RichText::new(format!("Error: {}", e)).color(Color32::from_rgb(255, 0, 0)));
        }

        range_display(ui, 65.0, 50.0, 70.0, 0.0, 100.0);

        crate::window::PageAction::None
    }

    fn get_title(&self) -> &'static str {
        "Ultimate-NAG52 configuration utility (Launcher)"
    }

    fn get_status_bar(&self) -> Option<Box<dyn crate::window::StatusBar>> {
        None
    }
}
