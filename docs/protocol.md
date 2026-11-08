# How the hardware is controlled

Notes on the firmware interface OpenSenses uses on the Acer Predator Helios 300
(PH317-53). They come from studying how PredatorSense 3.00.3136 talks to the
hardware, checked against the values read from and written to a real PH317-53,
and compared with the Linux `acer-wmi` driver and the
[acer-predator-turbo-and-rgb-keyboard-linux-module](https://github.com/JafarAkhondali/acer-predator-turbo-and-rgb-keyboard-linux-module).
No PredatorSense code is included in this project.

## The interface

The firmware exposes a WMI class, `AcerGamingFunction`, in the `root\WMI`
namespace. Each method takes one `gmInput` value and returns one `gmOutput`
value. Calling it needs administrator rights, which is why OpenSenses runs a
small background service.

For every `Set…` method, the low byte of the output is a status: `0` means
success. For `Get…` methods the low byte is also a status, and the data
follows from bit 8.

PredatorSense picks its screens from `C:\ProgramData\oem\PredatorSense\Feature.ini`.
The PH317-53 uses `MachineType=1`: a 4-zone RGB keyboard, a CPU fan and a GPU
fan, GPU overclocking and no CPU overclocking.

OpenSenses keeps the same per-model data in `crates/core/src/models.rs`,
looked up by the BIOS product name. On a model that isn't listed it probes
instead: each `Get…` call that returns status 0 turns the matching feature on.

## Keyboard lighting

**Effects** use `SetGamingKBBacklight`, which takes a 16-byte array:

| Byte | Meaning |
| ---- | ------- |
| 0 | Effect: 0 static, 1 breathing, 2 neon, 3 wave, 4 shifting, 5 zoom |
| 1 | Speed, 1 (slow) to 9 (fast); 0 for static |
| 2 | Brightness, 0 to 100 (PredatorSense uses 0, 25, 50, 75, 100) |
| 3 | `0x08` for wave, otherwise 0 |
| 4 | Direction for wave and shifting: 1 right to left, 2 left to right |
| 5–7 | Red, green, blue for breathing, shifting and zoom |
| 8–15 | Zero |

**Static zone colours** use `SetGamingRgbKb` (uint32):
`zone | red << 8 | green << 16 | blue << 24`, where zone is `1`, `2`, `4` or `8`
(left, middle, right, number pad). Send a static `SetGamingKBBacklight` first to
set the brightness. `GetGamingRgbKb(zone)` reads a zone's colour back.

The temperature effect is done in software: the service repaints all four
zones as the CPU temperature changes.

## Fans

`SetGamingFanBehavior` (uint64): fan bits in the low 16 bits (CPU `0x1`,
GPU `0x8`, second GPU fan `0x10` on the Triton 500), then a 2-bit mode per
fan, CPU at bit 16, GPU at bit 22 and the second GPU fan at bit 24.
Modes: 1 auto, 2 max, 3 custom. Examples: both auto `0x410009`, both max
`0x820009`, CPU custom `0x30001`, GPU custom `0xC00008`.

`SetGamingFanSpeed` (uint64): `fan_id | percent << 8`, fan id 1 for the CPU
fan, 4 for the GPU fan and 5 for the second GPU fan, 0 to 100 percent. Only has an effect in custom
mode.

`GetGamingFanBehavior(0x9)` returns the CPU mode in bits 8–9 and the GPU mode
in bits 14–15. `GetGamingFanSpeed(1 or 4)` returns the set percentage in bits
8–15.

## Sensors

`GetGamingSysInfo(1 | sensor << 8)` returns a reading in bits 8–23:

| Sensor | Reading |
| ------ | ------- |
| `0x01` | CPU temperature, °C |
| `0x02` | CPU fan, RPM |
| `0x03` | System temperature, °C |
| `0x06` | GPU fan, RPM |
| `0x0A` | GPU temperature, °C |

## GPU overclock

PredatorSense does two things for its Normal / Faster / Turbo levels:

1. Tells the firmware the level with `SetGamingMiscSetting(0x05 | level << 8)`,
   where `level` is the position in that model's list (0 Normal, 1 Faster,
   2 Turbo; the Helios 700 has a fourth, Boosted, before Turbo). The firmware
   stores whatever value it is given (read back with `GetGamingMiscSetting(5)`).
   Triton 500 and Helios 500 skip this step.
2. Applies clock offsets through NVIDIA's driver API (NVAPI pstates20, P0).
   The offsets depend on the GPU, identified by its PCI device and subsystem
   ids: on the PH317-53's RTX 2070 Max-Q (`DEV_1F10&SUBSYS_13421025`) Faster
   is +100 MHz core and +160 MHz memory, Turbo +160 and +320. The tables for
   every model are in `models.rs`.

NVIDIA's driver drops offsets on restart and on driver updates, so the
service sets them again.

## Turbo LED

`SetGamingLED(0x10001)` turns the turbo LED on and `SetGamingLED(0x1)` off;
`GetGamingLED(1)` reads `0x100` while it is lit. (PredatorSense's internal
name for this call is "LED behavior", but it is the `SetGamingLED` method,
not `SetGamingLEDBehavior`, which rejects these values.)

## Toggles

| Setting | Set | Read |
| ------- | --- | ---- |
| LCD overdrive | `SetGamingProfile(0x10 \| on << 48)` | `GetGamingProfile(0)`, bits 48–55 |
| Windows and Menu keys | `SetGamingProfile(0x2 \| on << 24)` | `GetGamingProfile(0)`, bits 24–31 |
| Boot animation and sound | `SetGamingMiscSetting(0x6 \| on << 8)` | `GetGamingMiscSetting(6)`, bits 8–15 |
| Keyboard backlight off after 30 s | `APGeAction.SetFunction`, see below | `APGeAction.GetFunction(0x88401)`, bits 40–47 (30 = on, 0 = off) |

The backlight timeout lives in a second WMI class, `APGeAction`
(`SetFunction` / `GetFunction`, parameters `uiInput` / `uiOutput`). In the
input `0x88402 | byte4 << 32 | seconds << 40`, 0x84 is the backlight hotkey
number on these models and `seconds` is 30 (on) or 0 (off). Two quirks, seen on
the PH317-53 and matching what PredatorSense does: the call is only accepted
when byte 4 equals byte 4 of the value `GetFunction(0x88401)` just returned, and
an accepted call flips the setting. So read first, send only when the state
needs to change, and copy byte 4 across. Readings seen: `0x1E0000080000` on,
`0x6400000000` off.

The firmware does the dimming itself. PredatorSense's agent only listens to
the `APGeEvent` WMI event (`EventDetail[0] == 1`) to record the current state.

On the Triton 500, `SetGamingMiscSetting(0x2 | mode << 8)` switches between
hybrid graphics (1) and the NVIDIA GPU only (2); it takes effect after a
restart. Read with `GetGamingMiscSetting(2)`.

**CoolBoost** (fans run harder in Auto mode) is also in `APGeAction`:
`SetFunction(7 | on << 16)`, read with `GetFunction(0x207)`, where byte 1 is 1
while it is on (`0x1010100` on, `0x1010000` off on the PH317-53). Sending the
same value twice leaves it as it is. PredatorSense shows it on the fan page and
only lets it be changed in Auto mode. On the PH317-53 it raised both fans by
about 1000 RPM.

Sticky Keys is a Windows accessibility setting of the signed-in user
(`SystemParametersInfo` with `SPI_SETSTICKYKEYS`), so the app sets it
directly; it doesn't involve the firmware.
