//! The part that owns the hardware. Everything that touches the firmware
//! runs on one thread (COM objects on Windows must stay on their thread);
//! IPC connections hand their requests to it through a channel.

use pcore::backend::{self, Backend, Sensors};
use pcore::ipc::{Request, Response, State};
use pcore::settings::{self, Effect, FanMode, GpuMode, HotkeyAction, Rgb, Settings, TurboAction};
use std::collections::HashSet;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant, SystemTime};

pub enum Job {
    Ipc(Request, Sender<Response>),
    /// A key event from the firmware (`EventDetail` bytes).
    Firmware([u8; 8]),
    /// The PredatorSense key was pressed (Linux; on Windows the per-user
    /// agent opens the app itself).
    #[cfg_attr(windows, allow(dead_code))]
    PredatorSenseKey,
    Stop,
}

/// PredatorSense only raises the CPU level on the charger with the battery
/// at 40% or more.
const CPU_MIN_BATTERY: u8 = 40;

const TICK: Duration = Duration::from_secs(2);
const DRIFT_EVERY: Duration = Duration::from_secs(30);
const SLEEP_GAP: Duration = Duration::from_secs(30);

/// Fans are forced to Max while either chip is this hot in Custom mode,
/// and handed back once both drop below the release point.
const GUARD_CPU_C: u32 = 95;
const GUARD_GPU_C: u32 = 90;
const GUARD_RELEASE_C: u32 = 80;

struct Daemon {
    hw: Option<Box<dyn Backend>>,
    open_error: Option<String>,
    /// The saved settings: everyday values plus the game profiles.
    settings: Settings,
    /// What was last sent to the hardware: the everyday settings, or those
    /// with a game's profile laid over them.
    applied: Settings,
    /// Game profile in use (index into `settings.games`).
    game: Option<usize>,
    /// Programs seen running at the last check.
    running: HashSet<String>,
    /// Windows-key state from before a game profile changed it, put back
    /// when the game closes if the everyday setting is "leave as is".
    restore_win_key: Option<bool>,
    sensors: Sensors,
    errors: Vec<String>,
    guard: bool,
    temp_colour: Option<Rgb>,
    toggles_now: pcore::settings::Toggles,
    /// Turbo mode from the Turbo key (or the app).
    turbo: bool,
    /// CPU level index and fan curve last sent.
    cpu_sent: Option<(usize, u8)>,
    power: Option<(bool, u8)>,
    hotkey_group: u8,
    notice: Option<(u32, String)>,
    #[cfg(windows)]
    agent: crate::user::Agent,
}

impl Daemon {
    fn state(&self) -> State {
        State {
            settings: self.settings.clone(),
            sensors: self.sensors.clone(),
            capabilities: self.hw.as_ref().map(|h| h.capabilities()).unwrap_or_default(),
            backend: self.hw.as_ref().map(|h| h.name().to_string()).unwrap_or_else(|| "none".into()),
            errors: match &self.open_error {
                Some(e) => vec![format!("hardware not available: {e}")],
                None => self.errors.clone(),
            },
            overheat_guard: self.guard,
            toggles_now: self.toggles_now,
            active_game: self.game.and_then(|i| self.settings.games.get(i)).map(|g| g.exe.clone()),
            original: pcore::snapshot::load().map(|s| s.info()),
            turbo: self.turbo,
            cpu_level: self.cpu_sent.map(|c| c.0),
            power: self.power,
            hotkey_group: self.hotkey_group,
            notice: self.notice.clone(),
        }
    }

    fn say(&mut self, text: impl Into<String>) {
        let text = text.into();
        log::info!("notice: {text}");
        let n = self.notice.as_ref().map(|(n, _)| n + 1).unwrap_or(1);
        self.notice = Some((n, text));
    }

    fn caps(&self) -> pcore::backend::Capabilities {
        self.hw.as_ref().map(|h| h.capabilities()).unwrap_or_default()
    }

    fn on_ac(&mut self) -> bool {
        self.power = backend::power_status();
        self.power.map(|(ac, _)| ac).unwrap_or(true)
    }

    /// Sends the CPU level and the firmware fan curve that go with the
    /// current levels, as PredatorSense does: CPU raised only on the
    /// charger with enough battery, curve = max(CPU, GPU level) + 1.
    fn sync_cpu(&mut self, force: bool) {
        // Nothing is sent until the user has changed something (or after a restore).
        if self.settings.restored || !self.settings.touched {
            return;
        }
        self.power = backend::power_status();
        let caps = self.caps();
        let cpu_control = caps.cpu_follows_gpu || !caps.cpu_levels.is_empty();
        if !cpu_control && !caps.fan_table {
            return;
        }
        let gpu_idx = caps.gpu_level(self.applied.gpu).map(|(i, _)| i).unwrap_or(0);
        let want = if !caps.cpu_levels.is_empty() {
            caps.cpu_levels.iter().position(|m| *m == self.applied.cpu).unwrap_or(0)
        } else if caps.cpu_follows_gpu {
            gpu_idx
        } else {
            0
        };
        let (ac, pct) = self.power.unwrap_or((true, 100));
        let cpu_idx = if ac && pct >= CPU_MIN_BATTERY { want } else { 0 };
        let curve = pcore::protocol::fan_table_for(cpu_idx, gpu_idx);
        if !force && self.cpu_sent == Some((cpu_idx, curve)) {
            return;
        }
        let Some(hw) = self.hw.as_mut() else { return };
        if cpu_control {
            if let Err(e) = hw.set_cpu_level(cpu_idx) {
                self.errors.push(format!("CPU level: {e:#}"));
            }
        }
        if caps.fan_table {
            if let Err(e) = hw.set_fan_table(curve) {
                self.errors.push(format!("fan curve: {e:#}"));
            }
        }
        if cpu_control {
            log::info!("CPU level {cpu_idx} (wanted {want}, AC {ac}, battery {pct}%), fan curve {curve}");
        } else {
            log::info!("fan curve {curve}");
        }
        self.cpu_sent = Some((cpu_idx, curve));
    }

    /// Turbo on or off: the Turbo key's action laid over the settings.
    fn set_turbo(&mut self, on: bool) {
        if on == self.turbo {
            return;
        }
        if self.settings.restored {
            self.settings.restored = false;
            self.save();
        }
        if on && !self.on_ac() {
            self.say("Turbo needs the charger plugged in");
            return;
        }
        self.turbo = on;
        self.apply_since_last();
        let led = on || self.applied.gpu == GpuMode::Turbo;
        if let Some(hw) = self.hw.as_mut() {
            let _ = hw.set_turbo_led(led);
        }
        self.say(if on { "Turbo on" } else { "Turbo off" });
    }

    fn save(&self) {
        if let Err(e) = settings::save(&self.settings) {
            log::error!("saving settings: {e:#}");
        }
    }

    /// A key event from the firmware.
    /// `test` is the app's "Try it" button: byte 3 then names the group.
    fn on_firmware_event(&mut self, d: [u8; 8], test: bool) {
        log::info!("{} event {d:02X?}", if test { "test" } else { "firmware" });
        let caps = self.caps();
        match (d[0], d[1]) {
            (7, 4) if caps.turbo_key && self.settings.turbo_key != TurboAction::Off => self.set_turbo(!self.turbo),
            (7, 1) if (1..=3).contains(&d[2]) => {
                self.hotkey_group = d[2];
                let name = self.settings.hotkeys.get(d[2] as usize - 1).map(|g| g.name.clone()).unwrap_or_default();
                self.say(if name.is_empty() {
                    format!("Hotkey group {}", d[2])
                } else {
                    format!("Hotkey group {}: {name}", d[2])
                });
            }
            (7, 2) if caps.hotkey_keys > 0 && (1..=caps.hotkey_keys).contains(&d[2]) => {
                let group = if test && (1..=3).contains(&d[3]) {
                    d[3]
                } else {
                    if let Some(g) = self.hw.as_mut().and_then(|h| h.hotkey_group()) {
                        self.hotkey_group = g;
                    }
                    self.hotkey_group
                };
                let group = group.clamp(1, 3) as usize;
                let action = self
                    .settings
                    .hotkeys
                    .get(group - 1)
                    .and_then(|g| g.keys.get(d[2] as usize - 1))
                    .cloned()
                    .unwrap_or_default();
                self.run_action(&action);
            }
            _ => {}
        }
    }

    /// Carries out a programmable key's action.
    fn run_action(&mut self, a: &HotkeyAction) {
        let caps = self.caps();
        let next = |cycle: &[GpuMode], now: GpuMode| -> Option<GpuMode> {
            let i = cycle.iter().position(|m| *m == now).map(|i| i + 1).unwrap_or(0);
            cycle.get(i % cycle.len().max(1)).copied()
        };
        let mut changed = true;
        match a {
            HotkeyAction::None => changed = false,
            HotkeyAction::GpuLevels { cycle } => match next(cycle, self.settings.gpu).filter(|m| caps.gpu_level(*m).is_some()) {
                Some(m) => {
                    self.settings.gpu = m;
                    self.say(format!("GPU: {m:?}"));
                }
                None => changed = false,
            },
            HotkeyAction::CpuLevels { cycle } => {
                if !self.on_ac() {
                    self.say("CPU overclocking needs the charger plugged in");
                    changed = false;
                } else if let Some(m) = next(cycle, self.settings.cpu).filter(|m| caps.cpu_levels.contains(m)) {
                    self.settings.cpu = m;
                    self.say(format!("CPU: {m:?}"));
                } else {
                    changed = false;
                }
            }
            HotkeyAction::FanModes { cycle } if !cycle.is_empty() => {
                let i = cycle.iter().position(|m| *m == self.settings.fans.mode).map(|i| i + 1).unwrap_or(0);
                let m = cycle[i % cycle.len()];
                self.settings.fans.mode = m;
                self.say(format!("Fans: {m:?}"));
            }
            HotkeyAction::FanModes { .. } => changed = false,
            HotkeyAction::Performance => {
                let top_gpu = caps.gpu_levels.last().map(|l| l.mode).unwrap_or(GpuMode::Normal);
                let top_cpu = caps.cpu_levels.last().copied().unwrap_or(GpuMode::Normal);
                let at_top = self.settings.gpu == top_gpu && (caps.cpu_levels.is_empty() || self.settings.cpu == top_cpu);
                if !at_top && !self.on_ac() {
                    self.say("Extreme performance needs the charger plugged in");
                    changed = false;
                } else {
                    self.settings.gpu = if at_top { GpuMode::Normal } else { top_gpu };
                    self.settings.cpu = if at_top { GpuMode::Normal } else { top_cpu };
                    self.say(if at_top { "Performance: Normal" } else { "Performance: Extreme" });
                }
            }
            HotkeyAction::Turbo => {
                self.set_turbo(!self.turbo);
                changed = false;
            }
            HotkeyAction::WinKey | HotkeyAction::LcdOverdrive => {
                let t = if *a == HotkeyAction::WinKey {
                    pcore::settings::Toggle::WinKey
                } else {
                    pcore::settings::Toggle::LcdOverdrive
                };
                if caps.has_toggle(t) {
                    let now = self.settings.toggles.get(t).or(self.toggles_now.get(t)).unwrap_or(true);
                    self.settings.toggles.set(t, Some(!now));
                    let label = if t == pcore::settings::Toggle::WinKey {
                        "Windows key"
                    } else {
                        "LCD overdrive"
                    };
                    self.say(format!("{label} {}", if now { "off" } else { "on" }));
                } else {
                    changed = false;
                }
            }
            HotkeyAction::StickyKeys | HotkeyAction::LaunchApp { .. } | HotkeyAction::Macro { .. } | HotkeyAction::OpenApp => {
                changed = false;
                if let Err(e) = crate::user::run(a) {
                    self.say(format!("Couldn't run the key's action: {e:#}"));
                }
            }
        }
        if changed {
            self.settings.restored = false;
            self.save();
            self.apply_since_last();
        }
    }

    /// Puts the original settings back and stops changing anything until
    /// the user changes a setting. The saved settings then describe the
    /// restored laptop, so the app shows what it really has.
    fn restore_original(&mut self) {
        let Some(snap) = pcore::snapshot::load() else {
            self.errors = vec!["no original settings were saved on this laptop".into()];
            return;
        };
        let Some(hw) = self.hw.as_mut() else { return };
        log::info!("restoring the original settings ({:?}, taken {})", snap.source, snap.taken);
        self.errors = hw.restore(&snap);
        for e in &self.errors {
            log::warn!("restore: {e}");
        }
        let caps = hw.capabilities();
        let mut s = self.settings.clone();
        if let Some((effect, speed, brightness, direction, colour)) = snap.kb_effect.as_deref().and_then(pcore::protocol::decode_kb_backlight) {
            s.lighting.effect = effect;
            s.lighting.speed = speed.max(1);
            s.lighting.brightness = brightness;
            s.lighting.direction = direction;
            if colour != Rgb::default() {
                s.lighting.colour = colour;
            }
        }
        if let Some(z) = snap.zones {
            s.lighting.zone_on = z.map(|c| c != Rgb::default());
            s.lighting.zones = z.map(|c| if c == Rgb::default() { s.lighting.colour } else { c });
        }
        if let Some(&(_, first)) = snap.fans.first() {
            s.fans.mode = if snap.fans.iter().all(|&(_, m)| m == first) { first } else { FanMode::Auto };
        }
        for &(fan, pct) in &snap.fan_speeds {
            match fan {
                pcore::settings::Fan::Cpu => s.fans.cpu_percent = Some(pct),
                pcore::settings::Fan::Gpu => s.fans.gpu_percent = Some(pct),
                pcore::settings::Fan::Gpu2 => s.fans.gpu2_percent = Some(pct),
            }
        }
        s.gpu = caps
            .gpu_levels
            .iter()
            .find(|l| Some(l.core_mhz) == snap.gpu_core && Some(l.memory_mhz) == snap.gpu_memory)
            .map(|l| l.mode)
            .unwrap_or(pcore::settings::GpuMode::Normal);
        // Everything is as it was; "leave as is" from now on.
        s.toggles = Default::default();
        s.restored = true;
        self.settings = s;
        if let Err(e) = settings::save(&self.settings) {
            log::error!("saving settings: {e:#}");
        }
        self.game = None;
        self.restore_win_key = None;
        self.guard = false;
        self.temp_colour = None;
        self.turbo = false;
        self.cpu_sent = None;
        self.applied = self.settings.clone();
        if let Some(hw) = self.hw.as_mut() {
            self.toggles_now = hw.observe().toggles;
        }
    }

    /// The settings the hardware should have right now.
    fn effective(&self) -> Settings {
        let mut s = match self.game {
            Some(i) => self.settings.with_game(i),
            None => {
                let mut s = self.settings.clone();
                if s.toggles.win_key.is_none() {
                    s.toggles.win_key = self.restore_win_key;
                }
                s
            }
        };
        if self.turbo {
            let caps = self.caps();
            let (oc, fans) = match self.settings.turbo_key {
                TurboAction::Performance => (true, true),
                TurboAction::Overclock => (true, false),
                TurboAction::Fans => (false, true),
                TurboAction::Off => (false, false),
            };
            if oc {
                if let Some(top) = caps.gpu_levels.last() {
                    s.gpu = top.mode;
                }
                if let Some(top) = caps.cpu_levels.last() {
                    s.cpu = *top;
                }
            }
            if fans && !caps.fans.is_empty() {
                s.fans.mode = FanMode::Max;
            }
        }
        s
    }

    /// Picks the game profile for what is running now. True when it changed.
    fn retarget(&mut self) -> bool {
        let now = self.settings.active_game(&self.running);
        if now == self.game {
            return false;
        }
        // Still holding a value means a game was in use moments ago (its
        // profile was just edited away); the hardware then shows the game's
        // state, not the one to go back to.
        if self.game.is_none() && self.restore_win_key.is_none() && self.settings.toggles.win_key.is_none() {
            self.restore_win_key = self.hw.as_mut().and_then(|h| h.observe().toggles.win_key);
        }
        match now.and_then(|i| self.settings.games.get(i)) {
            Some(g) => log::info!("GameSync: {} is running, using its profile", g.exe),
            None => log::info!("GameSync: back to everyday settings"),
        }
        self.game = now;
        true
    }

    fn apply(&mut self) {
        self.apply_from(None);
    }

    /// Sends what changed since `old` (or everything) and refreshes the
    /// toggle states shown in the app.
    fn apply_from(&mut self, old: Option<&Settings>) {
        if self.settings.restored {
            return;
        }
        let new = self.effective();
        let Some(hw) = self.hw.as_mut() else { return };
        let caps = hw.capabilities();
        if !self.settings.touched && (caps.lighting || !caps.fans.is_empty() || !caps.toggles.is_empty() || !caps.gpu_levels.is_empty()) {
            self.settings.touched = true;
            if let Err(e) = settings::save(&self.settings) {
                log::error!("saving settings: {e:#}");
            }
        }
        self.errors = pcore::apply_changes(hw.as_mut(), old, &new);
        if old.map(|o| o.lighting != new.lighting).unwrap_or(true) {
            self.temp_colour = None;
        }
        if old.map(|o| o.fans != new.fans).unwrap_or(true) {
            self.guard = false;
        }
        if old.map(|o| o.toggles != new.toggles).unwrap_or(true) && !hw.capabilities().toggles.is_empty() {
            self.toggles_now = hw.observe().toggles;
        }
        for e in &self.errors {
            log::warn!("{e}");
        }
        if self.turbo {
            // The LED shows Turbo even when the key only drives the fans.
            let _ = hw.set_turbo_led(true);
        }
        self.applied = new;
        if self.game.is_none() {
            self.restore_win_key = None;
        }
        self.sync_cpu(old.is_none());
    }

    fn apply_since_last(&mut self) {
        let old = self.applied.clone();
        self.apply_from(Some(&old));
    }

    fn handle(&mut self, req: Request) -> Response {
        match req {
            Request::GetState => {}
            Request::SetSettings { settings: s } => {
                let playing = self.game.and_then(|i| self.settings.games.get(i)).map(|g| g.exe.clone());
                let s = s.sanitized();
                // Picking a GPU level or fan mode yourself ends Turbo.
                if self.turbo && (s.gpu != self.settings.gpu || s.cpu != self.settings.cpu || s.fans.mode != self.settings.fans.mode) {
                    log::info!("Turbo off: levels changed in the app");
                    self.turbo = false;
                }
                self.settings = s;
                if let Err(e) = settings::save(&self.settings) {
                    log::error!("saving settings: {e:#}");
                }
                // The edit may have added, removed or switched off profiles,
                // which shifts indexes: find the game in use again first.
                self.game = playing.and_then(|exe| self.settings.games.iter().position(|g| g.exe == exe));
                self.retarget();
                self.apply_since_last();
            }
            Request::Reapply => {
                // "Apply again" after a restore means "use my settings again".
                if self.settings.restored {
                    self.settings.restored = false;
                    if let Err(e) = settings::save(&self.settings) {
                        log::error!("saving settings: {e:#}");
                    }
                }
                self.apply()
            }
            Request::RestoreOriginal => self.restore_original(),
            Request::SetTurbo { on } => self.set_turbo(on),
            Request::TestKey { detail } => self.on_firmware_event(detail, true),
        }
        Response::State(self.state())
    }

    fn tick(&mut self) {
        if self.hw.is_none() {
            return;
        }
        #[cfg(windows)]
        {
            let want = self.caps().ps_key;
            self.agent.keep(want);
        }
        if self.settings.restored {
            // Hands off after a restore: sensors only.
            if let Some(hw) = self.hw.as_mut() {
                self.sensors = hw.sensors();
            }
            return;
        }
        // GameSync: switch profiles as games start and close.
        if (self.settings.game_sync && !self.settings.games.is_empty()) || self.game.is_some() {
            self.running = pcore::procs::running();
            if self.retarget() {
                self.apply_since_last();
            }
        }

        // Charger unplugged: Turbo ends, and the CPU level drops (sync_cpu).
        if self.turbo && !backend::power_status().map(|(ac, _)| ac).unwrap_or(true) {
            self.turbo = false;
            self.apply_since_last();
            self.say("Turbo off: the charger was unplugged");
        }
        self.sync_cpu(false);

        let Some(hw) = self.hw.as_mut() else { return };
        self.sensors = hw.sensors();

        // Temperature lighting: repaint only when the colour actually changes.
        if self.applied.lighting.effect == Effect::Temperature {
            if let Some(t) = self.sensors.cpu_temp {
                let c = pcore::temperature_colour(t);
                if self.temp_colour != Some(c) {
                    let mut ok = true;
                    for z in 1..=4 {
                        ok &= hw.set_zone(z, c).is_ok();
                    }
                    if ok {
                        self.temp_colour = Some(c);
                    }
                }
            }
        }

        // Overheat guard for Custom fan speeds.
        let fans = hw.capabilities().fans;
        if self.applied.fans.mode == FanMode::Custom && !fans.is_empty() {
            let cpu = self.sensors.cpu_temp.unwrap_or(0);
            let gpu = self.sensors.gpu_temp.unwrap_or(0);
            if !self.guard && (cpu >= GUARD_CPU_C || gpu >= GUARD_GPU_C) {
                log::warn!("overheat guard on (CPU {cpu} C, GPU {gpu} C): fans to Max");
                for fan in fans {
                    let _ = hw.set_fan_mode(fan, FanMode::Max);
                }
                self.guard = true;
            } else if self.guard && cpu < GUARD_RELEASE_C && gpu < GUARD_RELEASE_C {
                log::info!("overheat guard off: back to custom fan speeds");
                self.apply();
            }
        }
    }

    fn check_drift(&mut self) {
        if !self.settings.keep_applied || self.guard || self.settings.restored {
            return;
        }
        let Some(hw) = self.hw.as_mut() else { return };
        let seen = hw.observe();
        self.toggles_now = seen.toggles;
        let want_core = hw.capabilities().gpu_level(self.applied.gpu).map(|(_, l)| l.core_mhz);
        if pcore::drifted(&self.applied, &seen, want_core) {
            log::info!("hardware state changed behind our back ({seen:?}), re-applying");
            self.apply();
        }
    }
}

fn adopt_current(hw: &mut Option<Box<dyn Backend>>) -> Settings {
    let mut s = Settings::default();
    let Some(hw) = hw.as_mut() else { return s };
    let seen = hw.observe();
    if let Some(z) = seen.zones {
        s.lighting.effect = Effect::Static;
        s.lighting.zones = z;
        s.lighting.zone_on = z.map(|c| c != Rgb::default());
        s.lighting.zones = z.map(|c| if c == Rgb::default() { s.lighting.colour } else { c });
    }
    if let Some(&(_, first)) = seen.fans.first() {
        if seen.fans.iter().all(|&(_, m)| m == first) {
            s.fans.mode = first;
        }
    }
    if let Some(g) = seen.gpu {
        s.gpu = g;
    }
    s
}

pub fn run(jobs: Receiver<Job>) {
    let (mut hw, open_error) = match backend::open() {
        Ok(h) => (Some(h), None),
        Err(e) => {
            log::error!("opening hardware: {e:#}");
            (None, Some(format!("{e:#}")))
        }
    };
    // First run: start from whatever the laptop is set to now instead of
    // repainting it with defaults the user never picked.
    let (initial, first_run) = match settings::load() {
        Some(s) => (s, false),
        None => (adopt_current(&mut hw), true),
    };
    // Save the laptop's own settings once, before anything is sent to it,
    // so they can be restored later. If an older OpenSenses already changed
    // them, Acer's defaults for the model are the better "original".
    if pcore::snapshot::load().is_none() {
        if let Some(h) = hw.as_mut() {
            let already_changed = !first_run && initial.touched;
            let snap = if already_changed {
                h.factory_snapshot().or_else(|| h.snapshot())
            } else {
                h.snapshot()
            };
            if let Some(s) = snap {
                match pcore::snapshot::save_once(&s) {
                    Ok(()) => log::info!("saved the original settings ({:?})", s.source),
                    Err(e) => log::error!("saving the original settings: {e:#}"),
                }
            }
        }
    }
    if first_run {
        if let Err(e) = settings::save(&initial) {
            log::error!("saving settings: {e:#}");
        }
    }
    let mut d = Daemon {
        hw,
        open_error,
        applied: initial.clone(),
        settings: initial,
        game: None,
        running: HashSet::new(),
        restore_win_key: None,
        sensors: Sensors::default(),
        errors: Vec::new(),
        guard: false,
        temp_colour: None,
        toggles_now: Default::default(),
        turbo: false,
        cpu_sent: None,
        power: None,
        hotkey_group: 1,
        notice: None,
        #[cfg(windows)]
        agent: Default::default(),
    };
    if let Some(hw) = d.hw.as_mut() {
        if !hw.capabilities().toggles.is_empty() {
            d.toggles_now = hw.observe().toggles;
        }
    }
    if d.settings.keep_applied && !first_run && !d.settings.restored {
        d.apply();
    }

    // Ticks run on a fixed schedule even while the app is sending requests
    // every couple of seconds; requests are answered in between.
    let mut next_tick = Instant::now();
    let mut last_tick_wall = SystemTime::now();
    let mut last_drift = Instant::now();
    loop {
        match jobs.recv_timeout(next_tick.saturating_duration_since(Instant::now())) {
            Ok(Job::Ipc(req, reply)) => {
                let _ = reply.send(d.handle(req));
            }
            Ok(Job::Firmware(e)) => d.on_firmware_event(e, false),
            Ok(Job::PredatorSenseKey) => {
                log::info!("PredatorSense key");
                d.run_action(&HotkeyAction::OpenApp);
            }
            Ok(Job::Stop) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }
        if Instant::now() < next_tick {
            continue;
        }
        next_tick = Instant::now() + TICK;

        // Ticks are 2 s apart, so a big jump in wall-clock time between two
        // of them means the laptop was asleep; firmware often forgets
        // lighting and fan settings across sleep.
        let now = SystemTime::now();
        let slept = now.duration_since(last_tick_wall).map(|gap| gap > SLEEP_GAP).unwrap_or(false);
        last_tick_wall = now;
        if slept && d.settings.keep_applied && !d.settings.restored {
            log::info!("woke from sleep, re-applying");
            d.apply();
        }

        d.tick();
        if last_drift.elapsed() >= DRIFT_EVERY {
            last_drift = Instant::now();
            d.check_drift();
        }
    }
    log::info!("daemon stopped");
}
