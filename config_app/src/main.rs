use std::sync::Arc;

use eframe::{egui::IconData, NativeOptions, Renderer};
use ui::launcher::Launcher;

mod plot_backend;
mod ui;
mod window;
mod ghapi;

// IMPORTANT. On windows, only the i686-pc-windows-msvc target is supported (Due to limitations with J2534 and D-PDU!
#[cfg(all(target_arch = "x86_64", target_os = "windows"))]
compile_error!("Windows can ONLY be built using the i686-pc-windows-msvc target!");

fn main() {
    env_logger::init();

    //#[cfg(target_os="linux")]
    //std::env::set_var("WINIT_UNIX_BACKEND", "x11");

    let mut app = window::MainWindow::new();
    app.add_new_page(Box::new(Launcher::new()));
    let mut native_options = NativeOptions::default();
    native_options.vsync = true;
    native_options.window_builder = Some(
        Box::new(|mut wb| {
            let icon = image::load_from_memory(include_bytes!("../icon.png"))
                .unwrap()
                .to_rgba8();
            let (icon_w, icon_h) = icon.dimensions();

            wb.inner_size = Some((1280.0, 720.0).into());
            wb.icon = Some(Arc::new(IconData {
                rgba: icon.into_raw(),
                width: icon_w,
                height: icon_h,
            }));
            wb
        })
    );
    #[cfg(windows)]
    {
        native_options.renderer = Renderer::Wgpu;
    }
    #[cfg(unix)]
    {
        native_options.renderer = Renderer::Glow;
    }
    eframe::run_native(
        "Ultimate NAG52 config suite",
        native_options,
        Box::new(|cc| Ok(Box::new(app))),
    );
}
