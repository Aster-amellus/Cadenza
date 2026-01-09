文档 1：Transport & Scheduler 设计说明（v0.1）
1. 目标

统一 播放(autopilot)、UI 光标、判定时间基准，避免漂移

支持：play/pause/stop/seek/loop/tempo倍率

音频稳定：lookahead 调度，不在音频回调里做重活

2. 时间模型

Tick (i64)：音乐逻辑时间（PPQ 基准）

SampleTime (u64)：音频物理时间（sample index，单调递增）

Transport 是唯一真相，提供映射：

tick_to_sample(tick) -> SampleTime

sample_to_tick(sample) -> Tick

3. TempoMap（v0.1）
v0.1 规则（先稳）

支持 常速 + 少量 tempo change（可选开关）

Tempo 以 “微秒/四分音符” 或 “BPM” 表示，内部统一为：

us_per_quarter: u32

tempo_multiplier: f32 应用于所有 tempo 段（整体变速）

数据结构建议

TempoPoint { tick: Tick, us_per_quarter: u32 } 按 tick 升序

预计算 tick→累计微秒 的分段前缀（避免每次 seek O(n)）

4. Transport 状态机

Stopped：位置归零或保持上次（建议保持上次，Stop=暂停+回到loop起点可选）

Playing

Paused

行为定义

play()：从当前 tick 继续

pause()：冻结 tick/sample（停止推进）

stop()：设置 tick 到 loop.start（若 loop enabled）否则 0（可配置）

seek(tick)：立即更新当前位置；触发：

清空 scheduler 队列

synth 做 “all notes off / reset pedal” （至少对 Autopilot bus）

set_loop(range)：

允许 None（关闭）

若开启且 end <= start：UI 侧防呆；core 侧再 clamp

set_tempo_multiplier(x)：立刻生效；需：

清空/重算 future queue（否则会时间错位）

5. Scheduler（lookahead）
关键参数（v0.1 推荐）

lookahead_ms = 30ms（可调 20–50ms）

schedule_quantum_ms = 5ms（core 每 5ms 推一次未来事件；或每 UI 帧/固定 tick）

队列模型

音频线程消费：ScheduledEvent { sample_time, bus, MidiLikeEvent }

队列要求：

有界（防爆内存）

近实时：生产者（core）写入、消费者（audio callback）读取

拥塞策略：宁可丢弃“过远未来事件”，也不要阻塞音频

实现建议：单生产者单消费者 ring buffer（或 lock-free queue）。先简单用 crossbeam/有界 channel 也行，但注意音频线程不能阻塞。

调度算法（核心）

core 定期读取 transport.now_sample() 与 now_tick()

计算 window_end_sample = now_sample + lookahead_samples

生成 window_end_tick = sample_to_tick(window_end_sample)

从 ScorePlaybackIterator 拉取 (tick, event)，映射为 sample_time 并 push 入队

Loop 处理

如果 next_tick >= loop.end：

触发一次 seek(loop.start) 的内部逻辑（但别发 UI 命令）

清空队列并从 loop.start 继续生成事件

要保证 loop 不漂移：

seek 后 transport 的 tick/sample 必须一致更新

映射使用同一套 tempo map + multiplier

6. 输入事件如何对齐 tick（给 Judge）

规则：Judge 接收的是已经映射到 Tick 的 PlayerNoteOn。

映射（v0.1）

player_instant → 转成 sample_time_est：

v0.1 直接用 “事件到达时刻” 相对音频启动时刻的 duration 推算（不完美但可用）

或更简单：把事件处理时的 transport.now_tick() 作为近似，再加 input_offset_tick

input_offset_ms（校准项）转成 tick：offset_tick = ms_to_tick(ms, current_tempo)

v0.2 可升级为：记录音频回调的 wallclock anchor，提高映射精度；但 v0.1 先让流程跑通且稳定。

7. 验收标准（防技术债）

Loop 连续播放 5 分钟，loop 点不漂移（误差不累计）

tempo 倍率切换后，播放不乱序、不爆音、UI 光标同步

seek 后不会出现“残留延音”（至少 Autopilot bus all-notes-off）

core 卡顿不会让音频回调阻塞（最多丢未来事件，不破音频线程）