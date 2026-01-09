# A) Ports：Rust trait 规格（`cadenza-ports`）

> 设计目标：
>
> * UI/Domain 不碰平台细节
> * 可 mock（单测）
> * MIDI 输入、音频输出、合成器、存储都能替换实现
> * 音频线程 realtime-safe：回调里不做阻塞/重锁/IO

## A.1 基础类型

```rust
// cadenza-ports/src/types.rs
use std::{fmt, sync::Arc, time::Duration};

pub type Tick = i64;            // musical time, monotonic in score
pub type SampleTime = u64;      // audio sample index, monotonic while stream running

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DeviceId(pub String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Bus {
    UserMonitor,
    Autopilot,
    MetronomeFx,
}

#[derive(Clone, Debug)]
pub struct MidiInputDevice {
    pub id: DeviceId,
    pub name: String,
    pub is_available: bool,
}

#[derive(Clone, Debug)]
pub struct AudioOutputDevice {
    pub id: DeviceId,
    pub name: String,
    pub default_config: AudioConfig,
}

#[derive(Clone, Copy, Debug)]
pub struct AudioConfig {
    pub sample_rate_hz: u32,
    pub channels: u16,             // v1 固定 2
    pub buffer_size_frames: Option<u32>,
}

#[derive(Clone, Copy, Debug)]
pub struct Volume01(pub f32);      // clamp 0..=1 in setters
```

---

## A.2 MIDI 输入 Port

### 事件标准化（含 CC64）

```rust
// cadenza-ports/src/midi.rs
use super::types::*;
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MidiLikeEvent {
    NoteOn { note: u8, velocity: u8 },
    NoteOff { note: u8 },
    /// CC64: value 0..127. pedal_down = value >= 64
    Cc64 { value: u8 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventSource {
    User,
    Autopilot,
    Metronome,
}

/// 来自 MIDI In 的原始事件（尚未映射到 Tick）
/// app_core 负责根据 Transport + offset 将其映射为 Tick 再送给 Judge。
#[derive(Clone, Copy, Debug)]
pub struct PlayerEvent {
    pub at: Instant,         // 单调时钟时刻（到达/采样时刻）
    pub event: MidiLikeEvent,
}
```

### Trait 规格

```rust
use std::sync::Arc;

#[derive(thiserror::Error, Debug)]
pub enum MidiError {
    #[error("device not found: {0}")]
    DeviceNotFound(String),
    #[error("device unavailable: {0}")]
    DeviceUnavailable(String),
    #[error("backend error: {0}")]
    Backend(String),
}

/// MIDI 输入流句柄：drop 即关闭
pub trait MidiInputStream: Send {
    fn close(self: Box<Self>);
}

pub type PlayerEventCallback = Arc<dyn Fn(PlayerEvent) + Send + Sync + 'static>;

pub trait MidiInputPort: Send + Sync {
    fn list_inputs(&self) -> Result<Vec<MidiInputDevice>, MidiError>;

    /// 打开输入流：实现方应在后台线程/回调中不断触发 cb。
    /// 要求：cb 调用必须非阻塞；若拥塞，建议内部丢弃或做有界队列。
    fn open_input(
        &self,
        device_id: &DeviceId,
        cb: PlayerEventCallback,
    ) -> Result<Box<dyn MidiInputStream>, MidiError>;
}
```

**实现约束**

* 回调线程不应做 UI 调用；只推事件到 app_core 的队列
* 不保证事件节奏均匀；Judge 不能假设固定采样间隔

---

## A.3 音频输出 Port（回调渲染）

音频输出只负责把 PCM 送到设备；**合成与混音由上层（Synth/Mixer）填充到 buffer**。

```rust
// cadenza-ports/src/audio.rs
use super::types::*;

#[derive(thiserror::Error, Debug)]
pub enum AudioError {
    #[error("device not found: {0}")]
    DeviceNotFound(String),
    #[error("device unavailable: {0}")]
    DeviceUnavailable(String),
    #[error("unsupported config: {0}")]
    UnsupportedConfig(String),
    #[error("backend error: {0}")]
    Backend(String),
}

/// 音频回调：必须 realtime-safe
/// out_l/out_r 长度 = frames
pub trait AudioRenderCallback: Send + Sync + 'static {
    fn render(&self, sample_time_start: SampleTime, out_l: &mut [f32], out_r: &mut [f32]);
}

pub trait AudioStreamHandle: Send {
    fn close(self: Box<Self>);
}

pub trait AudioOutputPort: Send + Sync {
    fn list_outputs(&self) -> Result<Vec<AudioOutputDevice>, AudioError>;

    fn open_output(
        &self,
        device_id: &DeviceId,
        config: AudioConfig,
        cb: Arc<dyn AudioRenderCallback>,
    ) -> Result<Box<dyn AudioStreamHandle>, AudioError>;
}
```

> v1 建议固定 `f32 stereo`，先别搞多格式，跨平台麻烦。

---

## A.4 合成器 Port（SF2 / 软件音源）

**注意**：`render()` 会在音频回调线程被调用；`load_soundfont()` 可能很重，不能在音频线程里做。

```rust
// cadenza-ports/src/synth.rs
use super::types::*;
use super::midi::MidiLikeEvent;

#[derive(thiserror::Error, Debug)]
pub enum SynthError {
    #[error("soundfont load failed: {0}")]
    SoundFontLoad(String),
    #[error("unsupported soundfont format")]
    UnsupportedFormat,
    #[error("backend error: {0}")]
    Backend(String),
}

#[derive(Clone, Debug)]
pub struct SoundFontInfo {
    pub name: String,
    pub preset_count: usize,
}

/// 线程模型：
/// - load_* / set_program 由 core 线程调用（可加内部锁）
/// - handle_event/render 由音频线程调用（必须 realtime-safe）
/// 最简单做法：
/// - core 线程更新“下一个配置”
/// - 音频线程在 buffer 边界原子切换配置
pub trait SynthPort: Send + Sync {
    fn load_soundfont_from_path(&self, path: &str) -> Result<SoundFontInfo, SynthError>;
    fn set_program(&self, bus: Bus, gm_program: u8) -> Result<(), SynthError>;

    /// 由音频线程调用：把事件注入合成器（按 bus 独立状态，含 CC64 sustain）
    fn handle_event(&self, bus: Bus, event: MidiLikeEvent, at: SampleTime);

    /// 由音频线程调用：渲染 frames 到 out_l/out_r（追加写入 or 覆盖写入需在实现里统一）
    fn render(&self, bus: Bus, frames: usize, out_l: &mut [f32], out_r: &mut [f32]);
}
```

---

## A.5 Mixer/Router（建议在 app_core 内，不做 Port）

Mixer 通常不需要换实现；做成 `app_core::audio_graph` 模块即可：

* 三个 bus 音量 + master
* monitor_enabled：控制 User 事件是否路由到 `Bus::UserMonitor`
* autopilot 永远路由到 `Bus::Autopilot`
* metronome 路由到 `Bus::MetronomeFx`

---

## A.6 Storage Port（可选，v1 可先 stub）

```rust
// cadenza-ports/src/storage.rs
use super::types::*;

#[derive(thiserror::Error, Debug)]
pub enum StorageError {
    #[error("io error: {0}")]
    Io(String),
    #[error("serialization error: {0}")]
    Serde(String),
}

#[derive(Clone, Debug, Default)]
pub struct SettingsDto {
    pub selected_midi_in: Option<DeviceId>,
    pub selected_audio_out: Option<DeviceId>,
    pub monitor_enabled: bool,
    pub master_volume: Volume01,
    pub bus_user_volume: Volume01,
    pub bus_autopilot_volume: Volume01,
    pub bus_metronome_volume: Volume01,
    pub input_offset_ms: i32,
    pub default_sf2_path: Option<String>,
}

pub trait StoragePort: Send + Sync {
    fn load_settings(&self) -> Result<SettingsDto, StorageError>;
    fn save_settings(&self, s: &SettingsDto) -> Result<(), StorageError>;
}
```

---

# C) Judge v1：输入/输出契约 + 判定规则（`cadenza-domain-eval`）

> 目标：先做一个“手感稳定、可回归”的 v1：
>
> * 以 **NoteOn** 为判定核心
> * 支持单音/和弦
> * 统计错音/漏音/多按
> * 推进策略清晰（event-driven）
> * CC64 **不影响命中判定**（只影响出声/可视化）