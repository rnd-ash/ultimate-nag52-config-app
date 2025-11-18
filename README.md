# Ultimate-NAG52 (Config app)

The configuration app for the Ultimate-NAG52 TCU

## Installation

### macOS

#### ⚠️ IMPORTANT: App is not code-signed
This app is not signed with an Apple Developer Certificate. macOS will show a security warning when you first try to open it.

**DO NOT double-click the app on first launch** - it will show "app is damaged" error.

#### Installation Steps:
1. Download `config_app.dmg` from [Releases](https://github.com/rnd-ash/ultimate-nag52-config-app/releases)
2. Open the downloaded DMG file
3. Drag `Ultimate-NAG52-Config-App.app` to your **Applications** folder
4. **Right-click** (or Control-click) on the app in Applications
5. Select **"Open"** from the menu
6. Click **"Open"** in the security dialog that appears

**This is only needed the first time.** After that, you can launch the app normally.

<details>
<summary>Alternative: Using Terminal (for advanced users)</summary>

If you prefer using the terminal:
```bash
xattr -cr /Applications/Ultimate-NAG52-Config-App.app
open /Applications/Ultimate-NAG52-Config-App.app
```
</details>

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