# 1.3.1 (07/01/25)
* Update EGS DB to fix crashing with various EGS51 calibrations
* Update EGUI to 0.30

# 1.3.0 (08/12/24)
* Added in support for EGS calibration
* Added support for new TCC zener addon PCB
* Added shift algorithm data diagnostics page

# 1.0.7 (08/08/23)
* Move diagnostic executor to seperate thread
* Migrate to diag_server_unified ecu_diagnostics branch
* Show diagnostic mode on status bar
* Chart rendering at 60fps - Along with linear interpolation!
* Initial SCN Configuration wizard:
    1. Save/Load from YML files
    2. Write/Read program settings
    3. Wiki integration
* Add support for ResponsePending ECU response (useful when flashing)
* Fix bug where App would crash on invalid TCU Config size (Instead present the user with some help)
* Give the home page a makeover!
* Add useful wiki links for configuration page
* Show data rate between TCU and App
* Allow you to save log files to disk
* Packet trace view (For diagnostic debugging)
* Add more RLI's for diagnostics page:
    1. Clutch speeds
    2. Clutch pack velocities
    3. Show torque request info on CAN Rx data
* RLI graphing now takes up more of the page for better readability
* Add multiple series charting at the same time

# 1.0.3 (27/2/23)
* Removed "UNDER DEVELOPMENT" watermark for V1.3 PCB

# 1.0.2 (20/2/23)
* Initial release
