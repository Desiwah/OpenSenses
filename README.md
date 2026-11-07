# OpenSenses

Keyboard lighting, fan control, GPU overclock levels and temperature
monitoring for the Acer Predator Helios 300 (PH317-53), on Windows and Linux.
A replacement for PredatorSense, which Acer no longer offers for this laptop.

Not affiliated with Acer.

## What it does

- **Lighting:** the four keyboard zones in any colour, each on or off, plus
  breathing, neon, wave, shifting and zoom with speed, brightness and
  direction. A temperature mode turns the keyboard from blue to red as the
  CPU heats up.
- **Fans:** Auto, Max, or a fixed speed per fan. If a chip gets too hot in
  custom mode (CPU 95 °C or GPU 90 °C), both fans go to Max until it cools
  below 80 °C.
- **GPU:** PredatorSense's Normal, Faster and Turbo overclock levels.
- **Monitor:** CPU, GPU and system temperatures and both fan speeds.
- **Keeps your settings:** a background service applies them at startup and
  after sleep, and puts them back if a Windows update or anything else
  changes them.

## Install

### Windows

Download the installer from Releases and run it. It installs the app and the
`OpenSenses` background service (it needs administrator rights once).

### Linux

Install the `.deb` or run the AppImage. Lighting needs the
[facer kernel module](https://github.com/JafarAkhondali/acer-predator-turbo-and-rgb-keyboard-linux-module);
temperatures and fan speeds come from the kernel's `acer` hwmon sensors. Then
set up the service from the app, or run `sudo opensenses-service install`.

Fan modes and GPU levels are Windows-only for now: the Linux module doesn't
expose them.

## How it works

The app runs as your normal user and talks to the background service over a
local pipe that only logged-in users on this computer can open. The service
runs with administrator rights and is the only part that touches the
hardware; it accepts settings only, checks their ranges, and never runs
arbitrary commands.

How the firmware is controlled is documented in [docs/protocol.md](docs/protocol.md).

## Building

Needs Rust, Node.js 22 and, on Linux, the WebKitGTK development libraries.

```sh
cargo build --release -p opensenses-service
mkdir -p src-tauri/bin && cp target/release/opensenses-service src-tauri/bin/
npm ci
npx tauri build
```

On Windows copy `opensenses-service.exe` instead. GitHub Actions builds the
Windows installer and the Linux packages on every push.

## License

MIT
