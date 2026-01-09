文档 2：ScoreDomain v1 + 导入规则（MIDI/MusicXML）（v0.1）
1. 目标

定义一个练习/判定友好的内部模型（不是排版模型）

支持输入源：MIDI、MusicXML（子集）、后续 PDF/OMR

提供稳定的 TargetEvent 序列给 Judge & Playback

2. 内部模型（v1）
2.1 核心结构

Score { meta, ppq, tempo_map, tracks }

Track { id, name, hand: Option<Hand>, targets: Vec<TargetEvent>, playback_events: Vec<PlaybackMidiEvent> }

v1 允许 targets 与 playback_events 分离：

Judge 用 targets（只关心 NoteOn 目标）

Autopilot 用 playback_events（包含 NoteOn/Off/CC64 等）

2.2 TargetEvent（给 Judge/高亮）

id: u64

tick: Tick

notes: Vec<u8>（去重、排序）

hand: Option<Hand>（Left/Right/None）

（可选）measure_index: u32（为 UI/loop 方便）

2.3 PlaybackMidiEvent（给 Scheduler）

tick: Tick

event: MidiLikeEvent（NoteOn/Off/Cc64）

bus_route_hint（可选：用于伴奏模式按手/轨路由）
3. MIDI 导入规则（v0.1）
3.1 解析与时间

读取 MIDI 文件 PPQ（ticks per quarter）

统一到内部 score.ppq = midi.ppq（v0.1 不重采样 PPQ，减少误差）

tempo：

若有 SetTempo meta：生成 TempoPoint

若无：默认 120 BPM

3.2 NoteOn/Off 规范化

velocity=0 的 NoteOn 视为 NoteOff（MIDI 常见）

多轨：

v0.1 默认合并为一个“演奏轨”（Track0）

可选：按 MIDI channel 拆轨（以后做伴奏会更方便）

3.3 TargetEvent 生成（关键）

从所有 NoteOn 事件生成目标：

同 tick 的 NoteOn 合并为一个 TargetEvent（和弦）

合并窗口（防浮点/微小偏差）：

v0.1：严格同 tick 合并

v0.2：可加入量化/吸附（例如 ±1 tick）

目标 notes 去重、排序

id 生成：建议单调递增（或 hash(tick+notes)）

3.4 CC64（踏板）

CC64 事件放入 playback_events

v0.1 不把 CC64 放入 TargetEvent（不影响命中判定）

伴奏模式下，CC64 默认只作用于 Autopilot bus（避免影响用户监听）

3.5 后处理（v0.1 最小集合）

RemoveImpossibleChords（极少见：tick 内重复 note）

HandSplit（可选、先弱规则）：

小于某阈值（如 MIDI note < 60）偏 Left，>=60 偏 Right

同 tick 跨度很大时拆到两手

VelocitySimplify（用于出声）：velocity clamp 到合理范围（例如 30–100）

4. MusicXML 导入子集（v0.1）
支持（Must）

pitch（step/alter/octave）→ MIDI note

duration（按 divisions）→ tick（映射到 score.ppq）

chord（<chord/>）→ 同 tick 合并

staff / part：用于 hand 推断（钢琴两谱表）

降级策略（遇到不支持）

装饰音/连音线：忽略对目标结构的影响（只保留基本音符）

多声部复杂对齐：以 start tick 为准，持续时间仅用于 playback NoteOff（可粗略）

5. 版本化与可编辑性（减少技术债）

ScoreFile（内部持久化 JSON）包含：

schema_version

source（midi/musicxml/pdf）

score（核心数据）

edit_log（可选：记录用户校正操作）

UI 校正只修改 targets（以及必要的 playback_events），不要回写到原始 MusicXML（避免变成编辑器）

6. 验收标准

导入 MIDI 后：

同时音符正确合并为和弦 TargetEvent

tempo/loop/seek 不出错

导入 MusicXML（简单钢琴谱）后：

左右手大体合理（允许后续手动修正）

可进入练习闭环