文档 4：错误处理与诊断规范（v0.1）（强烈建议一起定）
1. 错误分级

Fatal：无法继续（例如音频系统初始化完全失败且无兜底）

Recoverable：可恢复（设备断开、SF2 丢失、OMR 失败）

Warning：不影响主流程（某些不支持的 MusicXML 特性被忽略）

2. UX 规则

Recoverable：用 toast + 状态栏红点；必要时提供“重试”按钮

设备断开：

MIDI：自动回到未连接状态，保留练习页面但暂停判定输入

Audio：尝试重开默认设备；失败则无声模式

OMR 失败：显示“哪页/哪步失败 + 导出诊断”

3. 诊断包（Diagnostics Bundle）

一键导出 zip，包含：

app_version.json、platform.json

settings.json（敏感路径可脱敏）

device_snapshot.json（MIDI/Audio 设备列表）

recent_events.json（环形缓冲：最近 N 条输入/transport/judge 事件）

logs.txt（tracing 输出）

这能极大降低跨平台调试成本，是最值钱的“减债工程”。