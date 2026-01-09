# **Cadenza 的 UI 界面规划 + UI 标注规范（可直接用于 Figma / 设计交付 / 前端实现）**

## 1) UI 信息架构（IA）

### 顶层导航（建议左侧栏或顶部 Tab）

* **练习 Practice**（核心：演奏/判定/回放）
* **曲库 Library**（最近/收藏/导入）
* **导入 Import**（MIDI/MusicXML/PDF）
* **校准 Calibration**（输入延迟、音频缓冲）
* **设置 Settings**（音频/音源/MIDI/快捷键/诊断）
* **诊断 Diagnostics**（日志、设备状态、导出诊断包）

> MVP 允许先把 Library/Import 合并，Settings 里先放最关键的音频路由。

---

## 2) 关键页面与布局规范（页面级规格）

### 2.1 首次启动向导（Onboarding / Setup）

**目标**：5 分钟内让用户“能听到声音 + 能看到输入 + 能开始练”。

**布局块**

1. 音频输出设备选择（下拉）
2. 音源选择（默认内置简易音色 + 选择 SF2）
3. MIDI 输入设备选择（下拉）
4. **监听我的演奏出声**（默认 On，旁边提示“若双声请关闭/或设置 Local Control Off”）
5. 测试区：按键显示 + 出声
6. 下一步：进入 Practice

**必须状态**

* 设备不可用（灰显 + 提示）
* SF2 加载失败（错误 toast + “选择其他文件”）
* 无 MIDI 输入也允许继续（进入 demo 模式，只播放 autopilot）

---

### 2.2 练习页（Practice）— 核心屏

建议结构：**三段式**（信息密度高但不拥挤）

**A. 顶部状态栏（固定高度）**

* 当前曲名 / 段落
* 设备状态（MIDI In / Audio Out 小点灯）
* 监听开关（Monitor On/Off）
* 快捷入口：Mixer、Loop、Tempo

**B. 主区域（左右分栏）**

* **左：谱面视图**（MVP 推荐 piano-roll + 键盘高亮）

  * 目标音符（Expected）
  * 当前窗口（Now）
  * 播放光标（Transport）
  * 错误标记（Miss/错音）
* **右：反馈面板**

  * 当前判定（Perfect/Good/Miss）
  * 误差（ms 或 tick）
  * 组合数 Combo / 分数 Score / 命中率
  * 最近输入事件（可折叠，调试用）

**C. 底部 Transport 控制条（固定）**

* Play/Pause/Stop
* Seek（时间轴/小节）
* Tempo 倍率（0.5/0.8/1.0）
* Loop 开关 + 范围
* Metronome 开关 + 音量
* Autopilot 模式（Demo / Accompaniment：左手/右手播放）

**必须交互**

* 单击谱面可设置 loop 起点/终点（或拖拽选择范围）
* “只听伴奏 / 只听自己 / 都听”在 Mixer 里一键切换

---

### 2.3 Mixer 弹窗（音量路由）

**目标**：解决“默认应用内出声 + 双声问题 + 伴奏练习”三件事。

**UI 内容**

* Master Volume
* **User Monitor Volume**（监听音量）+ **监听开关（总开关）**
* Autopilot Volume
* Metronome/FX Volume
* 预设按钮：

  * 「合奏」= 监听 On + Autopilot On
  * 「只听伴奏」= 监听 Off + Autopilot On
  * 「只听自己」= 监听 On + Autopilot Off

---

### 2.4 导入页（Import）

三张卡片入口：

* 导入 **MIDI**
* 导入 **MusicXML**
* 导入 **PDF**（OMR）

PDF 导入流程（MVP 可保守）：

1. 选择 PDF
2. 预览页缩略图
3. 开始转换（进度条：页码/阶段）
4. 转换结果（成功进入 Practice；失败显示诊断与重试）

---

### 2.5 PDF 校正页（后续里程碑）

> 不是五线谱编辑器：是“可练习目标编辑器”。

**视图**

* 上：PDF 页预览（只做参考 overlay）
* 下：piano-roll 目标轨
* 工具：删除/合并/拆分/左右手切换/时间微调/小节拉伸

---

### 2.6 校准页（Calibration）

* 输入偏移（Input Offset ms）滑条 + “敲击校准”
* 音频 buffer size 选择（低延迟/稳定）
* 输出延迟自测（可选）
* 实时延迟/抖动读数（高级折叠）

---

### 2.7 设置页（Settings）

**分组**

* Audio：输出设备、buffer、主音量、SF2 管理、默认音色
* MIDI：输入设备、踏板显示（CC64 状态）、输入过滤（可选）
* UI：快捷键、主题（可后置）
* Diagnostics：日志级别、导出诊断包

---

## 3) 组件库与命名（Design System）

### 3.1 栅格与尺寸

* **8pt grid**（间距与尺寸都按 8 的倍数）
* 基准窗口：

  * macOS：**1200×800** 起（可缩放）
  * 最小可用：**960×640**
* 主内容区最小宽度：谱面视图 ≥ 640px

### 3.2 字体与层级（建议）

* Title 18–20
* Section 14–16
* Body 12–14
* Mono（调试数据）12

### 3.3 颜色语义（token）

* `--fg`, `--bg`, `--muted`
* `--success`（Perfect）
* `--warning`（Good）
* `--danger`（Miss/错误）
* `--accent`（光标/选区）

> 不在规范里写具体 RGB，写 token，便于换主题。

### 3.4 组件命名规范（Figma / 代码统一）

* Screen：`scr_practice`, `scr_import`, `scr_settings_audio`
* Component：`cmp_transport_bar`, `cmp_mixer_modal`, `cmp_device_selector`
* State：`--default / --disabled / --error / --loading / --active`
* 组合：`cmp_button--primary--disabled`

---

## 4) UI 标注规范（重点：怎么“标”才不扯皮）

建议你们统一用 **“规格卡（Spec Card）+ Redline + 状态机”** 三件套。

### 4.1 每个页面必须交付的标注内容

1. **Frame 信息**

   * 画板尺寸（如 1200×800）
   * 响应式规则（哪些区域可伸缩，最小宽高）
2. **布局 Redline**

   * 关键间距（padding/margin）
   * 对齐规则（左对齐/居中/基线）
3. **交互说明**

   * 可点击区域、hover、drag、快捷键
   * 动效是否必需（MVP 可不强制）
4. **状态枚举**

   * Loading / Empty / Error / Disabled
   * 设备断开、音源加载失败、无曲目等
5. **与 Core 的事件契约**

   * 该控件触发什么 Command
   * 依赖什么 Event 更新 UI

### 4.2 组件级 Spec Card 模板（建议照抄）

每个关键组件（Transport/Mixer/DeviceSelector/ScoreView）写一张卡：

* **Name / ID**：`cmp_transport_bar`
* **Purpose**：控制播放、速度、loop
* **Props（UI 输入）**：playing、tempoMultiplier、loopRange
* **Events（UI 输出）**：点击 Play→`StartPractice`
* **States**：default/disabled（未加载曲目）/loading（切曲中）
* **Edge Cases**：loop 起点>终点如何处理、seek 超界如何表现
* **A11y**：Tab 顺序、aria-label、快捷键（Space=Play/Pause）

### 4.3 标注符号与写法（团队统一）

* 尺寸：`W/H`，间距：`P/M`（如 `P16`）
* 圆角：`R8`，阴影：`S1`（用 token 级别）
* 字体：`T14/Regular`（字号/字重）
* 颜色：写 token：`--success` 不写 RGB
* 交互：`OnClick`, `OnDrag`, `OnHover`, `OnKey`

---

## 5) UI ↔ Core IPC 对齐表（你们实现时直接对）

下面列几个最关键控件与命令/事件映射（MVP 必用）：

| UI 控件           | Command（UI→Core）                                   | Event（Core→UI）                         |
| --------------- | -------------------------------------------------- | -------------------------------------- |
| MIDI 设备下拉       | `SelectMidiInput{device_id}`                       | `MidiInputsUpdated`                    |
| 音频输出下拉          | `SelectAudioOutput{...}`                           | `AudioOutputsUpdated`                  |
| 监听开关            | `SetMonitorEnabled{enabled}`                       | `SessionStateUpdated`                  |
| Master/Bus 音量   | `SetMasterVolume` / `SetBusVolume`                 | `SessionStateUpdated`                  |
| Play/Pause/Stop | `StartPractice` / `PausePractice` / `StopPractice` | `TransportUpdated`                     |
| Tempo 倍率        | `SetTempoMultiplier{x}`                            | `TransportUpdated`                     |
| Loop 设置         | `SetLoop{...}`                                     | `TransportUpdated`                     |
| Autopilot 模式    | `SetPlaybackMode` / `SetAccompanimentRoute`        | `TransportUpdated`                     |
| 判定展示            | —                                                  | `JudgeFeedback`, `ScoreSummaryUpdated` |

---

## 6) UI 性能与节流规范（避免 UI 被事件淹没）

* `RecentInputEvents`：最多 20 条，**10–20Hz 节流**（调试面板才开）
* `TransportUpdated`：建议 30Hz（足够流畅）
* `JudgeFeedback`：实时（但 UI 渲染要轻量）

---

## 7) 可访问性与快捷键（建议 MVP 就做基础）

* Space：Play/Pause
* L：Loop On/Off
* M：Metronome On/Off
* U：监听开关（User Monitor）
* +/-：速度倍率调整
* Tab 顺序：顶部栏 → 主区 → 底部栏 → 弹窗