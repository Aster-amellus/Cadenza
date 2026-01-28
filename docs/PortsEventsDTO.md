- **Cadenza《系统架构与接口规格（Ports / Events / DTO）》v0.1**

---

# Status (implementation vs spec)

This document started as a v0.1 design spec. The implementation has evolved.
For the current source of truth, see:

- Ports (traits and shared types): `crates/cadenza-ports/src/`
- UI <-> Core IPC (commands/events): `crates/cadenza-core/src/ipc.rs`
- Architecture overview: `docs/ARCHITECTURE.md`

Notes:

- Audio volume controls are implemented in core (`AudioParams`) rather than as methods on `AudioOutputPort`.
- PDF -> MIDI is currently executed by the Tauri shell (background job) to keep AppCore responsive.
- The UI score view event now includes pedal spans for visualization.

# 0. 范围与原则

## 范围

* 定义 **模块边界**
* 定义 **Ports（trait 级接口）**
* 定义 **事件模型（Events）**
* 定义 **UI↔Core 的 DTO 与 IPC 契约**

## 关键原则

* **Domain 不依赖平台/音频/MIDI/Tauri**
* 所有外部 I/O 都经由 **Ports**
* 音频线程 **realtime-safe**（不做阻塞 IO / 不拿大锁 / 不频繁分配）
* 时间基准统一：**Transport 是唯一真相**（播放、UI 光标、判定对齐它）

---

# 1. 顶层架构与模块职责

## 1.1 分层

* **Presentation（Tauri UI）**

  * 发送 Command（请求/控制）
  * 订阅 Event（状态/反馈/进度）
* **Application（app_core）**

  * 用例编排：加载曲目、选择设备、启动练习、路由/混音、播放控制
  * 维护 Session 状态机（Idle/Ready/Running/Paused）
* **Domain**

  * `score-domain`：内部谱面模型（面向练习/判定）
  * `eval-domain`：判定与评分（纯逻辑）
* **Infrastructure**

  * MIDI 输入实现、音频输出实现、SF2 合成器实现、OMR 管线实现、存储实现

---

# 2. 时间模型（必须先定死）

为避免漂移与跨平台差异，定义两类时间：

1. **Musical Time（逻辑时间）**：`Tick`

* 用于谱面、判定窗口、节拍/速度变换
* `Tick` 是整数（例如 PPQ=480 或 960）

2. **Audio Time（物理时间）**：`SampleTime`

* 用于音频回调、事件调度、无抖动播放
* `SampleTime` 是整数样本计数（采样率下的 sample index）

> Transport 提供 `tick ↔ sample_time` 映射（依赖 tempo map + sample_rate）。

---

# 3. Ports（Core 对外能力接口）

> 这些 Ports 由 `app_core` 依赖；具体实现放在 `infra_*`。

## 3.1 MIDI 输入：`MidiInputPort`

职责：枚举、打开输入流、输出标准化事件（含 CC64）。

**最小接口**

* `list_inputs() -> Vec<MidiInputDevice>`
* `open_input(device_id) -> MidiInputStream`
* `MidiInputStream` 通过 callback / channel 产出 `PlayerEvent`

## 3.2 音频输出：`AudioOutputPort`

职责：枚举输出设备、打开音频流（回调填充 PCM）。

* `list_outputs() -> Vec<AudioOutputDevice>`
* `open_output(device_id, config, render_callback) -> AudioStreamHandle`

Volume is controlled in core (master + per-bus) and applied when mixing rendered buffers.

**AudioConfig**

* sample_rate
* channels（固定 2）
* buffer_size_frames（可选：Fixed/Default）

## 3.3 软件合成器：`SynthPort`

职责：加载音源（SF2）、接收 MIDI-like 事件、在音频线程渲染 PCM。

* `load_soundfont(path|bytes) -> SoundFontInfo`
* `set_program(bus, program)`（GM program，默认钢琴）
* `handle_event(bus, event, sample_time)`（NoteOn/Off/CC64…）
* `render(bus, frames, out_l, out_r)`（realtime-safe）

> sustain（CC64）必须按 **bus 独立维护状态**，避免 autopilot 踏板影响用户监听。

## 3.4 播放与调度：`PlaybackPort`

职责：Transport 状态、seek/loop/tempo、把谱面转成事件并调度到音频线程消费的队列。

* `load_score(score)`
* `play()/pause()/stop()`
* `seek(tick)`
* `set_loop(range: Option<LoopRange>)`
* `set_tempo_multiplier(x: f32)`
* `set_mode(mode: PlaybackMode)`（Demo/Accompaniment）
* `poll_playback_events(window: SampleWindow) -> Vec<ScheduledEvent>`（供音频回调消费或由内部队列推送）

## 3.5 存储：`StoragePort`（可选，先预留）

* `load_settings()/save_settings()`
* `save_recent_files()`

## 3.6 OMR：`OmrPort`（阶段性可为空实现）

* `recognize_pdf(pdf_path|bytes, options) -> OmrResult`（MusicXML 或中间表示）
* `get_diagnostics()`

---

# 4. Bus / 路由模型（默认应用内出声）

定义三条总线（bus）：

* `UserMonitor`：用户按键监听出声（**默认 On，可一键 Off**）
* `Autopilot`：示范/伴奏出声（默认 On）
* `MetronomeFx`：节拍器/提示音

**路由规则**

* 用户 MIDI 输入事件：

  * 永远送给 **Judge**（判定/可视化）
  * 当 `monitor_enabled=true` 时，送给 `SynthPort(UserMonitor)`
* Autopilot 事件：

  * 送给 `SynthPort(Autopilot)`
* 监听一键关闭：

  * 只影响 “用户事件→UserMonitor 出声”，不影响判定

---

# 5. 事件模型（Core 内部统一事件）

## 5.1 PlayerEvent（来自 MIDI 输入）

* `source = User`
* `timestamp`：尽量使用单调时间/到达时刻（用于校准），最终会被映射到 Transport tick/sample_time

事件类型：

* `NoteOn { note: u8, velocity: u8 }`
* `NoteOff { note: u8 }`
* `CC64 { value: u8 }`  // >=64 down, <64 up

## 5.2 PlaybackEvent（来自 Autopilot/节拍器）

* `source = Autopilot | Metronome`
* 通常由 Score/Transport 生成

## 5.3 ScheduledEvent（给音频线程消费）

* 必带 `sample_time: u64`
* `bus: Bus`
* `event: MidiLikeEvent`

## 5.4 JudgeEvent（判定输出）

* `Hit { target_id, delta_tick, grade }`
* `Miss { target_id, reason }`
* `State { combo, score, accuracy }`

> v1 建议：判定主要基于 NoteOn；CC64 不影响命中，仅影响出声与可视化。

## 5.5 State / Transport / Device Events

* `TransportPosition { tick, sample_time }`
* `SessionStateChanged { state }`
* `DevicesChanged { midi_inputs, audio_outputs }`
* `AudioStatus { xruns, buffer_size, sample_rate }`

---

# 6. UI ↔ Core IPC：Commands / Events / DTO

约定：

* 所有消息带 `schema_version`
* Command 支持 request/response（带 `request_id`）
* Event 为 pub-sub（不需要 request_id）

## 6.1 DTO：共用基础字段

```json
{
  "schema_version": "0.1",
  "request_id": "uuid-optional",
  "type": "..."
}
```

---

## 6.2 Commands（UI → Core）

### 设备与设置

* `ListMidiInputs`
* `SelectMidiInput { device_id }`
* `ListAudioOutputs`
* `SelectAudioOutput { device_id, config? }`
* `TestAudio` (plays a short note on the monitor bus)
* `SetMonitorEnabled { enabled: bool }`  ✅一键关闭监听
* `SetBusVolume { bus, volume_0_1 }`
* `SetMasterVolume { volume_0_1 }`

### 音源

* `LoadSoundFont { path | bytes }`
* `SetProgram { bus, gm_program }`

### 曲目/谱面

* `LoadScore { source }`

  * `source = MidiFile(path|bytes) | MusicXmlFile(path|bytes) | InternalDemo(id)`
* `SetPracticeRange { range }`（按 tick 或按小节映射后置）

### 练习与播放（Transport）

* `StartPractice`
* `PausePractice`
* `StopPractice`
* `Seek { tick }`
* `SetLoop { enabled, start_tick, end_tick }`
* `SetTempoMultiplier { x }`
* `SetPlaybackMode { mode }`（Demo / Accompaniment）
* `SetAccompanimentRoute { play_left: bool, play_right: bool }`

### 校准（建议尽早支持）

* `SetInputOffsetMs { ms }`（手动）
* `RunLatencyCalibration { kind }`（可选：引导式）

---

## 6.3 Events（Core → UI）

### 设备与状态

* `MidiInputsUpdated { devices[] }`
* `AudioOutputsUpdated { devices[] }`
* `SessionStateUpdated { state, selected_devices, settings }`

### Transport

* `TransportUpdated { tick, sample_time, playing, tempo_multiplier, loop? }`

### 判定与反馈

* `JudgeFeedback { grade, delta_ms?, delta_tick?, expected_notes[], played_notes[] }`
* `ScoreSummaryUpdated { combo, score, accuracy }`

### 输入监控（可节流/采样）

* `RecentInputEvents { events[] }`（建议 UI 端仅用于调试面板）

### OMR（后续）

* `OmrProgress { page, total, stage }`
* `OmrDiagnostics { severity, message, page? }`

---

# 7. DTO 结构定义（建议的字段）

## 7.1 设备 DTO

```json
{
  "MidiInputDevice": { "id": "string", "name": "string", "is_available": true },
  "AudioOutputDevice": { "id": "string", "name": "string", "default_config": { "sample_rate": 48000, "buffer_size_frames": 256 } }
}
```

## 7.2 Bus 与音量 DTO

```json
{
  "Bus": "UserMonitor | Autopilot | MetronomeFx",
  "Volumes": { "master": 0.8, "user_monitor": 0.8, "autopilot": 0.8, "metronome": 0.6 }
}
```

## 7.3 谱面（Score）对 UI 的最小视图模型（避免暴露 Domain 细节）

```json
{
  "ScoreView": {
    "title": "string",
    "duration_ticks": 123456,
    "ppq": 480,
    "hands": true,
    "targets_preview": [
      { "id": "t1", "tick": 960, "notes": [60, 64, 67], "hand": "Right" }
    ]
  }
}
```

## 7.4 判定反馈

```json
{
  "JudgeFeedback": {
    "target_id": "t1",
    "grade": "Perfect|Good|Miss",
    "delta_ms": -12,
    "expected_notes": [60,64,67],
    "played_notes": [60,64,67]
  }
}
```

---

# 8. 状态机（SessionState）与线程模型（实现约束）

## 8.1 SessionState（最小）

* `Idle`：未加载谱面
* `Ready`：已加载谱面/设备可用
* `Running`
* `Paused`

关键不变量：

* `Running` 必须有：score + transport + audio stream（或可降级为无声模式）

## 8.2 线程/任务建议（实现层约束）

* **Audio Thread（回调）**：只做

  * 消费 scheduled events（无阻塞）
  * synth render
  * mixing
* **Core Runtime（tokio 或单线程 loop）**：

  * 收 UI commands
  * 收 MIDI input events
  * 跑 Judge
  * 生成/调度 autopilot events（写入队列）

---

# 9. CC64（踏板）在接口层的明确约束

* 只要 MIDI 输入端收到 CC64，就必须标准化为 `PlayerEvent::CC64{value}`
* `SynthPort` 必须实现 sustain 行为（按 bus 独立）
* `Judge` v1 不依赖 CC64 做命中判定（后续可扩展为 sustain-related scoring）

---

# 10. 版本化与兼容性

* `schema_version`：字符串（例如 “0.1”）
* DTO 新增字段应保持向后兼容（可选字段）
* Event 类型新增不破坏旧 UI（UI 忽略未知 type）

---
