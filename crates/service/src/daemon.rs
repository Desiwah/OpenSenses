//! The part that owns the hardware. Everything that touches the firmware
//! runs on one thread (COM objects on Windows must stay on their thread);
//! IPC connections hand their requests to it through a channel.

use pcore::backend::{self, Backend, Sensors};
use pcore::ipc::{Request, Response, State};
use pcore::settings::{self, Effect, FanMode, Rgb, Settings};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant, SystemTime};

pub enum Job {
    Ipc(Request, Sender<Response>),
    Stop,
}

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
    settings: Settings,
    sensors: Sensors,
    errors: Vec<String>,
    guard: bool,
    temp_colour: Option<Rgb>,
    toggles_now: pcore::settings::Toggles,
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
        }
    }

    fn apply(&mut self) {
        self.apply_from(None);
    }

    /// Sends what changed since `old` (or everything) and refreshes the
    /// toggle states shown in the app.
    fn apply_from(&mut self, old: Option<&Settings>) {
        let Some(hw) = self.hw.as_mut() else { return };
        self.errors = pcore::apply_changes(hw.as_mut(), old, &self.settings);
        if old.map(|o| o.lighting != self.settings.lighting).unwrap_or(true) {
            self.temp_colour = None;
        }
        if old.map(|o| o.fans != self.settings.fans).unwrap_or(true) {
            self.guard = false;
        }
        if old.map(|o| o.toggles != self.settings.toggles).unwrap_or(true) && hw.capabilities().toggles {
            self.toggles_now = hw.observe().toggles;
        }
        for e in &self.errors {
            log::warn!("{e}");
        }
    }

    fn handle(&mut self, req: Request) -> Response {
        match req {
            Request::GetState => {}
            Request::SetSettings { settings: s } => {
                let old = std::mem::replace(&mut self.settings, s.sanitized());
                if let Err(e) = settings::save(&self.settings) {
                    log::error!("saving settings: {e:#}");
                }
                self.apply_from(Some(&old));
            }
            Request::Reapply => self.apply(),
        }
        Response::State(self.state())
    }

    fn tick(&mut self) {
        let Some(hw) = self.hw.as_mut() else { return };
        self.sensors = hw.sensors();

        // Temperature lighting: repaint only when the colour actually changes.
        if self.settings.lighting.effect == Effect::Temperature {
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
        if self.settings.fans.mode == FanMode::Custom && hw.capabilities().fans {
            let cpu = self.sensors.cpu_temp.unwrap_or(0);
            let gpu = self.sensors.gpu_temp.unwrap_or(0);
            if !self.guard && (cpu >= GUARD_CPU_C || gpu >= GUARD_GPU_C) {
                log::warn!("overheat guard on (CPU {cpu} C, GPU {gpu} C): fans to Max");
                let _ = hw.set_fan_mode(pcore::protocol::FAN_CPU, FanMode::Max);
                let _ = hw.set_fan_mode(pcore::protocol::FAN_GPU, FanMode::Max);
                self.guard = true;
            } else if self.guard && cpu < GUARD_RELEASE_C && gpu < GUARD_RELEASE_C {
                log::info!("overheat guard off: back to custom fan speeds");
                self.apply();
            }
        }
    }

    fn check_drift(&mut self) {
        if !self.settings.keep_applied || self.guard {
            return;
        }
        let Some(hw) = self.hw.as_mut() else { return };
        let seen = hw.observe();
        self.toggles_now = seen.toggles;
        if pcore::drifted(&self.settings, &seen) {
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
    if let (Some(cpu), Some(gpu)) = seen.fans {
        if cpu == gpu {
            s.fans.mode = cpu;
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
    if first_run {
        if let Err(e) = settings::save(&initial) {
            log::error!("saving settings: {e:#}");
        }
    }
    let mut d = Daemon {
        hw,
        open_error,
        settings: initial,
        sensors: Sensors::default(),
        errors: Vec::new(),
        guard: false,
        temp_colour: None,
        toggles_now: Default::default(),
    };
    if let Some(hw) = d.hw.as_mut() {
        if hw.capabilities().toggles {
            d.toggles_now = hw.observe().toggles;
        }
    }
    if d.settings.keep_applied && !first_run {
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
        if slept && d.settings.keep_applied {
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
