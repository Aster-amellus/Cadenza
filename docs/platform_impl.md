文档 5：平台实现路线（Audio/MIDI Infra v0.1）

1. 目标

先保证跨平台“能出声 + 能读 MIDI”，再逐步做低延迟与高级功能。
MVP 选择成熟的跨平台库，平台差异由库内部承接，核心只面向 Ports。

2. 方案总览（v0.1）

- Audio 输出：`cpal`
  - macOS: CoreAudio
  - Linux: ALSA / JACK（按系统配置）
  - Windows: WASAPI
- MIDI 输入：`midir`
  - macOS: CoreMIDI
  - Linux: ALSA
  - Windows: WinMM

这套组合是纯 Rust、跨平台、维护成本低，适合 MVP 落地。

3. Infra Crates 规划

- `crates/cadenza-infra-audio-cpal`
  - 实现 `AudioOutputPort`
  - 统一为 `f32` stereo 输出
  - 支持设备枚举、打开输出流、回调渲染
- `crates/cadenza-infra-midi-midir`
  - 实现 `MidiInputPort`
  - 统一 NoteOn/NoteOff/CC64 事件
  - 设备枚举、打开输入流、回调推送 `PlayerEvent`

4. 后续可选路线（v0.2+）

- 更低延迟或高级功能时，可为单个平台引入专用后端：
  - macOS：coreaudio-rs + coremidi-rs
  - Windows：wasapi + winrt MIDI
  - Linux：ALSA rawmidi / JACK 专用实现
- Audio 方面可补充独占模式/固定 buffer size 优化。

5. 实施要点

- Audio 线程必须 realtime-safe（不做阻塞 IO、锁竞争、频繁分配）。
- MIDI 输入线程只做解析与入队，不做业务逻辑。
- 设备 ID 需要稳定（当前以枚举序号 + 设备名生成）。

6. 当前实现状态（repo）

- 已落地：`cadenza-infra-audio-cpal` / `cadenza-infra-midi-midir` / `cadenza-infra-synth-rustysynth`（可加载 SF2）。
- 现状限制：音频回调路径仍存在锁（`Mutex`）与少量动态分配，重压下可能出现爆音/卡顿；需要在 v0.1 稳定阶段消除。
- 设备 ID：目前基于枚举顺序 + 设备名，可能在系统重启或设备顺序变化时发生漂移；后续可考虑更稳定的标识方案。
