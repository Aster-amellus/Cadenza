const { invoke } = window.__TAURI__.tauri;
const { listen } = window.__TAURI__.event;
const { open: openDialog } = window.__TAURI__.dialog || {};

const state = {
  midiInputs: [],
  audioOutputs: [],
  settings: null,
  session: "Idle",
  transport: { tick: 0, tempo_multiplier: 1.0, playing: false },
};

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

function sendCommand(command) {
  return invoke("send_command", { command });
}

async function pickFile(options) {
  if (!openDialog) return null;
  const result = await openDialog(options);
  if (Array.isArray(result)) {
    return result[0] || null;
  }
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
}

function updateTransport() {
  document.getElementById("transport-tick").textContent = state.transport.tick;
  document.getElementById("transport-tempo").textContent = `${state.transport.tempo_multiplier.toFixed(2)}x`;
  document.getElementById("practice-status").textContent = state.session;
}

listen("core_event", (event) => {
  const payload = event.payload;
  if (!payload) return;
  const { type, payload: data } = payload;

  switch (type) {
    case "MidiInputsUpdated":
      state.midiInputs = data.devices;
      updateDeviceSelect(
        document.getElementById("midi-input"),
        state.midiInputs,
        state.settings?.selected_midi_in
      );
      break;
    case "AudioOutputsUpdated":
      state.audioOutputs = data.devices;
      updateDeviceSelect(
        document.getElementById("audio-output"),
        state.audioOutputs,
        state.settings?.selected_audio_out
      );
      break;
    case "SessionStateUpdated":
      state.session = data.state;
      state.settings = data.settings;
      updateSessionSettings(data.settings);
      updateTransport();
      break;
    case "TransportUpdated":
      state.transport = data;
      updateTransport();
      break;
    case "JudgeFeedback":
      document.getElementById("judge-grade").textContent = data.grade;
      break;
    case "ScoreSummaryUpdated":
      document.getElementById("judge-combo").textContent = data.combo;
      document.getElementById("judge-score").textContent = data.score;
      document.getElementById("judge-accuracy").textContent = `${Math.round(data.accuracy * 100)}%`;
      break;
    case "RecentInputEvents":
      renderRecentInputs(data.events);
      break;
    default:
      break;
  }
});

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

document.getElementById("btn-load-midi").addEventListener("click", () => {
  const path = document.getElementById("midi-path").value.trim();
  if (!path) return;
  sendCommand({
    type: "LoadScore",
    payload: { source: { type: "MidiFile", payload: path } },
  });
});

document.getElementById("btn-browse-midi").addEventListener("click", async () => {
  const file = await pickFile({
    title: "Select MIDI file",
    filters: [{ name: "MIDI", extensions: ["mid", "midi"] }],
  });
  if (file) {
    document.getElementById("midi-path").value = file;
  }
});

// Settings controls

document.getElementById("btn-refresh-audio").addEventListener("click", () => {
  sendCommand({ type: "ListAudioOutputs" });
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

// Initialize
sendCommand({ type: "ListAudioOutputs" });
sendCommand({ type: "ListMidiInputs" });
