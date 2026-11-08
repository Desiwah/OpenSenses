# OpenSenses

Keyboard lighting, fan control, GPU overclock levels and temperature
monitoring for Acer Predator laptops, on Windows and Linux. An unofficial,
open source alternative to Acer's PredatorSense.

> **Experimental software, use at your own risk.** Please read
> [Before you use it](#before-you-use-it) first.

## Why I made this

I have an Acer Predator Helios 300 (PH317-53). PredatorSense stopped working
for me: the version I had installed wouldn't open anymore, Windows updates
kept resetting its settings, and Acer has removed the download for my model
from its website, so there was no way to get it back. Without it there was no
way to control the keyboard lighting, the fans or the GPU levels.

So I built my own. I also wanted it to run on Linux, to make it easier for
people with these laptops who use Linux, or who are thinking of switching, to
keep these features. Keep in mind the Linux version hasn't been tested on real
hardware yet (see below), so it may or may not work there.

## Before you use it

Please read this before installing.

- **It's vibe coded.** I built this with heavy help from an AI coding
  assistant. I'm not a professional driver or firmware developer, and the code
  hasn't been reviewed by anyone who is.
- **It changes low-level hardware settings.** OpenSenses talks directly to
  the laptop's firmware and, on Linux, installs a kernel driver. It changes
  fan behaviour, GPU clock speeds and other firmware settings. Mistakes here
  could cause overheating, crashes, instability, data loss or damage to your
  hardware, and could affect your warranty.
- **Most of it I can't test myself.** I only own one laptop, a PH317-53, and
  only tested it on Windows. Every other model, and all of the Linux version,
  follow data and documentation I couldn't check on real hardware; the app
  marks those features "(experimental)". They may not work, or may behave
  differently than expected.
- **No warranty, no responsibility.** This software is provided "as is",
  without warranty of any kind (see the [license](LICENSE)). I'm not
  responsible for any damage, data loss, hardware failure, voided warranty or
  any other problem that comes from using it. You use it entirely at your own
  risk. If you're not comfortable with that, don't install it.
- **Not affiliated with Acer or NVIDIA.** This is an independent hobby
  project. It isn't made, endorsed or supported by Acer, NVIDIA or anyone
  else. Acer, Predator, PredatorSense, Helios, Triton and Nitro are trademarks
  of Acer Inc.; NVIDIA is a trademark of NVIDIA Corporation. They're mentioned
  only to say which hardware this works with.
- **No Acer code.** OpenSenses doesn't include or redistribute any part of
  PredatorSense. It was written from scratch by working out which firmware
  settings the laptops use, so that people can keep using hardware they own.
  How that was done is described in [docs/protocol.md](docs/protocol.md).
- **For problems with your laptop, contact Acer.** Don't contact Acer about
  OpenSenses, and don't expect them to support it.

If something goes wrong, uninstall OpenSenses; that stops it from changing
anything. Some settings it already changed are stored in the laptop's firmware
and stay until you change them back.

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
`OpenSenses` background service (it needs administrator rights once). Close
PredatorSense first if it's still installed, so the two don't fight over the
same settings.

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

## Testing and feedback

If you have one of the other models, or try it on Linux, please open an issue
and say what worked and what didn't. That's the only way the "(experimental)"
labels can come off. Please include your laptop model and, on Linux, your
distribution.

## License

The app and the background service are under the [MIT license](LICENSE). The
Linux kernel driver in `driver/` is under the GNU GPL, version 2 or later
([driver/LICENSE](driver/LICENSE)), as Linux kernel drivers have to be. Both
licenses come without any warranty.
