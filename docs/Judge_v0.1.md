## C.1 Judge 的输入假设（非常重要）

Judge **不负责**：

* MIDI 设备时间戳校准
* tick↔ms 换算
* tempo map 细节

Judge **只接收**：

* 已经映射到 `Tick` 的玩家输入事件（主要 NoteOn）
* 目标谱面 `TargetEvent` 序列（按 tick 升序）

> 即：app_core 做 `PlayerEvent(Instant)` → `PlayerNoteEvent{ tick }` 的映射（含 input_offset）。

---

## C.2 Score 侧给 Judge 的最小数据结构

```rust
// cadenza-domain-eval/src/score_contract.rs
use cadenza_ports::types::Tick;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hand {
    Left,
    Right,
}

#[derive(Clone, Debug)]
pub struct TargetEvent {
    pub id: u64,
    pub tick: Tick,
    pub notes: Vec<u8>,          // unique MIDI notes, sorted ascending
    pub hand: Option<Hand>,      // optional
}
```

约束：

* `TargetEvent` 必须按 `tick` 非递减排序
* 同 tick 多个事件：v1 建议在上游合并成一个和弦事件（避免判定复杂）

---

## C.3 玩家输入给 Judge 的结构

```rust
use cadenza_ports::types::Tick;

#[derive(Clone, Copy, Debug)]
pub struct PlayerNoteOn {
    pub tick: Tick,
    pub note: u8,
    pub velocity: u8, // v1 不用于评分
}
```

v1：Judge **只消费 NoteOn**（NoteOff/CC64 仅做统计或忽略）。

---

## C.4 配置：Timing Window / Chord Roll / 错音策略

```rust
#[derive(Clone, Copy, Debug)]
pub struct TimingWindowTicks {
    pub perfect: i64,   // abs(delta_tick) <= perfect
    pub good: i64,      // abs(delta_tick) <= good  (good >= perfect)
}

/// 和弦滚动容忍：同一个 TargetEvent 的多个音符允许分散在此范围内（ticks）
/// 例如允许手指滚奏，不要求完全同一 tick。
#[derive(Clone, Copy, Debug)]
pub struct ChordRollTicks(pub i64);

#[derive(Clone, Copy, Debug)]
pub enum WrongNotePolicy {
    /// 记录为 mistake，但不阻止该 TargetEvent 仍然命中
    RecordOnly,
    /// 若在判定窗口内出现“非目标音”，则该 TargetEvent 最多只能 Good（Perfect 降级）
    DegradePerfect,
}

#[derive(Clone, Copy, Debug)]
pub enum AdvanceMode {
    /// 命中（完成）或超时 Miss 才推进到下一个 target（推荐）
    OnResolve,
    /// 收到任何 NoteOn 都可能触发推进（不推荐 v1）
    Aggressive,
}

#[derive(Clone, Copy, Debug)]
pub struct JudgeConfig {
    pub window: TimingWindowTicks,
    pub chord_roll: ChordRollTicks,
    pub wrong_note_policy: WrongNotePolicy,
    pub advance: AdvanceMode,
}
```

---

## C.5 Judge 的输出事件（给 app_core/UI）

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grade { Perfect, Good, Miss }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissReason {
    Timeout,            // 过了 good window 仍未完成目标
    Skipped,            // 由于推进导致直接跳过（v1 可不启用）
}

#[derive(Clone, Debug)]
pub enum JudgeEvent {
    /// 当前 target 发生变化（用于 UI 高亮）
    FocusChanged { target_id: Option<u64> },

    /// 某个 target 被完全命中
    Hit {
        target_id: u64,
        grade: Grade,           // Perfect/Good
        delta_tick: i64,        // 以“命中代表音”（见下）计算的偏差
        wrong_notes: u32,
    },

    /// 某个 target 失败
    Miss {
        target_id: u64,
        reason: MissReason,
        missing_notes: u32,
        wrong_notes: u32,
    },

    /// 统计更新（可选：也可由 app_core 聚合）
    Stats {
        combo: u32,
        score: i64,
        hit: u32,
        miss: u32,
        wrong: u32,
    },
}
```

---

## C.6 判定状态与核心算法（v1 规则）

### C.6.1 内部状态（针对当前 focus target）

对当前 `TargetEvent`（focus）维护：

* `expected: HashSet<u8>`（目标音集合）
* `matched: HashMap<u8, Tick>`（已匹配的目标音 → 实际 NoteOn tick）
* `wrong_notes: u32`（窗口内出现的非目标音数量）
* `first_match_tick: Option<Tick>`（该 target 第一次匹配到目标音的 tick）

### C.6.2 匹配窗口

定义 target 的允许窗口（以 target.tick 为中心）：

* Perfect：`abs(player_tick - target.tick) <= perfect_ticks`
* Good：`abs(player_tick - target.tick) <= good_ticks`

**窗口外 NoteOn 如何处理？**

* 若 `player_tick < target.tick - good`：视为“过早”，v1 建议 **不消费**（不计错，不推进），让用户继续弹
* 若 `player_tick > target.tick + good`：说明当前 target 已超时，应先触发 `Timeout Miss`，推进到下一个 target，再重新尝试匹配（见 C.6.4）

### C.6.3 和弦（Chord）滚动容忍

一个 target 可能包含多个音（和弦）。允许用户在 `chord_roll` 范围内“滚奏”：

* 当第一个目标音被匹配时，记录 `first_match_tick`
* 之后同一 target 的其它目标音，只要满足：

  * 在 Good 窗口内：`abs(t - target.tick) <= good`
  * 且相对第一音：`abs(t - first_match_tick) <= chord_roll`
    即认为属于同一和弦尝试

> 这样既能容忍滚奏，也能防止用户把别的小节的音误算到同一和弦里。

### C.6.4 推进与 Miss 的产生（必须靠“时间推进”触发）

Judge 需要一个“时间推进”入口，否则用户不弹时无法产生 Miss：

* `advance_to(now_tick)`：由 app_core 以 Transport 光标驱动（比如每帧或固定间隔）
* 当 `now_tick > target.tick + good_ticks` 且当前 target 未完成：

  * 触发 `Miss { reason: Timeout }`
  * 统计 missing_notes = expected - matched
  * 推进 focus 到下一个 target，并发出 `FocusChanged`

### C.6.5 NoteOn 处理逻辑（v1）

当收到 `PlayerNoteOn { tick, note }`：

1. 如果没有当前 target：忽略（或等待加载）

2. 先调用 `advance_to(tick)`，确保超时 target 被结算

3. 再尝试用该 NoteOn 匹配“当前 target”：

   * 若 `note ∈ expected` 且该 note 尚未 matched，并且满足窗口条件（Good）：

     * 记录 matched[note] = tick
     * 若 first_match_tick 为空，设为 tick
   * 否则如果该 NoteOn 落在 Good 窗口内但 `note ∉ expected`：

     * wrong_notes += 1（并按 wrong_note_policy 影响 grade 上限）
   * 否则：窗口外（过早/过晚）按 C.6.2 处理（通常不计错）

4. 判断 target 是否“完成”（matched 覆盖 expected）：

   * 若完成：

     * 计算该 target 的 `delta_tick`：用 **第一目标音** 或 **平均** 作为代表（v1 推荐“第一目标音”更符合手感）

       * `delta = first_match_tick - target.tick`
     * 计算 grade：

       * 如果 `abs(delta) <= perfect` 且未触发降级条件 ⇒ Perfect
       * 否则若 `abs(delta) <= good` ⇒ Good
     * 若 wrong_note_policy = DegradePerfect 且 wrong_notes > 0，则 Perfect 降为 Good
     * 触发 `Hit`，combo++，推进 focus

> v1 的策略是“以完成时刻（第一音）给分”，滚奏不会因为最后一个音略晚而被强行降级。

---

## C.7 评分与统计（v1 推荐默认）

* Perfect：+100（或 1.0 accuracy）
* Good：+70
* Miss：+0，combo 清零
* wrong_notes：单独计数（可选扣分，但 v1 建议只做统计，避免手感变坏）

这些数值建议作为配置常量放 app_core（或 JudgeConfig 扩展），Judge 只产出 Grade 与 counts。

---

## C.8 Judge trait（建议的可测试接口）

```rust
pub struct Judge {
    cfg: JudgeConfig,
    targets: Vec<TargetEvent>,
    idx: usize, // current focus
    // ... current target state ...
}

impl Judge {
    pub fn new(cfg: JudgeConfig) -> Self { /* ... */ }

    pub fn load_targets(&mut self, targets: Vec<TargetEvent>) {
        self.targets = targets;
        self.idx = 0;
        // reset state, emit FocusChanged on first query if needed
    }

    /// 由 app_core 驱动：输入 NoteOn，返回可能产生的多个事件（Hit/Miss/Focus/Stats）
    pub fn on_note_on(&mut self, e: PlayerNoteOn) -> Vec<JudgeEvent> {
        // calls advance_to(e.tick), then match, then maybe resolve
        vec![]
    }

    /// 由 app_core 以 Transport 光标驱动：用于产生 Timeout Miss
    pub fn advance_to(&mut self, now_tick: Tick) -> Vec<JudgeEvent> {
        vec![]
    }

    pub fn current_focus(&self) -> Option<u64> {
        self.targets.get(self.idx).map(|t| t.id)
    }
}
```

---