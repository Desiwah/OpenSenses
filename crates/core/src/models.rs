//! What each laptop PredatorSense 3.00 supported can do, taken from the
//! per-model data that shipped with it (Feature.ini, HW_Support.ini and
//! PredatorSense.ini in its `plugs` folder). Laptops not listed here are
//! probed instead (see the backends).

use crate::settings::{Fan, GpuMode, Rgb, Toggle};
use serde::{Deserialize, Serialize};

/// The laptop OpenSenses has been tested on, and its GPU. Everything on
/// other models follows PredatorSense's data but is marked experimental.
pub const TESTED_PRODUCT: &str = "Predator PH317-53";
pub const TESTED_GPU: (u16, u32) = (0x1F10, 0x1342_1025);

/// PredatorSense's internal name for each family of models; the families
/// share screens and firmware conventions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    /// Helios 500 (PH517-51, PH517-61)
    Cayman,
    /// Helios 300 (PH315-52, PH317-53), Triton 300 (PT315-51)
    Covini,
    /// Triton 500 (PT515-51)
    Ghibli,
    /// Triton 900 (PT917-71)
    Raptor,
    /// Helios 700 (PH717-71)
    Levante,
}

/// Clock offsets in MHz for one GPU, one value per level in `Model::gpu_levels`.
#[derive(Debug, Clone, Copy)]
pub struct GpuTable {
    /// PCI device id (e.g. 0x1F10) and subsystem id (e.g. 0x13421025).
    pub device: u16,
    pub subsystem: u32,
    pub core: &'static [i32],
    pub memory: &'static [i32],
}

#[derive(Debug)]
pub struct Model {
    /// Product name as the BIOS reports it, e.g. "Predator PH317-53".
    pub product: &'static str,
    pub name: &'static str,
    pub family: Family,
    /// 4-zone RGB through the gaming WMI interface.
    pub four_zone: bool,
    /// Per-key RGB keyboard on its own USB controller.
    pub per_key: bool,
    pub default_colour: Rgb,
    pub fans: &'static [Fan],
    pub gpu_levels: &'static [GpuMode],
    /// Whether PredatorSense also tells the firmware the GPU level
    /// (`SetGamingMiscSetting(5)`), with the level's index as the value.
    pub gpu_level_to_firmware: bool,
    pub gpu_tables: &'static [GpuTable],
    pub toggles: &'static [Toggle],
    pub cpu_overclock: bool,
    pub power_limit: bool,
    pub hotkeys: bool,
}

const TEAL: Rgb = Rgb { r: 0x00, g: 0xAE, b: 0xC7 };
const BLUE: Rgb = Rgb { r: 0x00, g: 0x9B, b: 0xF0 };
const THREE: &[GpuMode] = &[GpuMode::Normal, GpuMode::Faster, GpuMode::Turbo];
const FOUR: &[GpuMode] = &[GpuMode::Normal, GpuMode::Faster, GpuMode::Boosted, GpuMode::Turbo];
const CPU_GPU: &[Fan] = &[Fan::Cpu, Fan::Gpu];

const fn gpu(device: u16, subsystem: u32, core: &'static [i32], memory: &'static [i32]) -> GpuTable {
    GpuTable { device, subsystem, core, memory }
}

const HELIOS_300_GPUS: &[GpuTable] = &[
    gpu(0x2191, 0x1343_1025, &[0, 100, 160], &[0, 160, 320]),
    gpu(0x1F11, 0x1343_1025, &[0, 100, 160], &[0, 160, 320]),
    gpu(0x1F10, 0x1342_1025, &[0, 100, 160], &[0, 160, 320]),
    gpu(0x1F11, 0x1342_1025, &[0, 100, 160], &[0, 160, 320]),
    gpu(0x2191, 0x1342_1025, &[0, 100, 160], &[0, 160, 320]),
];

const COVINI_TOGGLES: &[Toggle] = &[Toggle::BacklightTimeout, Toggle::LcdOverdrive, Toggle::WinKey, Toggle::BootAnimation];

pub static MODELS: &[Model] = &[
    Model {
        product: "Predator PH315-52",
        name: "Predator Helios 300",
        family: Family::Covini,
        four_zone: true,
        per_key: false,
        default_colour: TEAL,
        fans: CPU_GPU,
        gpu_levels: THREE,
        gpu_level_to_firmware: true,
        gpu_tables: HELIOS_300_GPUS,
        toggles: COVINI_TOGGLES,
        cpu_overclock: false,
        power_limit: false,
        hotkeys: false,
    },
    Model {
        product: "Predator PH317-53",
        name: "Predator Helios 300",
        family: Family::Covini,
        four_zone: true,
        per_key: false,
        default_colour: TEAL,
        fans: CPU_GPU,
        gpu_levels: THREE,
        gpu_level_to_firmware: true,
        gpu_tables: HELIOS_300_GPUS,
        toggles: COVINI_TOGGLES,
        cpu_overclock: false,
        power_limit: false,
        hotkeys: false,
    },
    Model {
        product: "Predator PT315-51",
        name: "Predator Triton 300",
        family: Family::Covini,
        four_zone: true,
        per_key: false,
        default_colour: TEAL,
        fans: CPU_GPU,
        gpu_levels: THREE,
        gpu_level_to_firmware: true,
        gpu_tables: &[
            gpu(0x2191, 0x1400_1025, &[0, 50, 100], &[0, 60, 120]),
            gpu(0x1F91, 0x1400_1025, &[0, 65, 130], &[0, 110, 220]),
            gpu(0x1F15, 0x1400_1025, &[0, 30, 60], &[0, 30, 60]),
            gpu(0x1F11, 0x1342_1025, &[0, 100, 160], &[0, 160, 320]),
            gpu(0x2191, 0x1342_1025, &[0, 100, 160], &[0, 160, 320]),
        ],
        toggles: COVINI_TOGGLES,
        cpu_overclock: false,
        power_limit: false,
        hotkeys: false,
    },
    Model {
        product: "Predator PT515-51",
        name: "Predator Triton 500",
        family: Family::Ghibli,
        four_zone: true,
        per_key: false,
        default_colour: TEAL,
        fans: &[Fan::Cpu, Fan::Gpu, Fan::Gpu2],
        gpu_levels: THREE,
        // This family's firmware value isn't known for sure, so only the
        // clock offsets (the actual overclock) are applied.
        gpu_level_to_firmware: false,
        gpu_tables: &[
            gpu(0x1F11, 0x132D_1025, &[0, 75, 100], &[120, 200, 270]),
            gpu(0x1F10, 0x132D_1025, &[0, 75, 100], &[120, 200, 270]),
            gpu(0x1ED0, 0x132D_1025, &[0, 75, 100], &[120, 200, 270]),
        ],
        toggles: &[Toggle::BacklightTimeout, Toggle::LcdOverdrive, Toggle::DiscreteGpu, Toggle::WinKey, Toggle::BootAnimation],
        cpu_overclock: false,
        power_limit: false,
        hotkeys: false,
    },
    Model {
        product: "Predator PH517-51",
        name: "Predator Helios 500",
        family: Family::Cayman,
        four_zone: true,
        per_key: false,
        default_colour: BLUE,
        fans: CPU_GPU,
        gpu_levels: THREE,
        gpu_level_to_firmware: false,
        gpu_tables: &[gpu(0x1BE1, 0x1243_1025, &[0, 75, 150], &[0, 150, 300])],
        toggles: &[Toggle::WinKey],
        cpu_overclock: true,
        power_limit: true,
        hotkeys: true,
    },
    Model {
        product: "Predator PH517-61",
        name: "Predator Helios 500",
        family: Family::Cayman,
        four_zone: true,
        per_key: false,
        default_colour: BLUE,
        fans: CPU_GPU,
        // AMD Radeon RX Vega 56: PredatorSense offered no GPU overclock.
        gpu_levels: &[],
        gpu_level_to_firmware: false,
        gpu_tables: &[],
        toggles: &[Toggle::WinKey],
        cpu_overclock: false,
        power_limit: false,
        hotkeys: true,
    },
    Model {
        product: "Predator PH717-71",
        name: "Predator Helios 700",
        family: Family::Levante,
        four_zone: false,
        per_key: true,
        default_colour: TEAL,
        fans: CPU_GPU,
        gpu_levels: FOUR,
        gpu_level_to_firmware: true,
        gpu_tables: &[
            gpu(0x1ED0, 0x133B_1025, &[0, 60, 60, 100], &[0, 133, 266, 400]),
            gpu(0x1F50, 0x133B_1025, &[0, 40, 80, 100], &[0, 133, 266, 400]),
        ],
        // Its Windows-key and backlight settings go through the per-key
        // keyboard's own controller, which isn't supported yet.
        toggles: &[Toggle::BootAnimation],
        cpu_overclock: true,
        power_limit: false,
        hotkeys: true,
    },
    Model {
        product: "Predator PT917-71",
        name: "Predator Triton 900",
        family: Family::Raptor,
        four_zone: false,
        per_key: true,
        default_colour: TEAL,
        fans: CPU_GPU,
        gpu_levels: THREE,
        gpu_level_to_firmware: true,
        gpu_tables: &[gpu(0x1ED0, 0x1323_1025, &[0, 60, 120], &[0, 200, 400])],
        toggles: &[Toggle::BootAnimation],
        cpu_overclock: true,
        power_limit: false,
        hotkeys: true,
    },
];

/// Finds a model by the BIOS product name (case and spacing are ignored).
pub fn lookup(product: &str) -> Option<&'static Model> {
    let norm = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ").to_ascii_lowercase();
    let want = norm(product);
    MODELS.iter().find(|m| norm(m.product) == want)
}

/// GPU offset table for a GPU, from this model's list or, for unknown
/// models, from any model that listed the same GPU.
pub fn gpu_table(model: Option<&'static Model>, device: u16, subsystem: u32) -> Option<&'static GpuTable> {
    let hit = |t: &&GpuTable| t.device == device && t.subsystem == subsystem;
    match model {
        Some(m) => m.gpu_tables.iter().find(hit),
        None => MODELS.iter().flat_map(|m| m.gpu_tables.iter()).find(hit),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_this_laptop_and_its_gpu() {
        let m = lookup("Predator PH317-53").unwrap();
        assert_eq!(m.family, Family::Covini);
        // RTX 2070 Max-Q in the PH317-53: PCI\VEN_10DE&DEV_1F10&SUBSYS_13421025
        let t = gpu_table(Some(m), 0x1F10, 0x1342_1025).unwrap();
        assert_eq!((t.core[1], t.memory[2]), (100, 320));
        assert!(lookup("predator  ph317-53 ").is_some());
        assert!(lookup("Nitro AN515-54").is_none());
    }

    #[test]
    fn tables_match_their_levels() {
        for m in MODELS {
            for t in m.gpu_tables {
                assert_eq!(t.core.len(), m.gpu_levels.len(), "{}", m.product);
                assert_eq!(t.memory.len(), m.gpu_levels.len(), "{}", m.product);
            }
        }
    }
}
