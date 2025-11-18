# Ultimate-NAG52 (Config app)

The configuration app for the Ultimate-NAG52 TCU

## Installation

### macOS
After downloading the `.app` bundle, you may need to remove the quarantine attribute due to the app not being signed with an Apple Developer Certificate:

```bash
xattr -cr Ultimate-NAG52-Config-App.app
open Ultimate-NAG52-Config-App.app
```

Alternatively, right-click the app and select "Open" (first time only).

### Windows
Run the `.exe` file directly.

### Linux
Make the AppImage executable and run:
```bash
chmod +x Ultimate-NAG52-Config-App*.AppImage
./Ultimate-NAG52-Config-App*.AppImage
```

## Starting version 1.0.8
to load the TCU Settings page, you will need `MODULE_SETTINGS.yml` file that is shipped with the TCUs firmware on Github!

## Repo structure

* Backend - Backend for ECU diagnostics
* config_app - Main configuration suite UI

## Branch names

The branches of this repository will follow the same branch names as the TCU firmware. Builds will only be avaliable for the main and dev branch. Other branches will need to be compiled manually