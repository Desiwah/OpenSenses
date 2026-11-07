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
GPU `0x8`), then a 2-bit mode per fan, CPU at bit 16 and GPU at bit 22.
Modes: 1 auto, 2 max, 3 custom. Examples: both auto `0x410009`, both max
`0x820009`, CPU custom `0x30001`, GPU custom `0xC00008`.

`SetGamingFanSpeed` (uint64): `fan_id | percent << 8`, fan id 1 for the CPU
fan and 4 for the GPU fan, 0 to 100 percent. Only has an effect in custom
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

1. Tells the firmware the level with `SetGamingMiscSetting(0x05 | level << 8)`.
   OpenSenses uses levels 0, 1 and 2, as the Linux driver does for normal and
   turbo. The firmware stores whatever value it is given (read back with
   `GetGamingMiscSetting(5)`).
2. Applies clock offsets through NVIDIA's driver API (NVAPI pstates20, P0):
   Faster is +100 MHz core and +160 MHz memory, Turbo is +160 MHz core and
   +320 MHz memory (from PredatorSense's settings for this model's GPUs).

NVIDIA's driver drops offsets on restart and on driver updates, so the
service sets them again.

## Turbo LED

`SetGamingLEDBehavior(0x10001)` and `(0x1)` switch the turbo LED on other
Predator models. The PH317-53 answers with status 1 (not supported), so
OpenSenses tries it but ignores the result.
