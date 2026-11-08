"use strict";

/* ---------- connection to the background service ---------- */

const tauri = window.__TAURI__;
const invoke = tauri ? tauri.core.invoke : mockInvoke;

// Lets the page run in a normal browser for design work.
let mock = null;
function mockCaps() {
  const q = new URLSearchParams(location.search).get("model") || "PH317-53";
  const lv = (mode, core_mhz, memory_mhz) => ({ mode, core_mhz, memory_mhz });
  const base = {
    product: `Predator ${q}`, model_name: "Predator Helios 300", known_model: true, lighting: true, per_key: false,
    fans: ["cpu", "gpu"], sensors: true, sticky_keys: true, coming: [],
    gpu_levels: [lv("normal", 0, 0), lv("faster", 100, 160), lv("turbo", 160, 320)],
    toggles: ["backlight_timeout", "lcd_overdrive", "win_key", "boot_animation", "cool_boost"], experimental: [],
  };
  const all = (t) => ["lighting", "fans", "gpu", "monitor", ...t];
  if (q === "PT515-51") Object.assign(base, { model_name: "Predator Triton 500", fans: ["cpu", "gpu", "gpu2"],
    gpu_levels: [lv("normal", 0, 120), lv("faster", 75, 200), lv("turbo", 100, 270)],
    toggles: ["backlight_timeout", "lcd_overdrive", "discrete_gpu", "win_key", "boot_animation", "cool_boost"] });
  // The PH317-53's BIOS turns the CPU level down, so only the fan curve follows.
  base.cpu_follows_gpu = false; base.turbo_key = true; base.fan_table = true; base.cpu_levels = []; base.hotkey_keys = 0;
  if (q === "PH717-71") Object.assign(base, { model_name: "Predator Helios 700", lighting: false, per_key: true,
    gpu_levels: [lv("normal", 0, 0), lv("faster", 60, 133), lv("boosted", 60, 266), lv("turbo", 100, 400)],
    toggles: ["backlight_timeout", "win_key", "boot_animation", "cool_boost"], cpu_follows_gpu: false, cpu_levels: ["normal", "faster", "boosted", "turbo"], hotkey_keys: 3, ps_key: true,
    per_key_layout: 0, heat_pipe_light: true, touchpad_light: 1 });
  if (q === "PT917-71") Object.assign(base, { model_name: "Predator Triton 900", lighting: false, per_key: true,
    toggles: ["backlight_timeout", "win_key", "boot_animation", "cool_boost"], cpu_follows_gpu: false, hotkey_keys: 3, ps_key: true,
    per_key_layout: 1, fan_light: true, touchpad_light: 2 });
  if (q === "PH517-51") Object.assign(base, { model_name: "Predator Helios 500", toggles: ["win_key", "cool_boost"], cpu_follows_gpu: false, turbo_key: false, hotkey_keys: 5, ps_key: true });
  if (q === "unknown") Object.assign(base, { product: "Nitro AN515-55", model_name: "Nitro 5", known_model: false,
    gpu_levels: [], toggles: ["backlight_timeout", "boot_animation"] });
  if (q !== "PH317-53") base.experimental = all(base.toggles);
  return base;
}
function mockInvoke(cmd, args) {
  if (!mock) {
    const teal = { r: 0, g: 174, b: 199 };
    mock = {
      settings: {
        lighting: { effect: "static", zones: [teal, { r: 70, g: 120, b: 255 }, { r: 190, g: 70, b: 255 }, { r: 255, g: 70, b: 120 }],
          zone_on: [true, true, true, true], colour: teal, speed: 4, brightness: 100, direction: "right_to_left", keys: [], extras: {} },
        fans: { mode: "auto", cpu_percent: 50, gpu_percent: 50 },
        gpu: "normal", cpu: "normal", turbo_key: "performance", hotkeys: [], keep_applied: true, game_sync: true,
        games: [{ exe: "eldenring.exe", enabled: true, lighting: null, fans: { mode: "max", cpu_percent: 50, gpu_percent: 50, gpu2_percent: 50 }, gpu: "turbo", win_key: false }],
      },
      sensors: {}, capabilities: mockCaps(),
      backend: "preview", errors: [], overheat_guard: false, turbo: false, cpu_level: 0, power: [true, 87], hotkey_group: 1, notice: null, original: { taken: 1794100000, source: location.search.includes("factory") ? "factory" : "laptop" },
      toggles_now: { lcd_overdrive: false, boot_animation: false, backlight_timeout: true, win_key: true, discrete_gpu: false, cool_boost: false },
    };
    mock.settings.toggles = { lcd_overdrive: null, boot_animation: null, backlight_timeout: null, win_key: null, discrete_gpu: null, cool_boost: null };
    mock.settings.fans.gpu2_percent = 50;
    mock.settings.fahrenheit = false;
    mock.sticky = false;
  }
  if (location.search.includes("noservice")) return Promise.reject("background service is not running");
  const t = Date.now() / 1000;
  mock.sensors = {
    cpu_temp: Math.round(66 + 6 * Math.sin(t / 7)), gpu_temp: Math.round(58 + 4 * Math.sin(t / 9)),
    sys_temp: 49, cpu_fan_rpm: Math.round(3000 + 300 * Math.sin(t / 5)), gpu_fan_rpm: Math.round(3450 + 250 * Math.sin(t / 6)),
  };
  if (cmd === "get_sticky_keys") return Promise.resolve(mock.sticky);
  if (cmd === "running_apps") return Promise.resolve(["discord.exe", "eldenring.exe", "steam.exe", "cyberpunk2077.exe"].map((name) => ({ name, path: `C:\\Games\\${name}` })));
  if (cmd === "app_icon") return Promise.resolve(null);
  if (cmd === "find_program" || cmd === "pick_program") return Promise.resolve(null);
  mock.active_game = location.search.includes("playing") ? "eldenring.exe" : null;
  if (cmd === "set_sticky_keys") { mock.sticky = args.on; return Promise.resolve(null); }
  if (cmd === "set_turbo") { mock.turbo = args.on; mock.notice = [(mock.notice ? mock.notice[0] : 0) + 1, args.on ? "Turbo on" : "Turbo off"]; }
  if (cmd === "test_key") mock.notice = [(mock.notice ? mock.notice[0] : 0) + 1, `Key ${args.detail[2]} pressed (preview)`];
  if (cmd === "restore_original") { mock.settings.restored = true; mock.settings.fans.mode = "auto"; }
  if (cmd === "set_settings") {
    mock.settings = JSON.parse(JSON.stringify(args.settings));
    for (const [k, v] of Object.entries(mock.settings.toggles)) if (v != null) mock.toggles_now[k] = v;
  }
  return Promise.resolve(JSON.parse(JSON.stringify(mock)));
}

/* ---------- state ---------- */

let state = null;        // last state from the service
let settings = null;     // what the user is editing
let selectedZone = 0;
let saveTimer = null;
let lastToggles = "";
let lastGame;
let lastLive = "";
const history = { cpu_temp: [], gpu_temp: [], sys_temp: [], cpu_fan_rpm: [], gpu_fan_rpm: [] };
const $ = (id) => document.getElementById(id);

const BASIC = ["#00AEC7", "#FF0000", "#FFA000", "#FFFF00", "#00FF00", "#001EFF", "#FF00FF", "#AA00FF", "#FFFFFF"];
const EFFECTS = [
  ["static", "Static", "Each zone keeps its own colour. Click a zone on the keyboard to change it."],
  ["breathing", "Breathing", "One colour fades in and out."],
  ["neon", "Neon", "The whole keyboard cycles through every colour."],
  ["wave", "Wave", "A rainbow moves across the keyboard."],
  ["shifting", "Shifting", "A band of colour sweeps across the keyboard."],
  ["zoom", "Zoom", "Colour pulses outward from the middle."],
  ["temperature", "Temperature", "The keyboard shows how hot the CPU is: blue when cool, through green and yellow, to red when hot."],
];
// The per-key keyboards' built-in effects (Helios 700, Triton 900).
const PER_KEY_EFFECTS = [
  ["static", "Static", "Every key keeps its own colour. Click keys on the keyboard to pick them (Ctrl or Shift adds more), then choose a colour."],
  ["breathing", "Breathing", "One colour fades in and out."],
  ["wave", "Wave", "A rainbow moves across the keyboard."],
  ["reactive", "Press to light", "Keys light up in the colour when you press them."],
  ["ripple", "Ripple", "Each key press sends a ring of colour across the keyboard."],
  ["marquee", "Marquee", "A band of the colour runs along the keyboard."],
  ["rainbow_marquee", "Colourful marquee", "Bands of every colour run along the keyboard."],
  ["raindrop", "Rain drop", "Keys light up at random, like rain."],
  ["circle_marquee", "Circle marquee", "Colour circles around the keyboard."],
  ["hedge", "Hedge", "Rows of colour move across the keyboard."],
  ["spiral", "Rotate", "Colour turns around the middle of the keyboard."],
  ["neon", "Neon", "The whole keyboard cycles through every colour."],
  ["shifting", "Shifting", "A band of colour sweeps across the keyboard."],
  ["zoom", "Zoom", "Colour pulses outward from the middle."],
];
const PER_KEY_COLOURED = ["breathing", "reactive", "ripple", "marquee", "raindrop", "circle_marquee", "hedge", "shifting", "zoom"];
const BRIGHTNESS = [[0, "Off"], [25, "25%"], [50, "50%"], [75, "75%"], [100, "100%"]];
const DIRECTIONS = [["right_to_left", "Right to left"], ["left_to_right", "Left to right"]];
const WAVE_DIRECTIONS = [["right_to_left", "Left"], ["left_to_right", "Right"], ["up", "Up"], ["down", "Down"]];
const SPIRAL_DIRECTIONS = [["right_to_left", "Anticlockwise"], ["left_to_right", "Clockwise"]];
const TOUCHPAD_SLOT = 126;
const perKeyMode = () => { const c = (state && state.capabilities) || {}; return !!c.per_key && !c.lighting; };
const effectList = () => (perKeyMode() ? PER_KEY_EFFECTS : EFFECTS);
let selectedKeys = new Set();
let layoutKey = "";
const FAN_MODES = [["auto", "Auto"], ["max", "Max"], ["custom", "Custom"]];
const GPU_LEVEL_NAMES = { normal: "Normal", faster: "Faster", boosted: "Boosted", turbo: "Turbo" };
const GPU_LEVEL_HINTS = { normal: "Stock clocks", faster: "A mild overclock", boosted: "A stronger overclock", turbo: "The highest preset" };
const FAN_NAMES = { cpu: "CPU fan", gpu: "GPU fan", gpu2: "Second GPU fan" };
const mhz = (v) => `${v >= 0 ? "+" : ""}${v} MHz`;

// Windows programs end in .exe; Linux ones usually have no extension, though
// Windows games run through Proton still show up as game.exe.
const IS_WINDOWS = /Windows/.test(navigator.userAgent);

// Features not tested on this model's hardware get "(experimental)".
const isExp = (key) => !!(state && state.capabilities && (state.capabilities.experimental || []).includes(key));
const expTitle = (title, key) => (isExp(key) ? `${title} (experimental)` : title);

const hex = (c) => "#" + [c.r, c.g, c.b].map((v) => v.toString(16).padStart(2, "0")).join("").toUpperCase();
const rgb = (h) => ({ r: parseInt(h.slice(1, 3), 16), g: parseInt(h.slice(3, 5), 16), b: parseInt(h.slice(5, 7), 16) });
const clone = (o) => JSON.parse(JSON.stringify(o));

/* ---------- talking to the service ---------- */

function setStatus(text, isError) {
  const el = $("statusMsg");
  el.textContent = text || "";
  el.classList.toggle("err", !!isError);
}

function showErrors(st) {
  if (st.errors && st.errors.length) setStatus(st.errors[0], true);
}

async function refresh(first) {
  try {
    const st = await invoke("get_state");
    $("noService").hidden = true;
    state = st;
    if (first || !settings) {
      settings = clone(st.settings);
      renderAll();
    }
    renderLive();
    if (first) {
      showErrors(st);
      try { stickyKeys = await invoke("get_sticky_keys"); } catch { stickyKeys = null; }
      renderSettings();
    } else if (JSON.stringify(st.toggles_now) !== lastToggles) {
      renderSettings();
      renderFans();
    }
    lastToggles = JSON.stringify(st.toggles_now);
    showNotice(false);
    const live = JSON.stringify([st.turbo, st.cpu_level, st.power, st.hotkey_group]);
    if (live !== lastLive) {
      lastLive = live;
      renderTurbo();
      if (st.hotkey_group) { keyGroup = st.hotkey_group - 1; renderKeys(); }
      // The Turbo key or a programmable key changed levels behind the app's back.
      if (!first && JSON.stringify([st.settings.gpu, st.settings.cpu, st.settings.fans.mode]) !== JSON.stringify([settings.gpu, settings.cpu, settings.fans.mode])) {
        settings = clone(st.settings);
        renderAll();
      }
    }
    if (st.active_game !== lastGame) { lastGame = st.active_game; renderGames(); if (lastGame) loadRunningApps(); }
  } catch (e) {
    $("noService").hidden = false;
    $("nsDetail").textContent = String(e);
  }
}

function save() {
  clearTimeout(saveTimer);
  // The per-key keyboard takes a moment per change; send fewer of them.
  const wait = perKeyMode() ? 500 : 140;
  saveTimer = setTimeout(async () => {
    try {
      state = await invoke("set_settings", { settings });
      if (state.errors && state.errors.length) showErrors(state);
      else setStatus("Applied");
    } catch (e) {
      setStatus(String(e), true);
    }
  }, wait);
}

function change(fn) {
  fn(settings);
  // Any change ends the hands-off state after a restore.
  settings.restored = false;
  renderAll();
  save();
}

/* ---------- small builders ---------- */

function segmented(el, items, current, onPick, disabled) {
  el.replaceChildren(...items.map(([value, label]) => {
    const b = document.createElement("button");
    b.type = "button";
    b.setAttribute("role", "radio");
    b.setAttribute("aria-checked", String(value === current));
    b.textContent = label;
    b.disabled = !!disabled;
    b.onclick = () => onPick(value);
    return b;
  }));
}

/* ---------- keyboard drawing ---------- */

// Key widths per row, in key units: the main block (15 units wide) and the number pad (4).
const rep = (w, n) => Array(n).fill(w);
const ROWS = [
  [rep(1, 15), rep(1, 4)],
  [[...rep(1, 13), 2], rep(1, 4)],
  [[1.5, ...rep(1, 12), 1.5], rep(1, 4)],
  [[1.75, ...rep(1, 11), 2.25], rep(1, 4)],
  [[2.25, ...rep(1, 10), 1.75, 1], rep(1, 4)],
  [[1, 1, 1, 1, 5, 1, 1, 1, 1, 1, 1], [2, 1, 1]],
];
let keys = [];

function perKeyLayout() {
  const caps = (state && state.capabilities) || {};
  const model = (caps.product || "").includes("PT917") ? "PT917-71" : "PH717-71";
  return { name: `${model}/${caps.per_key_layout ? "uk" : "us"}`, keys: PER_KEY_LAYOUTS[`${model}/${caps.per_key_layout ? "uk" : "us"}`] || [] };
}

// The per-key keyboard, drawn from the model's own key positions.
function drawPerKeyboard() {
  const svg = $("keyboard");
  const NS = "http://www.w3.org/2000/svg";
  const { name, keys: layout } = perKeyLayout();
  layoutKey = name;
  svg.replaceChildren();
  const defs = document.createElementNS(NS, "defs");
  defs.innerHTML = '<filter id="glow" x="-20%" y="-40%" width="140%" height="180%"><feGaussianBlur stdDeviation="6" result="b"/><feMerge><feMergeNode in="b"/><feMergeNode in="SourceGraphic"/></feMerge></filter>';
  svg.appendChild(defs);
  const maxX = Math.max(...layout.map((k) => k.x + k.w)), maxY = Math.max(...layout.map((k) => k.y + k.h));
  const s = Math.min(948 / maxX, 290 / maxY);
  const ox = (1000 - maxX * s) / 2, oy = (330 - maxY * s) / 2;
  const deck = document.createElementNS(NS, "rect");
  deck.setAttribute("class", "deck");
  deck.setAttribute("x", 6); deck.setAttribute("y", 6);
  deck.setAttribute("width", 988); deck.setAttribute("height", 318); deck.setAttribute("rx", 18);
  svg.appendChild(deck);
  const g = document.createElementNS(NS, "g");
  g.setAttribute("filter", "url(#glow)");
  svg.appendChild(g);
  const labels = document.createElementNS(NS, "g");
  labels.setAttribute("class", "legends");
  keys = [];
  for (const k of layout) {
    const rect = document.createElementNS(NS, "rect");
    rect.setAttribute("class", "key");
    rect.setAttribute("x", (ox + k.x * s + 2).toFixed(1));
    rect.setAttribute("y", (oy + k.y * s + 2).toFixed(1));
    rect.setAttribute("width", (k.w * s - 4).toFixed(1));
    rect.setAttribute("height", (k.h * s - 4).toFixed(1));
    rect.setAttribute("rx", 5);
    rect.setAttribute("aria-label", k.l || "Space");
    rect.onclick = (e) => pickKey(k.t, e.ctrlKey || e.shiftKey || e.metaKey);
    g.appendChild(rect);
    if (k.l) {
      const t = document.createElementNS(NS, "text");
      t.setAttribute("x", (ox + (k.x + k.w / 2) * s).toFixed(1));
      t.setAttribute("y", (oy + (k.y + k.h / 2) * s + 4).toFixed(1));
      t.textContent = k.l;
      labels.appendChild(t);
    }
    keys.push({ el: rect, slot: k.t, zone: 0, nx: (k.x + k.w / 2) / maxX, ny: (k.y + k.h / 2) / maxY, rnd: Math.random() * 6.28 });
  }
  svg.appendChild(labels);
}

function pickKey(slot, add) {
  if (add) { if (selectedKeys.has(slot)) selectedKeys.delete(slot); else selectedKeys.add(slot); }
  else selectedKeys = new Set([slot]);
  if (settings.lighting.effect !== "static") change((s) => (s.lighting.effect = "static"));
  else renderAll();
}

// Every slot's colour, so single keys can change without losing the rest.
function fullKeys(l) {
  const out = [];
  for (let i = 0; i <= TOUCHPAD_SLOT; i++) out.push(clone((l.keys && l.keys[i]) || l.colour));
  return out;
}

function drawKeyboard() {
  if (perKeyMode()) return drawPerKeyboard();
  layoutKey = "";
  const svg = $("keyboard");
  const NS = "http://www.w3.org/2000/svg";
  const u = 48, gap = 5, padX = 26, padY = 22, padGap = 22;
  svg.replaceChildren();

  const defs = document.createElementNS(NS, "defs");
  defs.innerHTML = '<filter id="glow" x="-20%" y="-40%" width="140%" height="180%"><feGaussianBlur stdDeviation="7" result="b"/><feMerge><feMergeNode in="b"/><feMergeNode in="SourceGraphic"/></feMerge></filter>';
  svg.appendChild(defs);

  const deck = document.createElementNS(NS, "rect");
  deck.setAttribute("class", "deck");
  deck.setAttribute("x", 6); deck.setAttribute("y", 6);
  deck.setAttribute("width", 988); deck.setAttribute("height", 318); deck.setAttribute("rx", 18);
  svg.appendChild(deck);

  const g = document.createElementNS(NS, "g");
  g.setAttribute("filter", "url(#glow)");
  svg.appendChild(g);
  keys = [];
  const mainW = 15 * u;

  ROWS.forEach((row, r) => {
    const y = padY + r * (u + gap - 3);
    const h = r === 0 ? u * 0.72 : u - 3;
    const yy = r === 0 ? y + u * 0.18 : y;
    [0, 1].forEach((part) => {
      const widths = row[part];
      const total = widths.reduce((a, b) => a + b, 0);
      const span = part === 0 ? mainW : 4 * u;
      const scale = span / (total * u);
      let x = padX + (part === 0 ? 0 : mainW + padGap);
      widths.forEach((w) => {
        const kw = w * u * scale;
        const rect = document.createElementNS(NS, "rect");
        rect.setAttribute("class", "key");
        rect.setAttribute("x", (x + gap / 2).toFixed(1));
        rect.setAttribute("y", yy.toFixed(1));
        rect.setAttribute("width", (kw - gap).toFixed(1));
        rect.setAttribute("height", h.toFixed(1));
        rect.setAttribute("rx", 6);
        const cx = x + kw / 2;
        const zone = part === 1 ? 3 : Math.min(2, Math.floor(((cx - padX) / mainW) * 3));
        const nx = [0.12, 0.37, 0.62, 0.9][zone];
        rect.onclick = () => { selectedZone = zone; if (settings.lighting.effect !== "static") change((s) => (s.lighting.effect = "static")); else renderAll(); };
        g.appendChild(rect);
        keys.push({ el: rect, zone, nx });
        x += kw;
      });
    });
  });
}

/* ---------- lighting preview ---------- */

function hsv(h, s, v) {
  const f = (n) => { const k = (n + h / 60) % 6; return v - v * s * Math.max(0, Math.min(k, 4 - k, 1)); };
  return { r: f(5) * 255, g: f(3) * 255, b: f(1) * 255 };
}

function tempColour(t) {
  const stops = [[45, [0, 80, 255]], [60, [0, 255, 60]], [72, [255, 220, 0]], [82, [255, 90, 0]], [92, [255, 0, 0]]];
  if (t <= stops[0][0]) return { r: 0, g: 80, b: 255 };
  for (let i = 1; i < stops.length; i++) {
    if (t <= stops[i][0]) {
      const [t0, a] = stops[i - 1], [t1, b] = stops[i];
      const k = (t - t0) / (t1 - t0);
      return { r: a[0] + (b[0] - a[0]) * k, g: a[1] + (b[1] - a[1]) * k, b: a[2] + (b[2] - a[2]) * k };
    }
  }
  return { r: 255, g: 0, b: 0 };
}

const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

function keyColour(k, t) {
  const l = settings.lighting;
  const sp = l.speed / 4;
  const dir = l.direction === "left_to_right" ? -1 : 1;
  const fade = (a) => ({ r: l.colour.r * a, g: l.colour.g * a, b: l.colour.b * a });
  if (k.slot != null) {
    // The touchpad light keeps its own colour; effects are the keyboard's.
    if (k.slot === TOUCHPAD_SLOT) return (l.keys && l.keys[k.slot]) || l.colour;
    switch (l.effect) {
      case "static": return (l.keys && l.keys[k.slot]) || l.colour;
      case "reactive": return fade(0.12 + 0.88 * Math.max(0, Math.sin(t * sp * 2 + k.rnd * 3)) ** 12);
      case "raindrop": return fade(0.08 + 0.92 * Math.max(0, Math.sin(t * sp * 1.3 + k.rnd * 5)) ** 16);
      case "ripple": case "circle_marquee": {
        const d = Math.hypot(k.nx - 0.5, (k.ny - 0.5) * 0.4);
        return fade(0.1 + 0.9 * Math.max(0, Math.cos((d * 14 - t * sp * 3) % (Math.PI * 2))));
      }
      case "marquee": case "hedge": {
        const pos = ((dir * t * sp * 0.35) % 1.4 + 1.4) % 1.4 - 0.2;
        return fade(Math.max(0.08, 1 - Math.abs((l.effect === "hedge" ? k.ny : k.nx) - pos) * 4));
      }
      case "rainbow_marquee": return hsv(((k.nx * 720 - t * sp * 120) % 360 + 360) % 360, 1, 1);
      case "spiral": return hsv(((Math.atan2(k.ny - 0.5, (k.nx - 0.5) * 2.5) * 57.3 + dir * t * sp * 90) % 360 + 360) % 360, 1, 1);
      case "wave": {
        const along = l.direction === "up" || l.direction === "down" ? k.ny * (l.direction === "up" ? 1 : -1) : k.nx * dir;
        return hsv(((along * 360 + t * sp * 90) % 360 + 360) % 360, 1, 1);
      }
    }
  }
  switch (l.effect) {
    case "static": return l.zone_on[k.zone] ? l.zones[k.zone] : { r: 0, g: 0, b: 0 };
    case "temperature": return tempColour((state && state.sensors.cpu_temp) || 50);
    case "breathing": {
      const a = 0.12 + 0.88 * (0.5 - 0.5 * Math.cos(t * sp * 1.6));
      return { r: l.colour.r * a, g: l.colour.g * a, b: l.colour.b * a };
    }
    case "neon": return hsv((t * sp * 40) % 360, 1, 1);
    case "wave": return hsv(((k.nx * 360 + dir * t * sp * 90) % 360 + 360) % 360, 1, 1);
    case "shifting": {
      const pos = ((dir * t * sp * 0.35) % 1.4 + 1.4) % 1.4 - 0.2;
      const a = Math.max(0.08, 1 - Math.abs(k.nx - pos) * 4);
      return { r: l.colour.r * a, g: l.colour.g * a, b: l.colour.b * a };
    }
    case "zoom": {
      const d = Math.abs(k.nx - 0.5);
      const a = 0.1 + 0.9 * Math.max(0, Math.cos((d * 9 - t * sp * 3) % (Math.PI * 2)));
      return { r: l.colour.r * a, g: l.colour.g * a, b: l.colour.b * a };
    }
  }
  return { r: 0, g: 0, b: 0 };
}

function paint(t) {
  if (!settings) return;
  const b = settings.lighting.brightness / 100;
  for (const k of keys) {
    const c = keyColour(k, t);
    const m = (v) => Math.round(Math.max(18, v * b));
    k.el.style.fill = `rgb(${m(c.r)},${m(c.g)},${m(c.b)})`;
  }
}

function loop(ms) {
  const animated = settings && !["static", "temperature"].includes(settings.lighting.effect);
  if (animated && !reduceMotion) paint(ms / 1000);
  requestAnimationFrame(loop);
}

/* ---------- colour picker ---------- */

function currentColour() {
  const l = settings.lighting;
  if (perKeyMode() && l.effect === "static") {
    const first = [...selectedKeys][0];
    return first != null ? (l.keys && l.keys[first]) || l.colour : l.colour;
  }
  return l.effect === "static" ? l.zones[selectedZone] : l.colour;
}

// Changes the colour being edited without rebuilding the page, so a drag in
// progress isn't interrupted; the keyboard preview and the laptop follow along.
function setTargetColour(c) {
  const l = settings.lighting;
  if (perKeyMode() && l.effect === "static") {
    if (!selectedKeys.size) { setStatus("Click the keys to colour first"); return; }
    l.keys = fullKeys(l);
    for (const k of selectedKeys) l.keys[k] = clone(c);
  } else if (l.effect === "static") { l.zones[selectedZone] = c; l.zone_on[selectedZone] = true; }
  else l.colour = c;
  paint(performance.now() / 1000);
  const pill = $("zonePills").children[selectedZone];
  if (pill && l.effect === "static" && !perKeyMode()) {
    pill.querySelector(".dot").style.background = hex(c);
    pill.querySelector(".hex").textContent = hex(c);
    pill.querySelector("input").checked = true;
  }
  save();
}

function rgbToHsv({ r, g, b }) {
  r /= 255; g /= 255; b /= 255;
  const max = Math.max(r, g, b), min = Math.min(r, g, b), d = max - min;
  let h = 0;
  if (d) {
    if (max === r) h = ((g - b) / d) % 6;
    else if (max === g) h = (b - r) / d + 2;
    else h = (r - g) / d + 4;
    h = (h * 60 + 360) % 360;
  }
  return { h, s: max ? d / max : 0, v: max };
}

function hsvToRgb({ h, s, v }) {
  const c = hsv(h, s, v);
  return { r: Math.round(c.r), g: Math.round(c.g), b: Math.round(c.b) };
}

function parseHex(text) {
  let t = text.trim().replace(/^#/, "");
  if (/^[0-9a-f]{3}$/i.test(t)) t = t.split("").map((ch) => ch + ch).join("");
  return /^[0-9a-f]{6}$/i.test(t) ? rgb("#" + t) : null;
}

const picker = {
  hsv: { h: 0, s: 1, v: 1 },
  dragging: false,

  // Shows a colour that was set from elsewhere (zone switch, swatch, hex).
  sync(c) {
    const next = rgbToHsv(c);
    if (next.s === 0 || next.v === 0) next.h = this.hsv.h; // greys and black keep the hue
    this.hsv = next;
    this.draw(c);
  },

  draw(c) {
    const { h, s, v } = this.hsv;
    $("sv").style.setProperty("--hue-colour", `hsl(${h}, 100%, 50%)`);
    $("svHandle").style.left = `${s * 100}%`;
    $("svHandle").style.top = `${(1 - v) * 100}%`;
    $("svHandle").style.background = hex(c);
    $("hueHandle").style.left = `${(h / 360) * 100}%`;
    $("hueHandle").style.background = `hsl(${h}, 100%, 50%)`;
    $("chip").style.background = hex(c);
    $("hue").setAttribute("aria-valuenow", String(Math.round(h)));
    const input = $("hexInput");
    if (document.activeElement !== input) { input.value = hex(c); input.removeAttribute("aria-invalid"); }
    document.querySelectorAll(".swatch").forEach((b) => b.setAttribute("aria-checked", String(b.getAttribute("aria-label") === hex(c))));
  },

  commit() {
    const c = hsvToRgb(this.hsv);
    this.draw(c);
    setTargetColour(c);
  },

  // Press anywhere to move the circle there, then keep dragging to fine-tune.
  attach(el, onMove) {
    el.addEventListener("pointerdown", (e) => {
      e.preventDefault();
      el.focus();
      el.setPointerCapture(e.pointerId);
      this.dragging = true;
      onMove(e);
    });
    el.addEventListener("pointermove", (e) => { if (this.dragging && el.hasPointerCapture(e.pointerId)) onMove(e); });
    const end = (e) => {
      if (!this.dragging) return;
      this.dragging = false;
      if (el.hasPointerCapture(e.pointerId)) el.releasePointerCapture(e.pointerId);
      renderAll();
    };
    el.addEventListener("pointerup", end);
    el.addEventListener("pointercancel", end);
  },

  init() {
    const sv = $("sv"), hue = $("hue");
    const clamp = (x) => Math.min(1, Math.max(0, x));
    this.attach(sv, (e) => {
      const r = sv.getBoundingClientRect();
      this.hsv.s = clamp((e.clientX - r.left) / r.width);
      this.hsv.v = 1 - clamp((e.clientY - r.top) / r.height);
      this.commit();
    });
    this.attach(hue, (e) => {
      const r = hue.getBoundingClientRect();
      this.hsv.h = clamp((e.clientX - r.left) / r.width) * 359.9;
      this.commit();
    });
    sv.addEventListener("keydown", (e) => {
      const step = e.shiftKey ? 0.1 : 0.02;
      const moves = { ArrowLeft: ["s", -step], ArrowRight: ["s", step], ArrowUp: ["v", step], ArrowDown: ["v", -step] };
      if (!moves[e.key]) return;
      e.preventDefault();
      const [k, d] = moves[e.key];
      this.hsv[k] = clamp(this.hsv[k] + d);
      this.commit();
    });
    hue.addEventListener("keydown", (e) => {
      const d = { ArrowLeft: -1, ArrowRight: 1 }[e.key];
      if (!d) return;
      e.preventDefault();
      this.hsv.h = (this.hsv.h + d * (e.shiftKey ? 15 : 3) + 360) % 360;
      this.commit();
    });

    const input = $("hexInput");
    input.addEventListener("input", () => {
      const c = parseHex(input.value);
      input.setAttribute("aria-invalid", String(!c && input.value.trim() !== ""));
      if (c) { this.sync(c); setTargetColour(c); }
    });
    input.addEventListener("keydown", (e) => { if (e.key === "Enter") input.blur(); });
    input.addEventListener("blur", () => renderAll());
    input.addEventListener("focus", () => input.select());

    $("copyHex").onclick = async () => {
      const text = hex(currentColour());
      try { await navigator.clipboard.writeText(text); }
      catch { input.focus(); input.select(); document.execCommand("copy"); }
      setStatus(`Copied ${text}`);
    };
    $("allZones").onclick = () => change((s) => {
      const c = clone(currentColour());
      if (perKeyMode()) { s.lighting.keys = fullKeys(s.lighting).map((k, i) => (i === TOUCHPAD_SLOT ? k : clone(c))); return; }
      s.lighting.zones = [0, 1, 2, 3].map(() => clone(c));
      s.lighting.zone_on = [true, true, true, true];
    });
  },
};

/* ---------- temperature unit ---------- */

const toUnit = (c) => (c == null ? null : settings && settings.fahrenheit ? Math.round((c * 9) / 5 + 32) : c);
const unitLabel = () => (settings && settings.fahrenheit ? "°F" : "°C");

/* ---------- settings page ---------- */

const TOGGLES = [
  ["discrete_gpu", "Use only the NVIDIA GPU", "Turns off hybrid graphics for a little more performance and lower latency in games, at the cost of battery life. Takes effect after a restart."],
  ["backlight_timeout", "Turn keyboard lighting off when idle", "The lighting goes dark after 30 seconds without typing and comes back with the next key press."],
  ["lcd_overdrive", "LCD overdrive", "Makes the laptop screen respond faster, so fast motion in games looks less blurry."],
  ["win_key", "Windows and Menu keys", "Turn off to block both keys, so a stray press can't pull you out of a game."],
  ["boot_animation", "Boot animation and sound", "Plays the Predator logo animation and start-up sound when the laptop turns on."],
];
let stickyKeys = null;

function switchRow(title, desc, checked, disabled, onChange) {
  const row = document.createElement("div");
  row.className = "toggle-row";
  row.innerHTML = `<h3></h3><p></p><label class="sw"><input type="checkbox" role="switch"><span></span></label>`;
  row.querySelector("h3").textContent = title;
  row.querySelector("p").textContent = desc;
  const input = row.querySelector("input");
  input.setAttribute("aria-label", title);
  input.checked = !!checked;
  input.disabled = !!disabled;
  input.onchange = () => onChange(input.checked);
  return row;
}

function renderSettings() {
  const caps = (state && state.capabilities) || {};
  const now = (state && state.toggles_now) || {};
  const available = caps.toggles || [];
  const rows = TOGGLES.filter(([key]) => available.includes(key)).map(([key, title, desc]) => {
    const chosen = settings.toggles ? settings.toggles[key] : null;
    // The backlight timeout can't be read back reliably; it's on from the factory.
    const fallback = key === "backlight_timeout" ? true : null;
    const value = chosen != null ? chosen : now[key] != null ? now[key] : fallback;
    return switchRow(expTitle(title, key), desc, value, false,
      (on) => change((s) => { s.toggles = s.toggles || {}; s.toggles[key] = on; }));
  });
  if (caps.sticky_keys && stickyKeys != null) rows.push(switchRow("Sticky Keys",
    IS_WINDOWS
      ? "A Windows accessibility feature: Shift, Ctrl, Alt and the Windows key stay pressed until you press the next key. This is a setting of your Windows account."
      : "An accessibility feature: Shift, Ctrl, Alt and the Super key stay pressed until you press the next key. This is a setting of your desktop.",
    stickyKeys, false,
    async (on) => {
      try { await invoke("set_sticky_keys", { on }); stickyKeys = on; setStatus(on ? "Sticky Keys on" : "Sticky Keys off"); }
      catch (e) { setStatus(String(e), true); }
    }));
  $("toggles").replaceChildren(...rows);
  $("toggles").hidden = rows.length === 0;
  const coming = caps.coming || [];
  $("coming").hidden = coming.length === 0;
  $("comingList").textContent = coming.join(", ") + ".";
  $("expNote").hidden = (caps.experimental || []).length === 0;
  renderRestore();
  $("modelLine").textContent = caps.product
    ? `${caps.model_name ? caps.model_name + ", " : ""}${caps.product}${caps.known_model ? "" : " (features found by testing what this laptop answers to)"}`
    : "";
  segmented($("unit"), [[false, "Celsius (°C)"], [true, "Fahrenheit (°F)"]], !!settings.fahrenheit,
    (v) => change((s) => (s.fahrenheit = v)));
}

/* ---------- games (GameSync) ---------- */

const EFFECT_NAMES = Object.fromEntries(EFFECTS.map(([k, n]) => [k, n]));
const gameTitle = (exe) => {
  const base = exe.replace(/\.exe$/i, "");
  return base.charAt(0).toUpperCase() + base.slice(1);
};
const exeName = (text) => text.trim().split(/[\\/]/).pop().toLowerCase();
let runningApps = [];        // [{ name, path }] from the last look at running programs
const iconCache = new Map(); // program path -> data URL, or null when it has none

// Small line icons for the profile rows.
const ROW_ICONS = {
  lighting: '<rect x="2" y="6" width="20" height="12" rx="2"/><path d="M6 10h.01M10 10h.01M14 10h.01M18 10h.01M7 14h10"/>',
  fans: '<circle cx="12" cy="12" r="2"/><path d="M12 10c-1-4 1-7 3-7s2 3-1 6M14 12c4-1 7 1 7 3s-3 2-6-1M12 14c1 4-1 7-3 7s-2-3 1-6M10 12c-4 1-7-1-7-3s3-2 6 1"/>',
  gpu: '<rect x="6" y="6" width="12" height="12" rx="1.5"/><rect x="9.5" y="9.5" width="5" height="5"/><path d="M9 2v4M15 2v4M9 18v4M15 18v4M2 9h4M2 15h4M18 9h4M18 15h4"/>',
  winkey: '<rect x="3" y="3" width="18" height="18" rx="3"/><rect x="7" y="7" width="10" height="9" rx="1.5"/>',
};
function rowIcon(name) {
  const NS = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(NS, "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("fill", "none");
  svg.setAttribute("stroke", "currentColor");
  svg.setAttribute("stroke-width", "1.7");
  svg.setAttribute("stroke-linecap", "round");
  svg.setAttribute("stroke-linejoin", "round");
  svg.setAttribute("aria-hidden", "true");
  svg.innerHTML = ROW_ICONS[name];
  return svg;
}

// A program's own icon when its file is known, else its first letter.
function programIcon(name, path) {
  const letter = document.createElement("span");
  letter.className = "gicon letter";
  letter.textContent = gameTitle(name).charAt(0);
  let h = 0;
  for (const ch of name) h = (h * 31 + ch.charCodeAt(0)) % 360;
  letter.style.setProperty("--h", h);
  if (!path) return letter;
  const img = (url) => {
    const i = document.createElement("img");
    i.className = "gicon"; i.alt = ""; i.src = url;
    return i;
  };
  if (iconCache.has(path)) return iconCache.get(path) ? img(iconCache.get(path)) : letter;
  invoke("app_icon", { path })
    .then((url) => { iconCache.set(path, url || null); if (url && letter.isConnected) letter.replaceWith(img(url)); })
    .catch(() => iconCache.set(path, null));
  return letter;
}

// Games added by name: look up where their program lives once per session.
const lookedUp = new Set();
function findMissingPaths() {
  for (const g of settings.games || []) {
    if (g.path || lookedUp.has(g.exe)) continue;
    lookedUp.add(g.exe);
    invoke("find_program", { exe: g.exe }).then((path) => {
      if (!path) return;
      const i = settings.games.findIndex((x) => x.exe === g.exe && !x.path);
      if (i >= 0) change((s) => (s.games[i].path = path));
    }).catch(() => {});
  }
}

async function pickProgram() {
  try { return await invoke("pick_program"); } catch (e) { setStatus(String(e), true); return null; }
}

function option(value, label, selected) {
  const o = document.createElement("option");
  o.value = value; o.textContent = label; o.selected = selected;
  return o;
}

function choice(items, current, onPick, label) {
  const sel = document.createElement("select");
  sel.setAttribute("aria-label", label);
  sel.replaceChildren(...items.map(([v, l]) => option(v, l, v === current)));
  sel.onchange = () => onPick(sel.value);
  return sel;
}

function lightingPreview(l) {
  const span = document.createElement("span");
  span.className = "mini";
  const colours = l.effect === "static" ? l.zones.map((c, i) => (l.zone_on[i] ? hex(c) : "transparent"))
    : ["breathing", "shifting", "zoom"].includes(l.effect) ? [hex(l.colour)] : [];
  for (const c of colours) { const i = document.createElement("i"); i.style.background = c; span.appendChild(i); }
  const t = document.createElement("span");
  t.textContent = ` ${EFFECT_NAMES[l.effect] || l.effect}, ${l.brightness}%`;
  const wrap = document.createElement("span");
  wrap.append(span, t);
  return wrap;
}

function gameCard(g, i, caps) {
  const card = document.createElement("div");
  card.className = "game";
  card.dataset.enabled = String(g.enabled);
  const running = state && state.active_game === g.exe;
  card.dataset.running = String(!!running);

  const head = document.createElement("div");
  head.className = "game-head";
  head.innerHTML = `<span class="names"><h3></h3><span class="exe"></span></span><span class="live"></span><label class="sw"><input type="checkbox" role="switch"><span></span></label>`;
  const iconBtn = document.createElement("button");
  iconBtn.type = "button";
  iconBtn.className = "icon-btn";
  iconBtn.title = g.path ? `${g.path}\nClick to choose a different program file` : "Click to choose the game's program file";
  iconBtn.setAttribute("aria-label", `Choose the program file for ${g.exe}`);
  iconBtn.appendChild(programIcon(g.exe, g.path));
  iconBtn.onclick = async () => {
    const path = await pickProgram();
    if (!path) return;
    if (exeName(path) !== g.exe) { setStatus(`That's ${exeName(path)}, not ${g.exe}`, true); return; }
    change((s) => (s.games[i].path = path));
  };
  head.prepend(iconBtn);
  head.querySelector("h3").textContent = gameTitle(g.exe);
  head.querySelector(".exe").textContent = g.exe;
  head.querySelector(".live").textContent = running ? "Running, profile in use" : "";
  const on = head.querySelector("input");
  on.checked = g.enabled;
  on.setAttribute("aria-label", `Use the profile for ${g.exe}`);
  on.onchange = () => change((s) => (s.games[i].enabled = on.checked));
  const del = document.createElement("button");
  del.type = "button"; del.className = "ghost"; del.textContent = "Remove";
  del.onclick = () => change((s) => s.games.splice(i, 1));
  head.appendChild(del);
  card.appendChild(head);

  const rows = document.createElement("div");
  rows.className = "game-rows";
  const row = (label, icon, ...content) => {
    const l = document.createElement("span");
    l.append(rowIcon(icon), label);
    const d = document.createElement("div"); d.className = "detail"; d.append(...content);
    rows.append(l, d);
  };
  const edit = (fn) => change((s) => fn(s.games[i]));

  if (caps.lighting) {
    const parts = [choice([["keep", "Keep everyday lighting"], ["own", "Its own lighting"]], g.lighting ? "own" : "keep",
      (v) => edit((p) => (p.lighting = v === "own" ? clone(settings.lighting) : null)), "Lighting")];
    if (g.lighting) {
      parts.push(lightingPreview(g.lighting));
      const take = document.createElement("button");
      take.type = "button"; take.className = "ghost"; take.textContent = "Use what's on the Lighting page";
      take.onclick = () => { edit((p) => (p.lighting = clone(settings.lighting))); setStatus(`Lighting saved for ${gameTitle(g.exe)}`); };
      parts.push(take);
    }
    row("Lighting", "lighting", ...parts);
  }

  const fans = caps.fans || [];
  if (fans.length) {
    const mode = g.fans ? g.fans.mode : "keep";
    const parts = [choice([["keep", "Keep everyday fans"], ["auto", "Auto"], ["max", "Max"], ["custom", "Custom"]], mode, (v) => edit((p) => {
      if (v === "keep") p.fans = null;
      else p.fans = Object.assign(clone(p.fans || settings.fans), { mode: v });
    }), "Fans")];
    if (g.fans && g.fans.mode === "custom") {
      const box = document.createElement("div");
      box.className = "fan-sliders";
      parts.push(box);
      for (const id of fans) {
        const key = `${id}_percent`;
        const lab = document.createElement("label");
        lab.className = "fan-pct";
        const val = g.fans[key] == null ? 50 : g.fans[key];
        lab.innerHTML = `<span></span><input type="range" min="0" max="100" step="5"><b></b>`;
        lab.querySelector("span").textContent = (FAN_NAMES[id] || id).replace(" fan", "");
        const r = lab.querySelector("input");
        r.value = val;
        r.setAttribute("aria-label", `${FAN_NAMES[id] || id} speed while playing`);
        lab.querySelector("b").textContent = `${val}%`;
        r.oninput = () => { lab.querySelector("b").textContent = `${r.value}%`; settings.games[i].fans[key] = Number(r.value); save(); };
        box.appendChild(lab);
      }
    }
    row("Fans", "fans", ...parts);
  }

  const levels = caps.gpu_levels || [];
  if (levels.length) {
    row("GPU", "gpu", choice([["keep", "Keep everyday level"], ...levels.map((l) => [l.mode, GPU_LEVEL_NAMES[l.mode] || l.mode])],
      g.gpu || "keep", (v) => edit((p) => (p.gpu = v === "keep" ? null : v)), "GPU level"));
  }

  if ((caps.toggles || []).includes("win_key")) {
    const cur = g.win_key == null ? "keep" : g.win_key ? "on" : "off";
    row("Windows key", "winkey", choice([["keep", "Keep everyday setting"], ["off", "Blocked while playing"], ["on", "Works while playing"]], cur,
      (v) => edit((p) => (p.win_key = v === "keep" ? null : v === "on")), "Windows key"));
  }

  card.appendChild(rows);
  return card;
}

function renderGames() {
  const caps = (state && state.capabilities) || {};
  const games = settings.games || [];
  $("gameSyncRow").replaceChildren(switchRow("GameSync",
    "Turn off to keep your everyday settings even while a listed game runs.",
    settings.game_sync !== false, false, (on) => change((s) => (s.game_sync = on))));
  const list = games.map((g, i) => gameCard(g, i, caps));
  if (!list.length) {
    const p = document.createElement("p");
    p.className = "empty";
    p.textContent = "No games yet.";
    list.push(p);
  }
  $("gameList").replaceChildren(...list);
  renderGameBanner();
  renderRunningApps();
  findMissingPaths();
}

// Running programs as tiles with their icons; a click adds one.
function renderRunningApps() {
  const filter = exeName($("gameExe").value || "").replace(/\.exe$/, "");
  const added = new Set((settings.games || []).map((g) => g.exe));
  const apps = runningApps.filter((a) => !filter || a.name.includes(filter));
  $("appGrid").replaceChildren(...apps.map((a) => {
    const b = document.createElement("button");
    b.type = "button";
    b.className = "app-tile";
    b.disabled = added.has(a.name);
    b.title = a.path || a.name;
    const label = document.createElement("span");
    label.innerHTML = "<b></b><small></small>";
    label.querySelector("b").textContent = gameTitle(a.name);
    label.querySelector("small").textContent = added.has(a.name) ? "Added" : a.name;
    b.append(programIcon(a.name, a.path), label);
    b.onclick = () => addProgram(a.name, a.path);
    return b;
  }));
  $("appEmpty").hidden = apps.length > 0;
  $("appEmpty").textContent = runningApps.length ? "Nothing running matches that name." : "No programs with a window are running.";
}

function renderGameBanner() {
  const exe = state && state.active_game;
  const b = $("gameBanner");
  b.hidden = !exe || !settings || settings.game_sync === false;
  if (exe) b.textContent = `${gameTitle(exe)} is running, so its game profile is in use. What it changes comes back to your everyday settings when the game closes.`;
}

async function loadRunningApps() {
  try { runningApps = await invoke("running_apps"); } catch { runningApps = []; }
  renderRunningApps();
  // Games added by name pick up their program path (and so their icon)
  // the first time they're seen running.
  const found = (settings.games || []).filter((g) => !g.path && runningApps.some((a) => a.name === g.exe && a.path));
  if (found.length) change((s) => {
    for (const g of s.games) {
      const a = runningApps.find((x) => x.name === g.exe && x.path);
      if (!g.path && a) g.path = a.path;
    }
  });
}

function addProgram(exe, path) {
  if ((settings.games || []).some((g) => g.exe === exe)) { setStatus(`${gameTitle(exe)} is already in the list`, true); return; }
  change((s) => {
    s.games = s.games || [];
    s.games.push({ exe, path: path || null, enabled: true, lighting: null, fans: null, gpu: null, win_key: null });
  });
  $("gameExe").value = "";
  renderRunningApps();
  setStatus(`Added ${gameTitle(exe)}. Pick what should change while it runs.`);
}

function addGame() {
  const typed = $("gameExe").value.trim().replace(/^"(.*)"$/, "$1"); // Explorer's "Copy as path" adds quotes
  let exe = exeName(typed);
  if (!exe) return;
  if (IS_WINDOWS && !exe.includes(".")) exe += ".exe";
  const seen = runningApps.find((a) => a.name === exe && a.path);
  addProgram(exe, /[\\/]/.test(typed) ? typed : seen ? seen.path : null);
}

async function browseGame() {
  const path = await pickProgram();
  if (path) addProgram(exeName(path), path);
}

/* ---------- restoring the original settings ---------- */

function originalText() {
  const o = state && state.original;
  if (!o) return null;
  const when = new Date(o.taken * 1000).toLocaleDateString(undefined, { year: "numeric", month: "long", day: "numeric" });
  return o.source === "factory"
    ? `Puts this laptop back to Acer's factory settings: its default keyboard colour, fans on Auto, no GPU overclock, CoolBoost off, the Windows key on and the idle backlight timeout on. (Saved ${when}; OpenSenses had already changed things before it kept a copy, so the factory settings are used.)`
    : `Puts back the lighting, fans, GPU overclock and on/off settings this laptop had before OpenSenses first changed anything (saved ${when}).`;
}

function renderRestore() {
  const text = originalText();
  $("restoreCard").hidden = !text;
  if (text) $("restoreText").textContent = text;
  $("restoredBanner").hidden = !(settings && settings.restored);
}

async function restoreOriginal() {
  $("restoreDialog").hidden = true;
  try {
    state = await invoke("restore_original");
    settings = clone(state.settings);
    renderAll();
    if (state.errors && state.errors.length) showErrors(state);
    else setStatus("Original settings restored");
  } catch (e) {
    setStatus(String(e), true);
  }
}

/* ---------- rendering ---------- */

function renderLighting() {
  const l = settings.lighting;
  const caps = (state && state.capabilities) || {};
  const isStatic = l.effect === "static";

  const perKey = perKeyMode();
  if ((perKey ? perKeyLayout().name : "") !== layoutKey) drawKeyboard();
  const effects = effectList();
  segmented($("effects"), effects.map(([k, n]) => [k, n]), l.effect, (v) => change((s) => (s.lighting.effect = v)), !caps.lighting && !caps.per_key);
  $("effectHint").textContent = (effects.find((e) => e[0] === l.effect) || ["", "", ""])[2];
  if (perKey) return renderPerKeyLighting(l, caps);
  $("zonePills").className = "zone-pills";
  $("allZones").textContent = "Use on all zones";

  // zone pills
  $("zonePills").replaceChildren(...[0, 1, 2, 3].map((z) => {
    const b = document.createElement("div");
    b.className = "zone-pill";
    b.tabIndex = 0;
    b.setAttribute("role", "button");
    b.setAttribute("aria-pressed", String(isStatic && z === selectedZone));
    const name = ["Left", "Middle", "Right", "Number pad"][z];
    b.innerHTML = `<span class="dot"></span><span class="name"></span><span class="hex"></span>`;
    b.querySelector(".dot").style.background = l.zone_on[z] ? hex(l.zones[z]) : "transparent";
    b.querySelector(".dot").style.border = "1px solid var(--line)";
    b.querySelector(".name").textContent = name;
    b.querySelector(".hex").textContent = l.zone_on[z] ? hex(l.zones[z]) : "Off";
    const pick = () => { selectedZone = z; if (!isStatic) change((s) => (s.lighting.effect = "static")); else renderAll(); };
    b.onclick = pick;
    b.onkeydown = (e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); pick(); } };
    const tog = document.createElement("input");
    tog.type = "checkbox";
    tog.checked = l.zone_on[z];
    tog.title = "Zone on";
    tog.setAttribute("aria-label", `${name} on`);
    tog.onclick = (e) => e.stopPropagation();
    tog.onchange = () => change((s) => (s.lighting.zone_on[z] = tog.checked));
    b.appendChild(tog);
    b.hidden = !isStatic;
    return b;
  }));
  $("zonePills").hidden = !isStatic;

  // colour
  const usesColour = isStatic || ["breathing", "shifting", "zoom"].includes(l.effect);
  $("colourField").hidden = !usesColour;
  const current = isStatic ? l.zones[selectedZone] : l.colour;
  $("colourTitle").textContent = isStatic ? `Zone ${selectedZone + 1} colour` : "Colour";
  const sw = BASIC.map((h) => {
    const b = document.createElement("button");
    b.type = "button"; b.className = "swatch"; b.style.background = h;
    b.setAttribute("role", "radio"); b.setAttribute("aria-label", h);
    b.setAttribute("aria-checked", String(hex(current) === h));
    b.onclick = () => { setTargetColour(rgb(h)); renderAll(); };
    return b;
  });
  $("swatches").replaceChildren(...sw);
  $("allZones").hidden = !isStatic;
  if (!picker.dragging) picker.sync(current);

  // speed / brightness / direction
  $("speedField").hidden = isStatic || l.effect === "temperature";
  $("speed").value = l.speed;
  $("speed").oninput = () => change((s) => (s.lighting.speed = Number($("speed").value)));
  segmented($("brightness"), BRIGHTNESS, l.brightness, (v) => change((s) => (s.lighting.brightness = v)));
  $("directionField").hidden = !["wave", "shifting"].includes(l.effect);
  segmented($("direction"), DIRECTIONS, l.direction, (v) => change((s) => (s.lighting.direction = v)));
  $("extraLights").hidden = true;

  paint(performance.now() / 1000);
}

function renderPerKeyLighting(l, caps) {
  const isStatic = l.effect === "static";
  // Selection tools instead of zone pills.
  const bar = $("zonePills");
  bar.className = "key-tools";
  bar.hidden = !isStatic;
  const btn = (label, fn) => { const b = document.createElement("button"); b.type = "button"; b.className = "ghost"; b.textContent = label; b.onclick = fn; return b; };
  const allSlots = keys.map((k) => k.slot);
  const info = document.createElement("span");
  info.className = "hint";
  info.textContent = selectedKeys.size ? `${selectedKeys.size} key${selectedKeys.size > 1 ? "s" : ""} selected` : "Click a key to pick it; Ctrl or Shift + click picks more.";
  bar.replaceChildren(
    btn("Select all", () => { selectedKeys = new Set(allSlots); renderAll(); }),
    btn("Select none", () => { selectedKeys.clear(); renderAll(); }),
    info);
  for (const k of keys) k.el.classList.toggle("picked", isStatic && selectedKeys.has(k.slot));

  const usesColour = isStatic || PER_KEY_COLOURED.includes(l.effect);
  $("colourField").hidden = !usesColour;
  const current = currentColour();
  $("colourTitle").textContent = isStatic ? (selectedKeys.size ? "Colour of the selected keys" : "Key colour") : "Colour";
  $("swatches").replaceChildren(...BASIC.map((h) => {
    const b = document.createElement("button");
    b.type = "button"; b.className = "swatch"; b.style.background = h;
    b.setAttribute("role", "radio"); b.setAttribute("aria-label", h);
    b.setAttribute("aria-checked", String(hex(current) === h));
    b.onclick = () => { setTargetColour(rgb(h)); renderAll(); };
    return b;
  }));
  $("allZones").hidden = !isStatic;
  $("allZones").textContent = "Use on all keys";
  if (!picker.dragging) picker.sync(current);

  $("speedField").hidden = isStatic;
  $("speed").value = l.speed;
  $("speed").oninput = () => change((s) => (s.lighting.speed = Number($("speed").value)));
  segmented($("brightness"), BRIGHTNESS, l.brightness, (v) => change((s) => (s.lighting.brightness = v)));
  const dirs = l.effect === "wave" ? WAVE_DIRECTIONS : l.effect === "spiral" ? SPIRAL_DIRECTIONS : null;
  $("directionField").hidden = !dirs;
  if (dirs) segmented($("direction"), dirs, dirs.some(([v]) => v === l.direction) ? l.direction : dirs[0][0], (v) => change((s) => (s.lighting.direction = v)));
  renderExtraLights(l, caps);
  paint(performance.now() / 1000);
}

// Heat pipe (Helios 700), fan window (Triton 900) and touchpad lights.
function renderExtraLights(l, caps) {
  const e = l.extras || {};
  const rows = [];
  const colourInput = (value, label, onPick) => {
    const i = document.createElement("input");
    i.type = "color"; i.value = hex(value || l.colour).toLowerCase();
    i.setAttribute("aria-label", label);
    i.onchange = () => onPick(rgb(i.value.toUpperCase()));
    return i;
  };
  const light = (title, desc, on, setOn, colour, setColour) => {
    const row = switchRow(title, desc, on, false, (v) => change((s) => { s.lighting.extras = s.lighting.extras || {}; setOn(s.lighting.extras, v); }));
    if (setColour) row.querySelector("p").append(" ", colourInput(colour, `${title} colour`, (c) => change((s) => setColour(s.lighting, c))));
    rows.push(row);
  };
  if (caps.heat_pipe_light) light("Heat pipe light", "The light strip over the heat pipes.", e.heat_pipe !== false,
    (x, v) => (x.heat_pipe = v), e.heat_pipe_colour, (s, c) => { s.extras = s.extras || {}; s.extras.heat_pipe_colour = c; });
  if (caps.fan_light) light("Fan light", "The light behind the fan window.", e.fan_led !== false,
    (x, v) => (x.fan_led = v), e.fan_led_colour, (s, c) => { s.extras = s.extras || {}; s.extras.fan_led_colour = c; });
  if (caps.touchpad_light) light("Touchpad light", "The light around the touchpad.", e.touchpad !== false,
    (x, v) => (x.touchpad = v), (l.keys || [])[TOUCHPAD_SLOT], caps.touchpad_light === 2 ? (s, c) => { s.keys = fullKeys(s); s.keys[TOUCHPAD_SLOT] = c; } : null);
  $("extraLights").replaceChildren(...rows);
  $("extraLights").hidden = rows.length === 0;
}

function renderFans() {
  const f = settings.fans;
  const caps = (state && state.capabilities) || {};
  const fans = caps.fans || [];
  segmented($("fanModes"), FAN_MODES, f.mode, (v) => change((s) => (s.fans.mode = v)), fans.length === 0);
  const cards = fans.map((id) => [id, FAN_NAMES[id] || id]).map(([id, title]) => {
    const pct = f[`${id}_percent`];
    const c = document.createElement("div");
    c.className = "fan";
    c.dataset.fan = id;
    c.dataset.off = String(f.mode !== "custom");
    c.innerHTML = `<h3>${title}</h3><div class="rpm" data-live="${id}_fan_rpm">–</div><div class="temp" data-live="${id}_temp_line"></div>
      <div class="custom"><div class="top"><span>Custom speed</span><span class="pct">${pct == null ? "Auto" : pct + "%"}</span></div>
      <input type="range" min="0" max="100" step="5" aria-label="${title} speed">
      <label class="switch"><input type="checkbox"> Let the laptop decide this fan's speed</label></div>`;
    const range = c.querySelector('input[type="range"]');
    const auto = c.querySelector('input[type="checkbox"]');
    range.value = pct == null ? 50 : pct;
    range.disabled = f.mode !== "custom" || pct == null;
    auto.checked = pct == null;
    auto.disabled = f.mode !== "custom";
    range.oninput = () => { c.querySelector(".pct").textContent = range.value + "%"; settings.fans[`${id}_percent`] = Number(range.value); save(); };
    auto.onchange = () => change((s) => (s.fans[`${id}_percent`] = auto.checked ? null : Number(range.value)));
    return c;
  });
  $("fanCards").replaceChildren(...cards);

  // CoolBoost sits with the fans, as in PredatorSense, and only acts in Auto.
  const hasBoost = (caps.toggles || []).includes("cool_boost");
  $("coolBoostRow").hidden = !hasBoost;
  if (hasBoost) {
    const chosen = settings.toggles ? settings.toggles.cool_boost : null;
    const now = state && state.toggles_now ? state.toggles_now.cool_boost : null;
    const auto = f.mode === "auto";
    $("coolBoostRow").replaceChildren(switchRow(expTitle("CoolBoost", "cool_boost"),
      auto ? "Runs the fans faster in Auto mode for extra cooling, at the cost of more noise."
           : "Works in Auto mode only. Switch the fans to Auto to use it.",
      chosen != null ? chosen : now, !auto,
      (on) => change((s) => { s.toggles = s.toggles || {}; s.toggles.cool_boost = on; })));
  }
}

function renderGpu() {
  const caps = (state && state.capabilities) || {};
  const levels = caps.gpu_levels || [];
  $("gpuLevels").style.gridTemplateColumns = `repeat(${Math.max(levels.length, 1)}, 1fr)`;
  $("gpuLevels").replaceChildren(...levels.map((l) => {
    const b = document.createElement("button");
    b.type = "button"; b.className = "level";
    b.setAttribute("role", "radio");
    b.setAttribute("aria-checked", String(settings.gpu === l.mode));
    b.innerHTML = `<h3>${GPU_LEVEL_NAMES[l.mode] || l.mode}</h3><dl><dt>Core clock</dt><dd>${mhz(l.core_mhz)}</dd><dt>Memory clock</dt><dd>${mhz(l.memory_mhz)}</dd></dl><p class="hint">${GPU_LEVEL_HINTS[l.mode] || ""}</p>`;
    b.onclick = () => change((s) => (s.gpu = l.mode));
    return b;
  }));
  const gpuName = caps.model_name ? `for this ${caps.model_name}` : "for this laptop";
  $("gpuIntro").textContent = `The overclock levels PredatorSense offered ${gpuName}'s GPU. NVIDIA's driver forgets overclocks on restart, so with “Keep settings applied” on, OpenSenses sets your level again each time.`;
}

const GAUGES = [
  ["cpu_temp", "CPU temperature", "temp", 30, 100],
  ["gpu_temp", "GPU temperature", "temp", 30, 100],
  ["sys_temp", "System temperature", "temp", 20, 80],
  ["cpu_fan_rpm", "CPU fan", "RPM", 0, 6000],
  ["gpu_fan_rpm", "GPU fan", "RPM", 0, 6000],
];

function buildGauges() {
  $("gauges").replaceChildren(...GAUGES.map(([id, title]) => {
    const d = document.createElement("div");
    d.className = "gauge";
    d.innerHTML = `<h3>${title}</h3><div class="val" data-gauge="${id}">–</div><canvas width="400" height="96" data-spark="${id}"></canvas>`;
    return d;
  }));
}

function spark(canvas, data, lo, hi) {
  const ctx = canvas.getContext("2d");
  const w = canvas.width, h = canvas.height;
  ctx.clearRect(0, 0, w, h);
  if (data.length < 2) return;
  ctx.strokeStyle = "#9fb4ff";
  ctx.lineWidth = 3;
  ctx.beginPath();
  data.forEach((v, i) => {
    const x = w - (data.length - 1 - i) * (w / 89);
    const y = h - 6 - ((Math.min(hi, Math.max(lo, v)) - lo) / (hi - lo)) * (h - 12);
    i ? ctx.lineTo(x, y) : ctx.moveTo(x, y);
  });
  ctx.stroke();
}

function renderLive() {
  if (!state) return;
  const s = state.sensors || {};
  for (const id of Object.keys(history)) {
    if (s[id] != null) { history[id].push(s[id]); if (history[id].length > 90) history[id].shift(); }
  }
  const fmt = (v, unit) => (v == null ? "–" : `${v.toLocaleString()}<small>${unit}</small>`);
  $("railTemps").innerHTML = `CPU <b>${toUnit(s.cpu_temp) ?? "–"}°</b><br>GPU <b>${toUnit(s.gpu_temp) ?? "–"}°</b>`;
  for (const [id, , unit, lo, hi] of GAUGES) {
    const v = document.querySelector(`[data-gauge="${id}"]`);
    if (v) v.innerHTML = unit === "temp" ? fmt(toUnit(s[id]), unitLabel()) : fmt(s[id], unit);
    const c = document.querySelector(`[data-spark="${id}"]`);
    if (c) spark(c, history[id], lo, hi);
  }
  document.querySelectorAll('[data-live="cpu_fan_rpm"]').forEach((e) => (e.innerHTML = fmt(s.cpu_fan_rpm, "RPM")));
  document.querySelectorAll('[data-live="gpu_fan_rpm"]').forEach((e) => (e.innerHTML = fmt(s.gpu_fan_rpm, "RPM")));
  document.querySelectorAll('[data-live="cpu_temp_line"]').forEach((e) => (e.textContent = s.cpu_temp != null ? `CPU at ${toUnit(s.cpu_temp)} ${unitLabel()}` : ""));
  document.querySelectorAll('[data-live="gpu_temp_line"]').forEach((e) => (e.textContent = s.gpu_temp != null ? `GPU at ${toUnit(s.gpu_temp)} ${unitLabel()}` : ""));
  $("guardNote").hidden = !state.overheat_guard;
  if (settings && settings.lighting.effect === "temperature") paint(0);
}

function renderSections() {
  const caps = (state && state.capabilities) || {};
  $("noticeBanner").hidden = !caps.notice;
  $("noticeBanner").textContent = caps.notice || "";
  const show = {
    lighting: caps.lighting || caps.per_key,
    fans: (caps.fans || []).length > 0,
    gpu: (caps.gpu_levels || []).length > 0,
    monitor: caps.sensors !== false,
  };
  for (const [page, visible] of Object.entries(show)) {
    document.querySelector(`.tab[data-page="${page}"]`).hidden = !visible;
  }
  $("perKeyNote").hidden = true;
  $("lightingExp").hidden = !((caps.lighting || caps.per_key) && isExp("lighting"));
  const TITLES = { fans: "Fans", gpu: "GPU overclock", monitor: "Monitor", games: "Games", keys: "Keys" };
  document.querySelectorAll("h1[data-title]").forEach((h) => (h.textContent = expTitle(TITLES[h.dataset.title], h.dataset.title)));
  document.querySelectorAll("#page-lighting .stage, #page-lighting .controls").forEach((el) => (el.hidden = !caps.lighting && !caps.per_key));
  const current = document.querySelector(".tab[aria-current=page]");
  if (current && current.hidden) {
    const first = [...document.querySelectorAll(".tab")].find((t) => !t.hidden);
    if (first) first.click();
  }
}

function renderAll() {
  if (!settings) return;
  renderSections();
  renderLighting();
  renderFans();
  renderGpu();
  renderTurbo();
  renderSettings();
  renderTurboKeySetting();
  renderKeys();
  renderGames();
  $("keepApplied").checked = settings.keep_applied;
  renderLive();
}


/* ---------- Turbo and CPU level (GPU page) ---------- */

const TURBO_ACTIONS = [
  ["performance", "GPU Turbo and fans Max"],
  ["overclock", "GPU Turbo only"],
  ["fans", "Fans Max only"],
  ["off", "Nothing"],
];
const TURBO_HINTS = {
  performance: "As in PredatorSense: the highest GPU level and both fans on Max. Only on the charger. Press it again to go back.",
  overclock: "The highest GPU level, with the fans left as they are. Only on the charger. Press it again to go back.",
  fans: "Only runs the fans on Max. Press it again to go back.",
  off: "The Turbo key does nothing.",
};
const levelName = (m) => GPU_LEVEL_NAMES[m] || m;

function powerText() {
  const p = state && state.power;
  if (!p) return "";
  return p[0] ? `on the charger, battery ${p[1]}%` : `on battery, ${p[1]}%`;
}

function renderTurbo() {
  const caps = (state && state.capabilities) || {};
  const levels = caps.gpu_levels || [];

  // CPU power level that follows the GPU level (PredatorSense's behaviour).
  $("cpuFollows").hidden = !caps.cpu_follows_gpu && !caps.fan_table;
  if (!caps.cpu_follows_gpu && caps.fan_table) {
    $("cpuFollows").textContent = "The fans' built-in curve follows the GPU level, as in PredatorSense: a higher level lets them spin up sooner.";
  } else if (caps.cpu_follows_gpu) {
    const now = state.cpu_level != null && levels[state.cpu_level] ? ` Right now: ${levelName(levels[state.cpu_level].mode)} (${powerText()}).` : "";
    $("cpuFollows").textContent = `${isExp("gpu") ? "(experimental) " : ""}The CPU's power level follows the GPU level, as in PredatorSense, and the fans' curve follows both. It's only raised on the charger with the battery at 40% or more; otherwise it stays at Normal.${now}`;
  }

  // Separate CPU overclock levels (unlocked HK processors).
  const cpu = caps.cpu_levels || [];
  $("cpuField").hidden = cpu.length === 0;
  if (cpu.length) {
    $("cpuTitle").textContent = expTitle("CPU overclock", "gpu");
    segmented($("cpuLevels"), cpu.map((m) => [m, levelName(m)]), settings.cpu || "normal", (v) => change((s) => (s.cpu = v)));
    const now = state.cpu_level != null && cpu[state.cpu_level] ? ` Right now: ${levelName(cpu[state.cpu_level])} (${powerText()}).` : "";
    $("cpuHint").textContent = `PredatorSense's CPU overclock levels for this laptop's unlocked processor. They only apply on the charger with the battery at 40% or more; otherwise the CPU runs at Normal.${now}`;
  }

  // Turbo (the key, or this button).
  $("turboCard").hidden = !caps.turbo_key;
  if (caps.turbo_key) {
    const on = !!state.turbo;
    const action = settings.turbo_key || "performance";
    $("turboCard").dataset.on = String(on);
    $("turboTitle").textContent = expTitle(on ? "Turbo is on" : "Turbo", "gpu");
    $("turboText").textContent = action === "off"
      ? "The Turbo key is set to do nothing (Settings → Turbo key)."
      : `${TURBO_HINTS[action]} The laptop's Turbo key does the same.`;
    $("turboBtn").textContent = on ? "Turn off" : "Turn on";
    $("turboBtn").disabled = action === "off";
  }
}

async function setTurbo(on) {
  try {
    state = await invoke("set_turbo", { on });
    renderTurbo();
  } catch (e) { setStatus(String(e), true); }
}

function renderTurboKeySetting() {
  const caps = (state && state.capabilities) || {};
  $("turboKeyField").hidden = !caps.turbo_key;
  if (!caps.turbo_key) return;
  const action = settings.turbo_key || "performance";
  segmented($("turboKey"), TURBO_ACTIONS, action, (v) => change((s) => { s.turbo_key = v; }));
  $("turboKeyHint").textContent = TURBO_HINTS[action];
}

/* ---------- programmable keys ---------- */

let keyGroup = 0;
const KEY_ACTIONS = [
  ["none", "Nothing"],
  ["performance", "System performance Normal ⇄ Extreme"],
  ["gpu_levels", "Switch GPU level"],
  ["cpu_levels", "Switch CPU level"],
  ["fan_modes", "Switch fan mode"],
  ["turbo", "Turbo on/off"],
  ["win_key", "Windows and Menu keys on/off"],
  ["lcd_overdrive", "LCD overdrive on/off"],
  ["sticky_keys", "Sticky Keys on/off"],
  ["launch_app", "Open a program"],
  ["macro", "Macro (type keys)"],
  ["open_app", "Open OpenSenses"],
];

// The switching orders PredatorSense offered, built from this laptop's levels.
function levelCycles(levels) {
  const names = levels.map(levelName);
  const out = [];
  if (levels.length >= 2) out.push([levels, names.join(" → ")]);
  const pair = (a, b) => [[levels[a], levels[b]], `${names[a]} ⇄ ${names[b]}`];
  if (levels.length >= 3) out.push(pair(0, levels.length - 1), pair(0, 1), pair(1, levels.length - 1));
  else if (levels.length === 2) out.push(pair(0, 1));
  return out;
}
const FAN_CYCLES = [
  [["auto", "max", "custom"], "Auto → Max → Custom"],
  [["auto", "max"], "Auto ⇄ Max"],
  [["max", "custom"], "Max ⇄ Custom"],
  [["auto", "custom"], "Auto ⇄ Custom"],
];

function hotkeyGroups(caps) {
  const n = caps.hotkey_keys || 0;
  const groups = clone(settings.hotkeys || []);
  while (groups.length < 3) groups.push({ name: "", keys: [] });
  for (const g of groups) while (g.keys.length < n) g.keys.push({ kind: "none" });
  return groups;
}

function editKey(k, fn) {
  const caps = state.capabilities || {};
  change((s) => {
    s.hotkeys = hotkeyGroups(caps);
    fn(s.hotkeys[keyGroup], k);
  });
}

const prettyKey = (code) => code.replace(/^Key/, "").replace(/^Digit/, "").replace(/^Numpad/, "Num ").replace(/(Left|Right)$/, "").replace(/^Arrow/, "") || code;
function macroText(steps) {
  const parts = [];
  for (const st of steps || []) {
    if (!st.code) parts.push(`${st.delay_ms} ms`);
    else if (st.down) parts.push(prettyKey(st.code));
  }
  return parts.join(" · ");
}

function keyCard(action, k, caps) {
  const card = document.createElement("div");
  card.className = "game";
  const head = document.createElement("div");
  head.className = "game-head";
  const num = document.createElement("span");
  num.className = "key-num";
  num.textContent = `P${k + 1}`;
  const names = document.createElement("span");
  names.className = "names";
  names.innerHTML = "<h3></h3>";
  names.querySelector("h3").textContent = `Key ${k + 1}`;
  const test = document.createElement("button");
  test.type = "button"; test.className = "ghost"; test.textContent = "Try it";
  test.title = "Does what the key would do now, without pressing it";
  test.style.marginLeft = "auto";
  test.disabled = action.kind === "none";
  test.onclick = async () => {
    try {
      state = await invoke("test_key", { detail: [7, 2, k + 1, keyGroup + 1, 0, 0, 0, 0] });
      showNotice(true);
      renderTurbo();
    } catch (e) { setStatus(String(e), true); }
  };
  head.append(num, names, test);
  card.appendChild(head);

  const rows = document.createElement("div");
  rows.className = "game-rows";
  const row = (label, ...content) => {
    const l = document.createElement("span");
    l.textContent = label;
    const d = document.createElement("div"); d.className = "detail"; d.append(...content);
    rows.append(l, d);
  };
  const toggles = caps.toggles || [];
  const available = KEY_ACTIONS.filter(([kind]) => {
    if (kind === "gpu_levels" || kind === "performance") return (caps.gpu_levels || []).length > 1;
    if (kind === "cpu_levels") return (caps.cpu_levels || []).length > 1;
    if (kind === "fan_modes") return (caps.fans || []).length > 0;
    if (kind === "turbo") return caps.turbo_key;
    if (kind === "win_key" || kind === "lcd_overdrive") return toggles.includes(kind);
    if (kind === "sticky_keys") return caps.sticky_keys;
    return true;
  });
  row("Does", choice(available, action.kind, (v) => editKey(k, (g) => {
    const gpu = (caps.gpu_levels || []).map((l) => l.mode);
    if (v === "gpu_levels") g.keys[k] = { kind: v, cycle: levelCycles(gpu)[0][0] };
    else if (v === "cpu_levels") g.keys[k] = { kind: v, cycle: levelCycles(caps.cpu_levels || [])[0][0] };
    else if (v === "fan_modes") g.keys[k] = { kind: v, cycle: FAN_CYCLES[1][0] };
    else if (v === "launch_app") g.keys[k] = { kind: v, target: "" };
    else if (v === "macro") g.keys[k] = { kind: v, steps: [] };
    else g.keys[k] = { kind: v };
  }), `What key ${k + 1} does`));

  if (["gpu_levels", "cpu_levels", "fan_modes"].includes(action.kind)) {
    const cycles = action.kind === "fan_modes" ? FAN_CYCLES
      : levelCycles(action.kind === "gpu_levels" ? (caps.gpu_levels || []).map((l) => l.mode) : caps.cpu_levels || []);
    const current = JSON.stringify(action.cycle || []);
    const items = cycles.map(([c, label], i) => [String(i), label]);
    const sel = cycles.findIndex(([c]) => JSON.stringify(c) === current);
    row("Order", choice(items, String(Math.max(sel, 0)), (v) => editKey(k, (g) => (g.keys[k].cycle = cycles[Number(v)][0])), "Switching order"));
  }
  if (action.kind === "launch_app") {
    const input = document.createElement("input");
    input.className = "key-target";
    input.value = action.target || "";
    input.placeholder = IS_WINDOWS ? "A program, file or web address" : "A program, .desktop file, file or web address";
    input.spellcheck = false;
    input.setAttribute("aria-label", `Program for key ${k + 1}`);
    input.onchange = () => editKey(k, (g) => (g.keys[k].target = input.value.trim()));
    const browse = document.createElement("button");
    browse.type = "button"; browse.className = "ghost"; browse.textContent = "Browse…";
    browse.onclick = async () => {
      const path = await pickProgram();
      if (path) editKey(k, (g) => (g.keys[k].target = path));
    };
    row("Opens", input, browse);
  }
  if (action.kind === "macro") {
    const text = document.createElement("span");
    text.textContent = (action.steps || []).length ? macroText(action.steps) : "Nothing recorded yet";
    const rec = document.createElement("button");
    rec.type = "button"; rec.className = "ghost"; rec.textContent = (action.steps || []).length ? "Record again" : "Record";
    rec.onclick = () => recordMacro(k);
    row("Types", text, rec);
  }
  if (action.kind === "performance") row("", Object.assign(document.createElement("span"), { textContent: "Highest GPU (and CPU) level, or back to Normal. Extreme needs the charger." }));
  card.appendChild(rows);
  return card;
}

function renderKeys() {
  const caps = (state && state.capabilities) || {};
  const n = caps.hotkey_keys || 0;
  document.querySelector('.tab[data-page="keys"]').hidden = n === 0;
  if (!n) return;
  const groups = hotkeyGroups(caps);
  segmented($("keyGroups"), [0, 1, 2].map((i) => [i, groups[i].name || `Group ${i + 1}`]), keyGroup, (v) => { keyGroup = v; renderKeys(); });
  const g = groups[keyGroup];
  $("groupName").value = g.name || "";
  $("groupName").placeholder = `Group ${keyGroup + 1}`;
  $("keyList").replaceChildren(...g.keys.slice(0, n).map((a, k) => keyCard(a, k, caps)));
  const active = state.hotkey_group ? ` The laptop is on group ${state.hotkey_group} now.` : "";
  $("keysIntro").textContent = `The ${n} programmable keys above the keyboard. Each one can do something different in each of the three groups; the laptop's group key switches between them.${active}`;
}

// Macro recorder: records key presses in the dialog, as KeyboardEvent.code names.
const macro = { key: -1, steps: [], last: 0, held: new Set() };
function recordMacro(k) {
  macro.key = k; macro.steps = []; macro.last = 0; macro.held.clear();
  $("mdKeys").textContent = "";
  $("macroDialog").hidden = false;
  $("mdDone").focus();
}
function macroEvent(e) {
  if ($("macroDialog").hidden) return;
  if (e.repeat || !e.code || e.code === "Unidentified") { e.preventDefault(); return; }
  // Let the dialog's buttons work with the mouse only; every key is recorded.
  e.preventDefault();
  e.stopPropagation();
  const down = e.type === "keydown";
  if (!down && !macro.held.has(e.code)) return;
  if (down) macro.held.add(e.code); else macro.held.delete(e.code);
  const now = performance.now();
  if ($("mdPauses").checked && macro.last && now - macro.last >= 30) {
    macro.steps.push({ code: null, down: false, delay_ms: Math.min(10000, Math.round((now - macro.last) / 10) * 10) });
  }
  macro.last = now;
  macro.steps.push({ code: e.code, down, delay_ms: 0 });
  if (macro.steps.length >= 50) {
    macro.steps.length = 50;
    $("mdText").textContent = "That's the 50-step limit. Click Done to keep it.";
  }
  $("mdKeys").textContent = macroText(macro.steps);
}
document.addEventListener("keydown", macroEvent, true);
document.addEventListener("keyup", macroEvent, true);
$("mdCancel").onclick = () => ($("macroDialog").hidden = true);
$("mdClear").onclick = () => { macro.steps = []; macro.last = 0; macro.held.clear(); $("mdKeys").textContent = ""; };
$("mdDone").onclick = () => {
  $("macroDialog").hidden = true;
  // Release anything still held when the recording stopped.
  for (const code of macro.held) macro.steps.push({ code, down: false, delay_ms: 0 });
  const steps = macro.steps.slice(0, 50);
  editKey(macro.key, (g) => (g.keys[macro.key] = { kind: "macro", steps }));
};
$("groupName").onchange = () => editKey(0, (g) => (g.name = $("groupName").value.trim()));
$("turboBtn").onclick = () => setTurbo(!(state && state.turbo));

// Messages from the service (Turbo key, key actions) shown in the status bar.
let lastNotice = null;
function showNotice(force) {
  const n = state && state.notice;
  if (!n) return;
  if (lastNotice !== null && (force || n[0] !== lastNotice)) setStatus(n[1]);
  lastNotice = n[0];
}

/* ---------- wiring ---------- */

document.querySelectorAll(".tab").forEach((t) => {
  t.onclick = () => {
    document.querySelectorAll(".tab").forEach((x) => x.removeAttribute("aria-current"));
    t.setAttribute("aria-current", "page");
    document.querySelectorAll(".page").forEach((p) => (p.hidden = p.id !== `page-${t.dataset.page}`));
  };
});
$("keepApplied").onchange = () => change((s) => (s.keep_applied = $("keepApplied").checked));
$("reapply").onclick = async () => {
  try {
    state = await invoke("reapply");
    if (state.errors.length) showErrors(state); else setStatus("Applied again");
  } catch (e) { setStatus(String(e), true); }
};
$("retry").onclick = () => refresh(true);
$("restoreBtn").onclick = () => { $("rdText").textContent = originalText() || ""; $("restoreDialog").hidden = false; $("rdNo").focus(); };
$("rdNo").onclick = () => ($("restoreDialog").hidden = true);
$("rdYes").onclick = restoreOriginal;
$("addGame").onclick = addGame;
$("gameExe").addEventListener("keydown", (e) => { if (e.key === "Enter") addGame(); });
$("gameExe").addEventListener("input", renderRunningApps);
$("browseGame").onclick = browseGame;
$("refreshApps").onclick = loadRunningApps;
document.querySelector('.tab[data-page="games"]').addEventListener("click", loadRunningApps);
$("installService").onclick = async () => {
  $("nsDetail").textContent = "Waiting for you to approve the administrator prompt…";
  try { await invoke("install_service"); } catch (e) { $("nsDetail").textContent = String(e); return; }
  setTimeout(() => refresh(true), 1500);
};

if (!IS_WINDOWS) {
  $("keepLabel").textContent = "Keep settings applied after restarts, sleep and system updates";
  $("gameExe").placeholder = "Type to search running programs, or a program name";
  $("gamesHint").textContent = "Start the game and it shows up here, or use Browse… to pick its program file. Windows games run through Proton or Wine show up as their .exe name.";
}

drawKeyboard();
buildGauges();
picker.init();
refresh(true);
setInterval(() => refresh(false), 2000);
requestAnimationFrame(loop);
