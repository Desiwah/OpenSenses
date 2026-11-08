# OpenSenses

Keyboard lighting, fan control, GPU overclock levels and temperature
monitoring for Acer Predator laptops, on Windows and Linux. A replacement for
PredatorSense, which Acer no longer offers for older models.

Not affiliated with Acer.

## What it does

- **Lighting:** the four keyboard zones in any colour, each on or off, plus
  breathing, neon, wave, shifting and zoom with speed, brightness and
  direction. A temperature mode turns the keyboard from blue to red as the
  CPU heats up.
- **Fans:** Auto, Max, or a fixed speed per fan, plus CoolBoost for extra
  cooling in Auto mode. If a chip gets too hot in
  custom mode (CPU 95 °C or GPU 90 °C), both fans go to Max until it cools
  below 80 °C.
- **GPU:** PredatorSense's Normal, Faster and Turbo overclock levels.
- **Monitor:** CPU, GPU and system temperatures and both fan speeds, in °C
  or °F.
- **Games (GameSync):** give a game its own lighting, fan mode, GPU level and
  Windows-key setting. They switch in when the game starts and back when it
  closes, whether or not the app is open.
- **Settings:** keyboard lighting off after 30 seconds idle, LCD overdrive,
  Windows and Menu keys on or off, boot animation and sound, and Sticky Keys.
- **Keeps your settings:** a background service applies them at startup and
  after sleep, and puts them back if a Windows update or anything else
  changes them.

## Supported laptops

OpenSenses reads the model from the BIOS and shows only what that laptop can
do, using the per-model data PredatorSense 3.00 shipped with:

| Model | Lighting | Fans | GPU levels | Settings |
| ----- | -------- | ---- | ---------- | -------- |
| Helios 300 (PH315-52, PH317-53) | 4 zones | CPU, GPU | 3 | backlight timeout, LCD overdrive, Windows key, boot animation |
| Triton 300 (PT315-51) | 4 zones | CPU, GPU | 3 | same as Helios 300 |
| Triton 500 (PT515-51) | 4 zones | CPU, 2 × GPU | 3 | same, plus NVIDIA-only mode |
| Helios 500 (PH517-51, PH517-61) | 4 zones | CPU, GPU | 3 (Intel model) | Windows key |
| Helios 700 (PH717-71) | per-key (planned) | CPU, GPU | 4 | boot animation |
| Triton 900 (PT917-71) | per-key (planned) | CPU, GPU | 3 | boot animation |

The PH317-53 is tested on real hardware; the others follow PredatorSense's own
data for them, and the app marks their features "(experimental)" until someone
confirms them. The Linux side is experimental too. Other Predator and Nitro laptops with the same firmware
interface work too: OpenSenses asks the firmware what it answers to and
enables those features. GPU levels are offered only for GPUs PredatorSense had
overclock numbers for.

Still to come: per-key keyboards, CPU overclocking and power limits, and
programmable hotkeys on the models that have them.

## Install

### Windows

Download the installer from Releases and run it. It installs the app and the
`OpenSenses` background service (it needs administrator rights once).

### Linux

Linux has every feature Windows has. The laptop's firmware is the same on both
systems, so OpenSenses sends it exactly the same commands; on Linux they go
through a small kernel driver that comes with OpenSenses (`driver/`), and GPU
overclocking goes through NVIDIA's NVML instead of NVAPI.

| Distribution | Package |
| ------------ | ------- |
| Ubuntu, Debian, Linux Mint, Pop!_OS, Zorin | `.deb` |
| Fedora, Nobara, openSUSE | `.rpm` |
| Arch, Manjaro, EndeavourOS and others | AppImage |

The `.deb` and `.rpm` set up the background service and build the driver with
DKMS, which rebuilds it after kernel updates. With the AppImage, open the app
and click **Set up service**; it builds the driver too (with DKMS when it's
installed, otherwise directly for the running kernel). Either way the driver
needs your kernel's headers (`linux-headers-…` / `kernel-devel`). With Secure
Boot on, DKMS asks you to enroll its signing key once.

On image-based systems (Bazzite, Fedora Silverblue, SteamOS) extra drivers
can't be built on the system itself, so the driver has to be included in the
system image; until it is, OpenSenses shows temperatures only.

Linux support hasn't been tested on real hardware yet, so the app marks it
"(experimental)".

## How it works

The app runs as your normal user and talks to the background service over a
local pipe that only logged-in users on this computer can open. The service
runs with administrator rights and is the only part that touches the
hardware; it accepts settings only, checks their ranges, and never runs
arbitrary commands.

How the firmware is controlled is documented in [docs/protocol.md](docs/protocol.md).

On Linux the service talks to the firmware through `/dev/opensenses`, which
only root can open. The driver behind it lets through only the firmware
methods listed in the protocol notes, nothing else.

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
