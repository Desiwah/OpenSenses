"use strict";

/* ---------- connection to the background service ---------- */

const tauri = window.__TAURI__;
const invoke = tauri ? tauri.core.invoke : mockInvoke;

// Lets the page run in a normal browser for design work.
let mock = null;
function mockInvoke(cmd, args) {
  if (!mock) {
    const teal = { r: 0, g: 174, b: 199 };
    mock = {
      settings: {
        lighting: { effect: "static", zones: [teal, { r: 70, g: 120, b: 255 }, { r: 190, g: 70, b: 255 }, { r: 255, g: 70, b: 120 }],
          zone_on: [true, true, true, true], colour: teal, speed: 4, brightness: 100, direction: "right_to_left" },
        fans: { mode: "auto", cpu_percent: 50, gpu_percent: 50 },
        gpu: "normal", keep_applied: true,
      },
      sensors: {}, capabilities: { lighting: true, fans: true, gpu_modes: true, sensors: true },
      backend: "preview", errors: [], overheat_guard: false,
    };
  }
  if (location.search.includes("noservice")) return Promise.reject("background service is not running");
  const t = Date.now() / 1000;
  mock.sensors = {
    cpu_temp: Math.round(66 + 6 * Math.sin(t / 7)), gpu_temp: Math.round(58 + 4 * Math.sin(t / 9)),
    sys_temp: 49, cpu_fan_rpm: Math.round(3000 + 300 * Math.sin(t / 5)), gpu_fan_rpm: Math.round(3450 + 250 * Math.sin(t / 6)),
  };
  if (cmd === "set_settings") mock.settings = JSON.parse(JSON.stringify(args.settings));
  return Promise.resolve(JSON.parse(JSON.stringify(mock)));
}

/* ---------- state ---------- */

let state = null;        // last state from the service
let settings = null;     // what the user is editing
let selectedZone = 0;
let saveTimer = null;
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
const BRIGHTNESS = [[0, "Off"], [25, "25%"], [50, "50%"], [75, "75%"], [100, "100%"]];
const DIRECTIONS = [["right_to_left", "Right to left"], ["left_to_right", "Left to right"]];
const FAN_MODES = [["auto", "Auto"], ["max", "Max"], ["custom", "Custom"]];
const GPU_LEVELS = [
  ["normal", "Normal", "Stock clocks", "+0 MHz", "+0 MHz"],
  ["faster", "Faster", "A mild overclock", "+100 MHz", "+160 MHz"],
  ["turbo", "Turbo", "The highest preset", "+160 MHz", "+320 MHz"],
];

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
    if (first) showErrors(st);
  } catch (e) {
    $("noService").hidden = false;
    $("nsDetail").textContent = String(e);
  }
}

function save() {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(async () => {
    try {
      state = await invoke("set_settings", { settings });
      if (state.errors && state.errors.length) showErrors(state);
      else setStatus("Applied");
    } catch (e) {
      setStatus(String(e), true);
    }
  }, 140);
}

function change(fn) {
  fn(settings);
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

function drawKeyboard() {
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

/* ---------- rendering ---------- */

function renderLighting() {
  const l = settings.lighting;
  const caps = (state && state.capabilities) || {};
  const isStatic = l.effect === "static";

  segmented($("effects"), EFFECTS, l.effect, (v) => change((s) => (s.lighting.effect = v)), caps.lighting === false);
  $("effectHint").textContent = EFFECTS.find((e) => e[0] === l.effect)[2];

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
  const setColour = (h) => change((s) => {
    if (s.lighting.effect === "static") { s.lighting.zones[selectedZone] = rgb(h); s.lighting.zone_on[selectedZone] = true; }
    else s.lighting.colour = rgb(h);
  });
  const sw = BASIC.map((h) => {
    const b = document.createElement("button");
    b.type = "button"; b.className = "swatch"; b.style.background = h;
    b.setAttribute("role", "radio"); b.setAttribute("aria-label", h);
    b.setAttribute("aria-checked", String(hex(current) === h));
    b.onclick = () => setColour(h);
    return b;
  });
  const custom = document.createElement("label");
  custom.className = "custom-colour";
  custom.innerHTML = '<input type="color"> Any colour';
  const ci = custom.querySelector("input");
  ci.value = hex(current).toLowerCase();
  ci.oninput = () => setColour(ci.value.toUpperCase());
  const allOn = document.createElement("button");
  allOn.type = "button"; allOn.className = "ghost"; allOn.textContent = "Use on all zones";
  allOn.hidden = !isStatic;
  allOn.onclick = () => change((s) => { s.lighting.zones = [0, 1, 2, 3].map(() => clone(current)); s.lighting.zone_on = [true, true, true, true]; });
  $("swatches").replaceChildren(...sw, custom, allOn);

  // speed / brightness / direction
  $("speedField").hidden = isStatic || l.effect === "temperature";
  $("speed").value = l.speed;
  $("speed").oninput = () => change((s) => (s.lighting.speed = Number($("speed").value)));
  segmented($("brightness"), BRIGHTNESS, l.brightness, (v) => change((s) => (s.lighting.brightness = v)));
  $("directionField").hidden = !["wave", "shifting"].includes(l.effect);
  segmented($("direction"), DIRECTIONS, l.direction, (v) => change((s) => (s.lighting.direction = v)));

  paint(performance.now() / 1000);
}

function renderFans() {
  const f = settings.fans;
  const caps = (state && state.capabilities) || {};
  segmented($("fanModes"), FAN_MODES, f.mode, (v) => change((s) => (s.fans.mode = v)), caps.fans === false);
  const cards = [["cpu", "CPU fan"], ["gpu", "GPU fan"]].map(([id, title]) => {
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
  if (caps.fans === false) setStatus("Fan control isn't available on this system", true);
}

function renderGpu() {
  const caps = (state && state.capabilities) || {};
  $("gpuLevels").replaceChildren(...GPU_LEVELS.map(([v, name, desc, core, mem]) => {
    const b = document.createElement("button");
    b.type = "button"; b.className = "level";
    b.setAttribute("role", "radio");
    b.setAttribute("aria-checked", String(settings.gpu === v));
    b.disabled = caps.gpu_modes === false;
    b.innerHTML = `<h3>${name}</h3><dl><dt>Core clock</dt><dd>${core}</dd><dt>Memory clock</dt><dd>${mem}</dd></dl><p class="hint">${desc}</p>`;
    b.onclick = () => change((s) => (s.gpu = v));
    return b;
  }));
}

const GAUGES = [
  ["cpu_temp", "CPU temperature", "°C", 30, 100],
  ["gpu_temp", "GPU temperature", "°C", 30, 100],
  ["sys_temp", "System temperature", "°C", 20, 80],
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
  $("railTemps").innerHTML = `CPU <b>${s.cpu_temp ?? "–"}°</b><br>GPU <b>${s.gpu_temp ?? "–"}°</b>`;
  for (const [id, , unit, lo, hi] of GAUGES) {
    const v = document.querySelector(`[data-gauge="${id}"]`);
    if (v) v.innerHTML = fmt(s[id], unit);
    const c = document.querySelector(`[data-spark="${id}"]`);
    if (c) spark(c, history[id], lo, hi);
  }
  document.querySelectorAll('[data-live="cpu_fan_rpm"]').forEach((e) => (e.innerHTML = fmt(s.cpu_fan_rpm, "RPM")));
  document.querySelectorAll('[data-live="gpu_fan_rpm"]').forEach((e) => (e.innerHTML = fmt(s.gpu_fan_rpm, "RPM")));
  document.querySelectorAll('[data-live="cpu_temp_line"]').forEach((e) => (e.textContent = s.cpu_temp != null ? `CPU at ${s.cpu_temp} °C` : ""));
  document.querySelectorAll('[data-live="gpu_temp_line"]').forEach((e) => (e.textContent = s.gpu_temp != null ? `GPU at ${s.gpu_temp} °C` : ""));
  $("guardNote").hidden = !state.overheat_guard;
  if (settings && settings.lighting.effect === "temperature") paint(0);
}

function renderAll() {
  if (!settings) return;
  renderLighting();
  renderFans();
  renderGpu();
  $("keepApplied").checked = settings.keep_applied;
  renderLive();
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
$("installService").onclick = async () => {
  $("nsDetail").textContent = "Waiting for you to approve the administrator prompt…";
  try { await invoke("install_service"); } catch (e) { $("nsDetail").textContent = String(e); return; }
  setTimeout(() => refresh(true), 1500);
};

drawKeyboard();
buildGauges();
refresh(true);
setInterval(() => refresh(false), 2000);
requestAnimationFrame(loop);
