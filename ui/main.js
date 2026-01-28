const tauriApi = window.__TAURI__ || {};
const invoke = (tauriApi.tauri && tauriApi.tauri.invoke) || tauriApi.invoke;
const listen = (tauriApi.event && tauriApi.event.listen) || tauriApi.listen;
const openDialog = tauriApi.dialog && tauriApi.dialog.open;
const saveDialog = tauriApi.dialog && tauriApi.dialog.save;

const state = {
  midiInputs: [],
  audioOutputs: [],
  settings: null,
  session: "Idle",
  transport: { tick: 0, tempo_multiplier: 1.0, playing: false },
  pdfConvert: {
    running: false,
    stage: "Idle",
    outputPath: null,
    musicxmlPath: null,
    logPath: null,
  },
  scoreView: { title: null, ppq: 480, notes: [], targets: [], pedal: [], noteStarts: [], pedalStarts: [] },
  pressedNotes: new Set(),
  sustainDown: false,
  sf2Loaded: false,
};

const transportInterp = {
  lastTick: 0,
  lastUpdateMs: typeof performance !== "undefined" ? performance.now() : Date.now(),
  tickRate: 0,
  playing: false,
};

function onTransportUpdate(data) {
  const nowMs = typeof performance !== "undefined" ? performance.now() : Date.now();
  const dtMs = nowMs - transportInterp.lastUpdateMs;

  if (data && data.playing && transportInterp.playing && dtMs > 0) {
    const dtTick = (data.tick || 0) - (transportInterp.lastTick || 0);
    if (dtTick >= 0 && dtTick < 5_000_000) {
      transportInterp.tickRate = dtTick / dtMs;
    }
  }

  transportInterp.lastTick = data.tick || 0;
  transportInterp.lastUpdateMs = nowMs;
  transportInterp.playing = !!data.playing;
  if (!transportInterp.playing) {
    transportInterp.tickRate = 0;
  }
}

function displayTick() {
  const base = state.transport.tick || 0;
  if (!transportInterp.playing || transportInterp.tickRate <= 0) return base;
  const nowMs = typeof performance !== "undefined" ? performance.now() : Date.now();
  const dtMs = nowMs - transportInterp.lastUpdateMs;
  return Math.max(0, transportInterp.lastTick + dtMs * transportInterp.tickRate);
}

const viewButtons = document.querySelectorAll(".tab");
const views = {
  practice: document.getElementById("view-practice"),
  settings: document.getElementById("view-settings"),
};

viewButtons.forEach((button) => {
  button.addEventListener("click", () => {
    viewButtons.forEach((btn) => btn.classList.remove("is-active"));
    button.classList.add("is-active");
    const view = button.dataset.view;
    Object.values(views).forEach((node) => node.classList.remove("is-active"));
    views[view].classList.add("is-active");
  });
});

async function sendCommandAck(command) {
  if (!invoke) {
    showError(
      "Tauri JS API not available. Ensure `build.withGlobalTauri=true` in `src-tauri/tauri.conf.json` and rebuild."
    );
    return false;
  }
  try {
    await invoke("send_command", { command });
    return true;
  } catch (err) {
    showError(err);
    return false;
  }
}

function sendCommand(command) {
  return sendCommandAck(command);
}

function ensureMidiExtension(path) {
  if (!path) return path;
  if (path.endsWith("/") || path.endsWith("\\")) {
    return path;
  }
  const lower = path.toLowerCase();
  if (lower.endsWith(".mid") || lower.endsWith(".midi")) {
    return path;
  }
  return `${path}.mid`;
}

let errorTimer = null;
function showError(err) {
  const banner = document.getElementById("error-banner");
  if (!banner) return;
  const message =
    typeof err === "string"
      ? err
      : err && typeof err === "object" && "message" in err
        ? String(err.message)
        : JSON.stringify(err);
  banner.textContent = message;
  banner.classList.remove("is-hidden");
  if (errorTimer) {
    clearTimeout(errorTimer);
  }
  errorTimer = setTimeout(() => {
    banner.classList.add("is-hidden");
  }, 8000);
}

function setPdfConvertUi(running, statusText) {
  state.pdfConvert.running = running;
  if (statusText) {
    state.pdfConvert.stage = statusText;
  }

  const convertBtn = document.getElementById("btn-convert-pdf");
  const cancelBtn = document.getElementById("btn-cancel-pdf");
  const spinner = document.getElementById("pdf-convert-spinner");
  const statusEl = document.getElementById("pdf-convert-status");
  const openMidiBtn = document.getElementById("btn-open-midi-output");
  const openMusicXmlBtn = document.getElementById("btn-open-musicxml");
  const openLogBtn = document.getElementById("btn-open-omr-log");

  if (convertBtn) convertBtn.disabled = running;
  if (cancelBtn) cancelBtn.disabled = !running;
  if (spinner) spinner.classList.toggle("is-hidden", !running);
  if (statusEl) statusEl.textContent = statusText || (running ? "Working..." : "Idle");
  if (openMidiBtn) openMidiBtn.disabled = running || !state.pdfConvert.outputPath;
  if (openMusicXmlBtn) openMusicXmlBtn.disabled = running || !state.pdfConvert.musicxmlPath;
  if (openLogBtn) openLogBtn.disabled = running || !state.pdfConvert.logPath;

  const disableIds = [
    "pdf-path",
    "midi-output-path",
    "btn-browse-pdf",
    "btn-browse-midi-output",
    "btn-browse-midi-output-folder",
  ];
  disableIds.forEach((id) => {
    const el = document.getElementById(id);
    if (el) el.disabled = running;
  });
}

async function revealPath(path) {
  if (!invoke) {
    showError(
      "Tauri JS API not available. Ensure `build.withGlobalTauri=true` in `src-tauri/tauri.conf.json` and rebuild."
    );
    return;
  }
  if (!path) return;
  try {
    await invoke("reveal_path", { path });
  } catch (err) {
    showError(err);
  }
}

function setMidiLoadUi(running, statusText) {
  const loadBtn = document.getElementById("btn-load-midi");
  const spinner = document.getElementById("midi-load-spinner");
  const statusEl = document.getElementById("midi-load-status");

  if (loadBtn) loadBtn.disabled = running;
  if (spinner) spinner.classList.toggle("is-hidden", !running);
  if (statusEl) statusEl.textContent = statusText || (running ? "Loading..." : "Idle");

  const disableIds = ["midi-path", "btn-browse-midi"];
  disableIds.forEach((id) => {
    const el = document.getElementById(id);
    if (el) el.disabled = running;
  });
}

async function pickFile(options) {
  if (!openDialog) return null;
  const result = await openDialog(options);
  if (Array.isArray(result)) {
    return result[0] || null;
  }
  return result || null;
}

async function pickSaveFile(options) {
  if (!saveDialog) return null;
  const result = await saveDialog(options);
  return result || null;
}

function updateDeviceSelect(selectEl, devices, selectedId) {
  selectEl.innerHTML = "";
  if (!devices.length) {
    const option = document.createElement("option");
    option.textContent = "No devices";
    selectEl.appendChild(option);
    return;
  }

  devices.forEach((device) => {
    const option = document.createElement("option");
    option.value = device.id;
    option.textContent = device.name;
    if (selectedId && device.id === selectedId) {
      option.selected = true;
    }
    selectEl.appendChild(option);
  });
}

function pickFirstAvailable(devices) {
  if (!devices || devices.length === 0) return null;
  return devices.find((d) => d.is_available) || devices[0];
}

function ensureAudioSelected() {
  if (!state.audioOutputs.length) return;
  const selectedId = state.settings?.selected_audio_out;
  const hasSelected = selectedId && state.audioOutputs.some((d) => d.id === selectedId);
  if (hasSelected) return;

  const device = pickFirstAvailable(state.audioOutputs);
  if (!device) return;
  const selectEl = document.getElementById("audio-output");
  if (selectEl) selectEl.value = device.id;
  sendCommand({
    type: "SelectAudioOutput",
    payload: { device_id: device.id, config: null },
  });
}

function ensureMidiSelected() {
  if (!state.midiInputs.length) return;
  const selectedId = state.settings?.selected_midi_in;
  const hasSelected = selectedId && state.midiInputs.some((d) => d.id === selectedId);
  if (hasSelected) return;

  const device = pickFirstAvailable(state.midiInputs);
  if (!device) return;
  const selectEl = document.getElementById("midi-input");
  if (selectEl) selectEl.value = device.id;
  sendCommand({ type: "SelectMidiInput", payload: { device_id: device.id } });
}

function renderRecentInputs(events) {
  const list = document.getElementById("recent-inputs");
  if (!events || events.length === 0) {
    list.textContent = "No input yet.";
    return;
  }
  list.innerHTML = events
    .map((event) => {
      const [kind, payload] = Object.entries(event)[0] || [];
      if (kind === "NoteOn") {
        return `NoteOn ${payload.note} vel ${payload.velocity}`;
      }
      if (kind === "NoteOff") {
        return `NoteOff ${payload.note}`;
      }
      if (kind === "Cc64") {
        return `CC64 ${payload.value}`;
      }
      return kind;
    })
    .join("<br />");
}

function updateSessionSettings(settings) {
  if (!settings) return;
  document.getElementById("monitor-toggle").checked = settings.monitor_enabled;
  document.getElementById("master-volume").value = settings.master_volume;
  document.getElementById("master-volume-value").textContent = settings.master_volume.toFixed(2);
  document.getElementById("bus-user").value = settings.bus_user_volume;
  document.getElementById("bus-auto").value = settings.bus_autopilot_volume;
  document.getElementById("bus-metro").value = settings.bus_metronome_volume;
  const buffer = document.getElementById("audio-buffer");
  if (buffer) {
    buffer.value =
      settings.audio_buffer_size_frames && settings.audio_buffer_size_frames > 0
        ? String(settings.audio_buffer_size_frames)
        : "";
  }
  if (typeof settings.input_offset_ms === "number") {
    const inputOffset = document.getElementById("input-offset");
    const inputOffsetValue = document.getElementById("input-offset-value");
    if (inputOffset) inputOffset.value = settings.input_offset_ms;
    if (inputOffsetValue) inputOffsetValue.textContent = `${settings.input_offset_ms} ms`;
  }
  if (settings.audiveris_path) {
    document.getElementById("audiveris-path").value = settings.audiveris_path;
  }
  if (settings.default_sf2_path) {
    document.getElementById("sf2-path").value = settings.default_sf2_path;
    const status = document.getElementById("sf2-status");
    if (status && !status.dataset.locked) {
      status.textContent = "Selected (auto-loads on startup).";
    }
  }
}

function updateTransport() {
  document.getElementById("transport-tick").textContent = state.transport.tick;
  document.getElementById("transport-tempo").textContent = `${state.transport.tempo_multiplier.toFixed(2)}x`;
  document.getElementById("practice-status").textContent = state.session;
  const loopEl = document.getElementById("transport-loop");
  if (loopEl) {
    const loop = state.transport.loop_range;
    if (loop && typeof loop.start_tick === "number" && typeof loop.end_tick === "number") {
      const ppq = state.scoreView.ppq || 480;
      loopEl.textContent = `${formatBarBeat(loop.start_tick, ppq)}–${formatBarBeat(loop.end_tick, ppq)}`;
    } else {
      loopEl.textContent = "Off";
    }
  }
}

function formatBarBeat(tick, ppq) {
  const beatsPerBar = 4;
  const barLen = beatsPerBar * ppq;
  const clamped = Math.max(0, Math.floor(tick));
  const bar = Math.floor(clamped / barLen) + 1;
  const beat = Math.floor((clamped % barLen) / ppq) + 1;
  return `${bar}.${beat}`;
}

function lowerBound(arr, value) {
  let lo = 0;
  let hi = arr.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (arr[mid] < value) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

function isWhiteKey(note) {
  const n = ((note % 12) + 12) % 12;
  return n === 0 || n === 2 || n === 4 || n === 5 || n === 7 || n === 9 || n === 11;
}

function keyGeometry(width) {
  const minNote = 21;
  const maxNote = 108;
  const keys = [];
  const whiteNotes = [];
  for (let note = minNote; note <= maxNote; note += 1) {
    if (isWhiteKey(note)) whiteNotes.push(note);
  }
  const whiteKeyWidth = width / whiteNotes.length;
  const blackKeyWidth = whiteKeyWidth * 0.62;

  let whiteIndex = 0;
  for (let note = minNote; note <= maxNote; note += 1) {
    const white = isWhiteKey(note);
    if (white) {
      keys[note] = { x: whiteIndex * whiteKeyWidth, w: whiteKeyWidth, white: true };
      whiteIndex += 1;
      continue;
    }
    const prevWhiteIndex = whiteIndex - 1;
    const x = (prevWhiteIndex + 1) * whiteKeyWidth - blackKeyWidth / 2;
    keys[note] = { x, w: blackKeyWidth, white: false };
  }
  return { minNote, maxNote, whiteKeyWidth, blackKeyWidth, keys };
}

function readMidiLike(event) {
  const [kind, payload] = Object.entries(event || {})[0] || [];
  return { kind, payload: payload || {} };
}

function applyPressedState(event) {
  const { kind, payload } = readMidiLike(event);
  if (kind === "NoteOn") state.pressedNotes.add(payload.note);
  if (kind === "NoteOff") state.pressedNotes.delete(payload.note);
  if (kind === "Cc64") state.sustainDown = payload.value >= 64;
}

const pianoRollCanvas = document.getElementById("piano-roll");
const pianoRollCtx = pianoRollCanvas ? pianoRollCanvas.getContext("2d") : null;
const staffCanvas = document.getElementById("staff-view");
const staffCtx = staffCanvas ? staffCanvas.getContext("2d") : null;

let pianoRollMetrics = null;
const loopDrag = {
  active: false,
  startTick: 0,
  endTick: 0,
};

const DIATONIC_STEP_BY_PC = [0, 0, 1, 1, 2, 3, 3, 4, 4, 5, 5, 6];
const IS_SHARP_PC = [false, true, false, true, false, false, true, false, true, false, true, false];

function midiToDiatonicIndex(note) {
  const n = Math.max(0, Math.min(127, note | 0));
  const pc = ((n % 12) + 12) % 12;
  const octave = Math.floor(n / 12) - 1;
  return octave * 7 + DIATONIC_STEP_BY_PC[pc];
}

function drawStaffView(nowTick, ppq) {
  if (!staffCanvas || !staffCtx) return;

  const { w, h, dpr } = ensureCanvasSize(staffCanvas);
  const ctx = staffCtx;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, w, h);

  const margin = 14;
  const leftPad = 54;
  const rightPad = 14;
  const usableW = Math.max(1, w - leftPad - rightPad);

  const lineSpacing = Math.max(9, Math.min(12, Math.round(h / 18)));
  const staffHeight = lineSpacing * 4;
  const staffGap = lineSpacing * 3;

  const trebleTop = margin;
  const trebleBottom = trebleTop + staffHeight;
  const bassTop = trebleBottom + staffGap;
  const bassBottom = bassTop + staffHeight;

  ctx.fillStyle = "rgba(255, 255, 255, 0.35)";
  ctx.fillRect(0, 0, w, h);

  ctx.strokeStyle = "rgba(15, 23, 42, 0.18)";
  ctx.lineWidth = 1;

  const drawStaffLines = (topY) => {
    for (let i = 0; i < 5; i += 1) {
      const y = topY + i * lineSpacing;
      ctx.beginPath();
      ctx.moveTo(leftPad, y);
      ctx.lineTo(w - rightPad, y);
      ctx.stroke();
    }
  };

  drawStaffLines(trebleTop);
  drawStaffLines(bassTop);

  // Clef glyphs (fallback to plain text if the font doesn't support them).
  ctx.fillStyle = "rgba(15, 23, 42, 0.75)";
  ctx.textBaseline = "middle";
  ctx.font = `${Math.round(lineSpacing * 3.2)}px "Apple Symbols", "Segoe UI Symbol", serif`;
  ctx.fillText("𝄞", 18, trebleTop + lineSpacing * 2);
  ctx.font = `${Math.round(lineSpacing * 2.6)}px "Apple Symbols", "Segoe UI Symbol", serif`;
  ctx.fillText("𝄢", 18, bassTop + lineSpacing * 2);

  // Legend
  ctx.font = `12px "Avenir Next", "Trebuchet MS", sans-serif`;
  ctx.fillStyle = "rgba(15, 23, 42, 0.7)";
  ctx.fillText("Expected", leftPad, 10);
  ctx.fillText("Played", leftPad + 90, 10);

  const expectedColor = "rgba(15, 118, 110, 0.95)";
  const playedColor = "rgba(34, 197, 94, 0.9)";
  ctx.fillStyle = expectedColor;
  ctx.beginPath();
  ctx.arc(leftPad + 62, 10, 4, 0, Math.PI * 2);
  ctx.fill();
  ctx.fillStyle = playedColor;
  ctx.beginPath();
  ctx.arc(leftPad + 142, 10, 4, 0, Math.PI * 2);
  ctx.fill();

  // Compute next expected target chord.
  const targets = state.scoreView.targets || [];
  let nextTarget = null;
  if (targets.length) {
    let lo = 0;
    let hi = targets.length;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (targets[mid].tick < nowTick) lo = mid + 1;
      else hi = mid;
    }
    nextTarget = targets[lo] || null;
  }

  const expectedNotes = new Set((nextTarget && nextTarget.notes) || []);
  const pressedNotes = state.pressedNotes || new Set();

  const trebleRef = midiToDiatonicIndex(64); // E4 bottom line
  const bassRef = midiToDiatonicIndex(43); // G2 bottom line
  const stepSize = lineSpacing / 2;

  const noteToPos = (note) => {
    const di = midiToDiatonicIndex(note);
    const useTreble = note >= 60;
    const ref = useTreble ? trebleRef : bassRef;
    const bottomY = useTreble ? trebleBottom : bassBottom;
    const stepsFromBottom = di - ref;
    const y = bottomY - stepsFromBottom * stepSize;
    return { useTreble, bottomY, stepsFromBottom, y };
  };

  const drawLedgerLines = (x, bottomY, stepsFromBottom) => {
    const topLineStep = 8;
    ctx.strokeStyle = "rgba(15, 23, 42, 0.2)";
    ctx.lineWidth = 1;
    if (stepsFromBottom < 0) {
      for (let s = -2; s >= stepsFromBottom; s -= 2) {
        const y = bottomY - s * stepSize;
        ctx.beginPath();
        ctx.moveTo(x - 16, y);
        ctx.lineTo(x + 16, y);
        ctx.stroke();
      }
    } else if (stepsFromBottom > topLineStep) {
      for (let s = 10; s <= stepsFromBottom; s += 2) {
        const y = bottomY - s * stepSize;
        ctx.beginPath();
        ctx.moveTo(x - 16, y);
        ctx.lineTo(x + 16, y);
        ctx.stroke();
      }
    }
  };

  const drawNoteHead = (x, y, fill, stroke, sharp) => {
    const rx = Math.max(6, Math.round(lineSpacing * 0.65));
    const ry = Math.max(4, Math.round(lineSpacing * 0.45));
    if (sharp) {
      ctx.fillStyle = "rgba(15, 23, 42, 0.75)";
      ctx.font = `${Math.round(lineSpacing * 1.2)}px Menlo, monospace`;
      ctx.fillText("#", x - rx - 14, y + 1);
    }
    ctx.beginPath();
    ctx.ellipse(x, y, rx, ry, -0.35, 0, Math.PI * 2);
    if (fill) {
      ctx.fillStyle = fill;
      ctx.fill();
    }
    if (stroke) {
      ctx.strokeStyle = stroke;
      ctx.lineWidth = 2;
      ctx.stroke();
    }
  };

  const centerX = leftPad + Math.round(usableW * 0.5);
  const expectedX = centerX - 22;
  const playedX = centerX + 22;

  const drawChord = (notes, xBase, style) => {
    const arr = Array.from(notes).filter((n) => Number.isFinite(n)).sort((a, b) => a - b);
    for (let i = 0; i < arr.length; i += 1) {
      const note = arr[i] | 0;
      const { bottomY, stepsFromBottom, y } = noteToPos(note);
      const x = xBase + (i % 2) * 10;
      drawLedgerLines(x, bottomY, stepsFromBottom);
      const pc = ((note % 12) + 12) % 12;
      const sharp = IS_SHARP_PC[pc];
      drawNoteHead(x, y, style.fill, style.stroke, sharp);
    }
  };

  drawChord(expectedNotes, expectedX, { fill: null, stroke: expectedColor });
  drawChord(pressedNotes, playedX, { fill: playedColor, stroke: "rgba(15, 23, 42, 0.25)" });

  // Time hint
  if (nextTarget && typeof nextTarget.tick === "number") {
    const label = `Next: ${formatBarBeat(nextTarget.tick, ppq || 480)}`;
    ctx.fillStyle = "rgba(15, 23, 42, 0.65)";
    ctx.font = `12px Menlo, monospace`;
    ctx.fillText(label, w - rightPad - ctx.measureText(label).width, 10);
  }
}

function ensureCanvasSize(canvas) {
  const rect = canvas.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  const w = Math.max(1, Math.round(rect.width));
  const h = Math.max(1, Math.round(rect.height));
  const needW = Math.round(w * dpr);
  const needH = Math.round(h * dpr);
  if (canvas.width !== needW || canvas.height !== needH) {
    canvas.width = needW;
    canvas.height = needH;
  }
  return { w, h, dpr };
}

function drawPianoRoll() {
  if (!pianoRollCanvas || !pianoRollCtx) return;

  const { w, h, dpr } = ensureCanvasSize(pianoRollCanvas);
  const ctx = pianoRollCtx;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

  const ppq = state.scoreView.ppq || 480;
  const nowTick = displayTick();
  drawStaffView(nowTick, ppq);
  const beatsAhead = 8;
  const behindBeats = 4;

  const keyboardHeight = Math.min(110, Math.max(72, Math.round(h * 0.22)));
  const keyboardY = h - keyboardHeight;
  const hudHeight = 22;
  const nowLineY = keyboardY - hudHeight;
  const pxPerTick = nowLineY / (beatsAhead * ppq);

  pianoRollMetrics = { ppq, nowTick, nowLineY, pxPerTick, keyboardY };

  ctx.clearRect(0, 0, w, h);

  ctx.fillStyle = "rgba(255, 255, 255, 0.55)";
  ctx.fillRect(0, 0, w, keyboardY);

  const geom = keyGeometry(w);
  for (let note = geom.minNote; note <= geom.maxNote; note += 1) {
    const k = geom.keys[note];
    if (!k) continue;
    if (!k.white) continue;
    ctx.fillStyle = "rgba(15, 23, 42, 0.02)";
    ctx.fillRect(k.x, 0, k.w, keyboardY);
  }
  for (let note = geom.minNote; note <= geom.maxNote; note += 1) {
    const k = geom.keys[note];
    if (!k) continue;
    if (k.white) continue;
    ctx.fillStyle = "rgba(15, 23, 42, 0.045)";
    ctx.fillRect(k.x, 0, k.w, keyboardY);
  }

  const tickToY = (tick) => nowLineY - (tick - nowTick) * pxPerTick;

  const visibleMin = nowTick - behindBeats * ppq;
  const visibleMax = nowTick + beatsAhead * ppq;
  const noteStarts = state.scoreView.noteStarts || [];
  const notes = state.scoreView.notes || [];
  const pedal = state.scoreView.pedal || [];
  const pedalStarts = state.scoreView.pedalStarts || [];

  const startIdx = lowerBound(noteStarts, visibleMin);
  const endIdx = lowerBound(noteStarts, visibleMax + 1);

  const beatStart = Math.floor(nowTick / ppq) * ppq;
  ctx.lineWidth = 1;
  for (let t = beatStart; t <= visibleMax; t += ppq) {
    const y = tickToY(t);
    if (y < 0 || y > nowLineY) continue;
    const isNow = Math.abs(t - nowTick) < 1;
    const isMeasure = ((t / ppq) | 0) % 4 === 0;
    ctx.strokeStyle = isNow
      ? "rgba(15, 118, 110, 0.78)"
      : isMeasure
        ? "rgba(15, 23, 42, 0.14)"
        : "rgba(15, 23, 42, 0.07)";
    ctx.beginPath();
    ctx.moveTo(0, y);
    ctx.lineTo(w, y);
    ctx.stroke();
  }

  const drawLoopRange = (range, fill, stroke) => {
    if (!range) return;
    const startTick = range.start_tick;
    const endTick = range.end_tick;
    if (typeof startTick !== "number" || typeof endTick !== "number") return;
    const a = Math.max(0, Math.min(startTick, endTick));
    const b = Math.max(0, Math.max(startTick, endTick));
    if (b <= a) return;

    const y1 = tickToY(a);
    const y2 = tickToY(b);
    const top = Math.max(0, Math.min(y1, y2));
    const bottom = Math.min(nowLineY, Math.max(y1, y2));
    if (bottom <= 0 || top >= nowLineY) return;

    ctx.fillStyle = fill;
    ctx.fillRect(0, top, w, bottom - top);

    ctx.strokeStyle = stroke;
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(0, y1);
    ctx.lineTo(w, y1);
    ctx.stroke();
    ctx.beginPath();
    ctx.moveTo(0, y2);
    ctx.lineTo(w, y2);
    ctx.stroke();
  };

  drawLoopRange(
    state.transport.loop_range,
    "rgba(20, 184, 166, 0.08)",
    "rgba(15, 118, 110, 0.32)"
  );
  if (loopDrag.active) {
    drawLoopRange(
      { start_tick: loopDrag.startTick, end_tick: loopDrag.endTick },
      "rgba(20, 184, 166, 0.18)",
      "rgba(15, 118, 110, 0.7)"
    );
  }

  const pedalBarW = 8;
  ctx.fillStyle = "rgba(245, 158, 11, 0.08)";
  ctx.fillRect(0, 0, pedalBarW, nowLineY);
  let pedalIdx = lowerBound(pedalStarts, visibleMin);
  if (pedalIdx > 0) pedalIdx -= 1;
  for (let i = pedalIdx; i < pedal.length; i += 1) {
    const seg = pedal[i];
    if (!seg) continue;
    if (seg.start_tick > visibleMax) break;
    if (seg.end_tick < visibleMin) continue;
    const y1 = tickToY(seg.start_tick);
    const y2 = tickToY(seg.end_tick);
    const top = Math.max(0, Math.min(y1, y2));
    const bottom = Math.min(nowLineY, Math.max(y1, y2));
    if (bottom <= 0 || top >= nowLineY) continue;
    ctx.fillStyle = "rgba(245, 158, 11, 0.55)";
    ctx.fillRect(1, top, pedalBarW - 2, bottom - top);
  }

  const isPedalDown = (() => {
    if (!pedal.length) return false;
    let idx = lowerBound(pedalStarts, nowTick);
    idx -= 1;
    if (idx < 0 || idx >= pedal.length) return false;
    const seg = pedal[idx];
    return seg && seg.start_tick <= nowTick && nowTick < seg.end_tick;
  })();

  const autopilotNow = new Set();
  const lanePadding = 1.5;

  const handHue = (hand) => {
    if (hand === "Left") return 285;
    if (hand === "Right") return 160;
    return 35;
  };
  const noteFill = (n, white) => {
    const vel = Math.min(1, Math.max(0, (n.velocity || 80) / 127));
    const hue = handHue(n.hand);
    const sat = 82;
    const light = white ? 44 + vel * 8 : 40 + vel * 10;
    const alpha = 0.25 + vel * 0.65;
    return `hsla(${hue}, ${sat}%, ${light}%, ${alpha})`;
  };

  for (let i = startIdx; i < endIdx; i += 1) {
    const n = notes[i];
    if (!n) continue;
    if (n.end_tick < visibleMin || n.start_tick > visibleMax) continue;
    const k = geom.keys[n.note];
    if (!k) continue;

    if (n.start_tick <= nowTick && nowTick < n.end_tick) {
      autopilotNow.add(n.note);
    }

    let y1 = tickToY(n.start_tick);
    let y2 = tickToY(n.end_tick);
    if (!Number.isFinite(y1) || !Number.isFinite(y2)) continue;
    const top = Math.max(0, Math.min(y1, y2));
    const bottom = Math.min(nowLineY, Math.max(y1, y2));
    if (bottom <= 0 || top >= nowLineY) continue;

    const x = k.x + lanePadding;
    const width = Math.max(1, k.w - lanePadding * 2);
    const height = Math.max(2, bottom - top);

    ctx.fillStyle = noteFill(n, k.white);
    ctx.fillRect(x, top, width, height);
    ctx.strokeStyle = "rgba(15, 23, 42, 0.18)";
    ctx.strokeRect(x + 0.5, top + 0.5, width - 1, height - 1);
  }

  const targets = state.scoreView.targets || [];
  let nextTarget = null;
  if (targets.length) {
    let lo = 0;
    let hi = targets.length;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (targets[mid].tick < nowTick) lo = mid + 1;
      else hi = mid;
    }
    nextTarget = targets[lo] || null;
  }

  if (nextTarget) {
    const y = tickToY(nextTarget.tick);
    if (y >= 0 && y <= nowLineY) {
      ctx.strokeStyle = "rgba(34, 197, 94, 0.95)";
      ctx.lineWidth = 2;
      for (const note of nextTarget.notes || []) {
        const k = geom.keys[note];
        if (!k) continue;
        ctx.strokeRect(k.x + 1, y - 6, k.w - 2, 12);
      }
    }
  }

  ctx.fillStyle = "rgba(20, 184, 166, 0.16)";
  ctx.fillRect(0, nowLineY - 2, w, 4);

  const pressed = state.pressedNotes || new Set();
  const expected = new Set((nextTarget && nextTarget.notes) || []);

  ctx.fillStyle = "rgba(255, 255, 255, 0.95)";
  ctx.fillRect(0, keyboardY, w, keyboardHeight);

  ctx.strokeStyle = "rgba(15, 23, 42, 0.12)";
  ctx.lineWidth = 1;
  for (let note = geom.minNote; note <= geom.maxNote; note += 1) {
    const k = geom.keys[note];
    if (!k || !k.white) continue;
    ctx.fillStyle = "rgba(255, 255, 255, 0.98)";
    ctx.fillRect(k.x, keyboardY, k.w, keyboardHeight);
    ctx.strokeRect(k.x, keyboardY, k.w, keyboardHeight);

    const highlight = pressed.has(note) || expected.has(note) || autopilotNow.has(note);
    if (highlight) {
      ctx.fillStyle = pressed.has(note)
        ? "rgba(34, 197, 94, 0.35)"
        : expected.has(note)
          ? "rgba(20, 184, 166, 0.24)"
          : "rgba(20, 184, 166, 0.12)";
      ctx.fillRect(k.x + 1, keyboardY + 1, k.w - 2, keyboardHeight - 2);
    }
  }

  for (let note = geom.minNote; note <= geom.maxNote; note += 1) {
    const k = geom.keys[note];
    if (!k || k.white) continue;
    const keyH = Math.round(keyboardHeight * 0.62);
    ctx.fillStyle = "rgba(15, 23, 42, 0.86)";
    ctx.fillRect(k.x, keyboardY, k.w, keyH);

    const highlight = pressed.has(note) || expected.has(note) || autopilotNow.has(note);
    if (highlight) {
      ctx.fillStyle = pressed.has(note)
        ? "rgba(34, 197, 94, 0.55)"
        : expected.has(note)
          ? "rgba(45, 212, 191, 0.55)"
          : "rgba(45, 212, 191, 0.33)";
      ctx.fillRect(k.x + 1, keyboardY + 1, k.w - 2, keyH - 2);
    }
  }

  if (state.sustainDown || isPedalDown) {
    ctx.fillStyle = "rgba(15, 23, 42, 0.75)";
    ctx.font = "12px Menlo, monospace";
    ctx.fillText(state.sustainDown ? "Sustain (Input)" : "Sustain (Score)", 12, keyboardY + 16);
  }

  requestAnimationFrame(drawPianoRoll);
}

if (pianoRollCanvas && pianoRollCtx) {
  requestAnimationFrame(drawPianoRoll);
}

function snapDown(tick, step) {
  if (step <= 0) return tick;
  return Math.floor(tick / step) * step;
}

function snapUp(tick, step) {
  if (step <= 0) return tick;
  return Math.ceil(tick / step) * step;
}

function setLoopRange(startTick, endTick) {
  const start = Math.max(0, Math.floor(startTick));
  const end = Math.max(0, Math.floor(endTick));
  if (end <= start) return;
  state.transport.loop_range = { start_tick: start, end_tick: end };
  updateTransport();
  sendCommand({ type: "SetLoop", payload: { enabled: true, start_tick: start, end_tick: end } });
}

function clearLoopRange() {
  state.transport.loop_range = null;
  updateTransport();
  sendCommand({ type: "SetLoop", payload: { enabled: false, start_tick: 0, end_tick: 0 } });
}

function seekToTick(tick) {
  const t = Math.max(0, Math.floor(tick));
  sendCommand({ type: "Seek", payload: { tick: t } });
}

function mouseYToTick(clientY) {
  if (!pianoRollCanvas || !pianoRollMetrics) return null;
  const rect = pianoRollCanvas.getBoundingClientRect();
  const y = clientY - rect.top;
  const { nowTick, nowLineY, pxPerTick } = pianoRollMetrics;
  if (!Number.isFinite(y) || pxPerTick <= 0) return null;
  if (y < 0 || y > nowLineY) return null;
  const tick = nowTick + (nowLineY - y) / pxPerTick;
  if (!Number.isFinite(tick)) return null;
  return Math.max(0, tick);
}

if (pianoRollCanvas) {
  pianoRollCanvas.addEventListener("mousedown", (event) => {
    if (event.button !== 0) return;
    const tick = mouseYToTick(event.clientY);
    if (tick == null) return;
    loopDrag.active = true;
    loopDrag.startTick = tick;
    loopDrag.endTick = tick;
    event.preventDefault();
  });

  window.addEventListener("mousemove", (event) => {
    if (!loopDrag.active) return;
    const tick = mouseYToTick(event.clientY);
    if (tick == null) return;
    loopDrag.endTick = tick;
  });

  window.addEventListener("mouseup", () => {
    if (!loopDrag.active) return;
    loopDrag.active = false;
    if (!pianoRollMetrics) return;
    const ppq = pianoRollMetrics.ppq || 480;
    const a = Math.min(loopDrag.startTick, loopDrag.endTick);
    const b = Math.max(loopDrag.startTick, loopDrag.endTick);
    if (b - a < ppq * 0.5) {
      seekToTick(snapDown(a, ppq));
      return;
    }
    const start = snapDown(a, ppq);
    let end = snapUp(b, ppq);
    if (end <= start) end = start + ppq;
    setLoopRange(start, end);
    seekToTick(start);
  });
}

if (!listen) {
  window.addEventListener("DOMContentLoaded", () => {
    showError(
      "Tauri event API not available (window.__TAURI__ missing). Rebuild with `build.withGlobalTauri=true`."
    );
  });
} else {
  listen("core_event", (event) => {
    const payload = event.payload;
    if (!payload) return;
    const { type, payload: data } = payload;

    switch (type) {
      case "ScoreViewUpdated":
        state.scoreView.title = data.title || null;
        state.scoreView.ppq = data.ppq || 480;
        state.scoreView.notes = Array.isArray(data.notes) ? data.notes : [];
        state.scoreView.targets = Array.isArray(data.targets) ? data.targets : [];
        state.scoreView.pedal = Array.isArray(data.pedal) ? data.pedal : [];
        state.scoreView.pedal.sort((a, b) => (a.start_tick || 0) - (b.start_tick || 0));
        state.scoreView.noteStarts = state.scoreView.notes.map((n) => n.start_tick || 0);
        state.scoreView.pedalStarts = state.scoreView.pedal.map((p) => p.start_tick || 0);
        document.getElementById("score-title").textContent =
          state.scoreView.title || `PPQ ${state.scoreView.ppq}`;
        break;
      case "OmrProgress":
        setPdfConvertUi(true, data.stage);
        break;
      case "OmrDiagnostics":
        if (data.severity === "error") {
          showError(data.message);
        }
        break;
      case "PdfToMidiFinished":
        state.pdfConvert.outputPath = data.output_path || null;
        state.pdfConvert.musicxmlPath = data.musicxml_path || null;
        state.pdfConvert.logPath = data.diagnostics_path || null;
        {
          const lines = [];
          if (data.ok && data.output_path) {
            lines.push(`Wrote MIDI: ${data.output_path}`);
            if (data.musicxml_path) lines.push(`MusicXML: ${data.musicxml_path}`);
            if (data.diagnostics_path) lines.push(`Log: ${data.diagnostics_path}`);
          } else {
            lines.push(data.message || "Conversion failed");
            if (data.diagnostics_path) lines.push(`Log: ${data.diagnostics_path}`);
          }
          setPdfConvertUi(false, lines.join("\n"));
        }
        if (data.ok) {
          document.getElementById("midi-path").value = data.output_path;
          document.getElementById("midi-output-path").value = data.output_path;
          (async () => {
            const source = data.musicxml_path
              ? { type: "MusicXmlFile", payload: data.musicxml_path }
              : { type: "MidiFile", payload: data.output_path };
            setPdfConvertUi(true, data.musicxml_path ? "Loading MusicXML..." : "Loading MIDI...");
            setMidiLoadUi(true, "Loading...");
            const loaded = await sendCommandAck({
              type: "LoadScore",
              payload: { source },
            });
            if (loaded) {
              setPdfConvertUi(false, "Loaded into Practice");
              setMidiLoadUi(false, "Loaded");
            } else {
              setPdfConvertUi(false, "Saved, but failed to load into Practice");
              setMidiLoadUi(false, "Failed");
            }
          })();
        }
        break;
      case "MidiInputsUpdated":
        state.midiInputs = data.devices;
        updateDeviceSelect(
          document.getElementById("midi-input"),
          state.midiInputs,
          state.settings?.selected_midi_in
        );
        ensureMidiSelected();
        break;
      case "AudioOutputsUpdated":
        state.audioOutputs = data.devices;
        updateDeviceSelect(
          document.getElementById("audio-output"),
          state.audioOutputs,
          state.settings?.selected_audio_out
        );
        ensureAudioSelected();
        break;
      case "SessionStateUpdated":
        state.session = data.state;
        state.settings = data.settings;
        updateSessionSettings(data.settings);
        if (state.audioOutputs.length) {
          updateDeviceSelect(
            document.getElementById("audio-output"),
            state.audioOutputs,
            state.settings?.selected_audio_out
          );
        }
        if (state.midiInputs.length) {
          updateDeviceSelect(
            document.getElementById("midi-input"),
            state.midiInputs,
            state.settings?.selected_midi_in
          );
        }
        updateTransport();
        ensureAudioSelected();
        ensureMidiSelected();
        break;
      case "SoundFontStatus": {
        state.sf2Loaded = !!data.loaded;
        if (data.path) {
          document.getElementById("sf2-path").value = data.path;
        }
        const status = document.getElementById("sf2-status");
        if (status) {
          status.dataset.locked = "1";
          if (data.loaded) {
            const name = data.name || "SoundFont";
            const count =
              typeof data.preset_count === "number" ? ` (${data.preset_count} presets)` : "";
            status.textContent = `Loaded: ${name}${count}`;
          } else {
            const message = data.message ? `Failed to load: ${data.message}` : "SoundFont not loaded.";
            status.textContent = message;
          }
        }
        break;
      }
      case "TransportUpdated":
        state.transport = data;
        updateTransport();
        onTransportUpdate(data);
        break;
      case "JudgeFeedback":
        document.getElementById("judge-grade").textContent = data.grade;
        break;
      case "ScoreSummaryUpdated":
        document.getElementById("judge-combo").textContent = data.combo;
        document.getElementById("judge-score").textContent = data.score;
        document.getElementById("judge-accuracy").textContent = `${Math.round(data.accuracy * 100)}%`;
        break;
      case "MidiInputEvent":
        applyPressedState(data.event);
        break;
      case "RecentInputEvents":
        renderRecentInputs(data.events);
        break;
      default:
        break;
    }
  });
}

// Practice controls

document.getElementById("btn-play").addEventListener("click", () => {
  sendCommand({ type: "StartPractice" });
});

document.getElementById("btn-pause").addEventListener("click", () => {
  sendCommand({ type: "PausePractice" });
});

document.getElementById("btn-stop").addEventListener("click", () => {
  sendCommand({ type: "StopPractice" });
});

document.querySelectorAll(".tempo-btn").forEach((button) => {
  button.addEventListener("click", () => {
    const tempo = parseFloat(button.dataset.tempo);
    if (!Number.isFinite(tempo) || tempo <= 0) return;
    state.transport.tempo_multiplier = tempo;
    updateTransport();
    sendCommand({ type: "SetTempoMultiplier", payload: { x: tempo } });
  });
});

document.getElementById("btn-loop-clear").addEventListener("click", () => {
  clearLoopRange();
});

document.getElementById("input-offset").addEventListener("input", (event) => {
  const value = parseInt(event.target.value, 10);
  if (!Number.isFinite(value)) return;
  document.getElementById("input-offset-value").textContent = `${value} ms`;
  sendCommand({ type: "SetInputOffsetMs", payload: { ms: value } });
});

document.getElementById("btn-input-offset-reset").addEventListener("click", () => {
  document.getElementById("input-offset").value = 0;
  document.getElementById("input-offset-value").textContent = "0 ms";
  sendCommand({ type: "SetInputOffsetMs", payload: { ms: 0 } });
});

document.getElementById("btn-load-midi").addEventListener("click", () => {
  const path = document.getElementById("midi-path").value.trim();
  if (!path) return;
  (async () => {
    setMidiLoadUi(true, "Loading...");
    const ok = await sendCommandAck({
      type: "LoadScore",
      payload: { source: { type: "MidiFile", payload: path } },
    });
    setMidiLoadUi(false, ok ? "Loaded" : "Failed");
  })();
});

document.getElementById("btn-load-demo").addEventListener("click", () => {
  (async () => {
    setMidiLoadUi(true, "Loading demo...");
    const ok = await sendCommandAck({
      type: "LoadScore",
      payload: { source: { type: "InternalDemo", payload: "c_major_scale" } },
    });
    setMidiLoadUi(false, ok ? "Loaded demo" : "Failed");
  })();
});

document.getElementById("btn-browse-midi").addEventListener("click", async () => {
  const file = await pickFile({
    title: "Select MIDI file",
    filters: [{ name: "MIDI", extensions: ["mid", "midi"] }],
  });
  if (file) {
    document.getElementById("midi-path").value = file;
    setMidiLoadUi(false, "Ready");
  }
});

document.getElementById("btn-browse-pdf").addEventListener("click", async () => {
  const file = await pickFile({
    title: "Select PDF score",
    filters: [{ name: "PDF", extensions: ["pdf"] }],
  });
  if (file) {
    document.getElementById("pdf-path").value = file;
  }
});

document.getElementById("btn-browse-midi-output").addEventListener("click", async () => {
  const file = await pickSaveFile({
    title: "Save MIDI file",
    filters: [{ name: "MIDI", extensions: ["mid", "midi"] }],
  });
  if (file) {
    document.getElementById("midi-output-path").value = ensureMidiExtension(file);
  }
});

document.getElementById("btn-browse-midi-output-folder").addEventListener("click", async () => {
  const folder = await pickFile({
    title: "Select output folder",
    directory: true,
  });
  if (folder) {
    const suffix = folder.endsWith("/") || folder.endsWith("\\") ? "" : "/";
    document.getElementById("midi-output-path").value = `${folder}${suffix}`;
  }
});

document.getElementById("btn-convert-pdf").addEventListener("click", () => {
  const pdfPath = document.getElementById("pdf-path").value.trim();
  let outputPath = document.getElementById("midi-output-path").value.trim();
  const audiverisPath = document.getElementById("audiveris-path").value.trim();
  if (!pdfPath) return;
  if (outputPath) {
    const normalizedOutputPath = ensureMidiExtension(outputPath);
    if (normalizedOutputPath !== outputPath) {
      outputPath = normalizedOutputPath;
      document.getElementById("midi-output-path").value = outputPath;
    }
  }
  (async () => {
    setPdfConvertUi(true, "Starting...");
    const ok = await sendCommandAck({
      type: "ConvertPdfToMidi",
      payload: {
        pdf_path: pdfPath,
        output_path: outputPath || "",
        audiveris_path: audiverisPath || null,
      },
    });
    if (!ok) {
      setPdfConvertUi(false, "Idle");
    }
  })();
});

document.getElementById("btn-cancel-pdf").addEventListener("click", () => {
  (async () => {
    setPdfConvertUi(true, "Cancelling...");
    await sendCommandAck({ type: "CancelPdfToMidi" });
  })();
});

document.getElementById("btn-open-midi-output").addEventListener("click", () => {
  revealPath(state.pdfConvert.outputPath);
});

document.getElementById("btn-open-musicxml").addEventListener("click", () => {
  revealPath(state.pdfConvert.musicxmlPath);
});

document.getElementById("btn-open-omr-log").addEventListener("click", () => {
  revealPath(state.pdfConvert.logPath);
});

// Settings controls

document.getElementById("btn-refresh-audio").addEventListener("click", () => {
  sendCommand({ type: "ListAudioOutputs" });
});

document.getElementById("btn-test-audio").addEventListener("click", () => {
  sendCommand({ type: "TestAudio" });
});

document.getElementById("btn-refresh-midi").addEventListener("click", () => {
  sendCommand({ type: "ListMidiInputs" });
});

document.getElementById("audio-output").addEventListener("change", (event) => {
  const id = event.target.value;
  if (!id) return;
  sendCommand({
    type: "SelectAudioOutput",
    payload: { device_id: id, config: null },
  });
});

document.getElementById("audio-buffer").addEventListener("change", (event) => {
  const id = document.getElementById("audio-output").value;
  if (!id) return;
  const value = String(event.target.value || "").trim();
  const frames = value ? parseInt(value, 10) : 0;
  const desiredFrames = Number.isFinite(frames) && frames > 0 ? frames : null;

  const device = state.audioOutputs.find((d) => d.id === id);
  const base = device ? device.default_config : { sample_rate_hz: 48000, channels: 2 };

  sendCommand({
    type: "SelectAudioOutput",
    payload: {
      device_id: id,
      config: {
        sample_rate_hz: base.sample_rate_hz,
        channels: base.channels,
        buffer_size_frames: desiredFrames,
      },
    },
  });
});

document.getElementById("midi-input").addEventListener("change", (event) => {
  const id = event.target.value;
  if (!id) return;
  sendCommand({
    type: "SelectMidiInput",
    payload: { device_id: id },
  });
});

document.getElementById("monitor-toggle").addEventListener("change", (event) => {
  sendCommand({ type: "SetMonitorEnabled", payload: { enabled: event.target.checked } });
});

document.getElementById("master-volume").addEventListener("input", (event) => {
  const volume = parseFloat(event.target.value);
  document.getElementById("master-volume-value").textContent = volume.toFixed(2);
  sendCommand({ type: "SetMasterVolume", payload: { volume } });
});

document.getElementById("bus-user").addEventListener("input", (event) => {
  const volume = parseFloat(event.target.value);
  sendCommand({ type: "SetBusVolume", payload: { bus: "UserMonitor", volume } });
});


document.getElementById("bus-auto").addEventListener("input", (event) => {
  const volume = parseFloat(event.target.value);
  sendCommand({ type: "SetBusVolume", payload: { bus: "Autopilot", volume } });
});


document.getElementById("bus-metro").addEventListener("input", (event) => {
  const volume = parseFloat(event.target.value);
  sendCommand({ type: "SetBusVolume", payload: { bus: "MetronomeFx", volume } });
});


document.getElementById("btn-export-diag").addEventListener("click", () => {
  const path = document.getElementById("diag-path").value.trim();
  if (!path) return;
  sendCommand({ type: "ExportDiagnostics", payload: { path } });
});

document.getElementById("btn-browse-diag").addEventListener("click", async () => {
  const folder = await pickFile({
    title: "Select diagnostics folder",
    directory: true,
  });
  if (folder) {
    document.getElementById("diag-path").value = folder;
  }
});

document.getElementById("btn-browse-audiveris").addEventListener("click", async () => {
  const file = await pickFile({ title: "Select Audiveris executable" });
  if (file) {
    document.getElementById("audiveris-path").value = file;
  }
});

document.getElementById("btn-save-audiveris").addEventListener("click", () => {
  const path = document.getElementById("audiveris-path").value.trim();
  if (!path) return;
  sendCommand({ type: "SetAudiverisPath", payload: { path } });
});

document.getElementById("btn-browse-sf2").addEventListener("click", async () => {
  const file = await pickFile({
    title: "Select SoundFont (.sf2)",
    filters: [{ name: "SoundFont", extensions: ["sf2"] }],
  });
  if (file) {
    document.getElementById("sf2-path").value = file;
    document.getElementById("sf2-status").textContent = "Ready";
  }
});

document.getElementById("btn-load-sf2").addEventListener("click", () => {
  const path = document.getElementById("sf2-path").value.trim();
  if (!path) return;
  (async () => {
    document.getElementById("sf2-status").textContent = "Loading...";
    const ok = await sendCommandAck({ type: "LoadSoundFont", payload: { path } });
    state.sf2Loaded = ok;
    document.getElementById("sf2-status").textContent = ok ? "Loaded" : "Failed";
  })();
});

// Initialize
sendCommand({ type: "ListAudioOutputs" });
sendCommand({ type: "ListMidiInputs" });
sendCommand({ type: "GetSessionState" });
