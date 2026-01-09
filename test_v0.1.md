文档 5：测试与回归资产规范（v0.1）（减少未来返工）
1. Golden Tests（必须）

Judge：给定 targets + note_on/advance_to 序列，断言输出事件序列

Import：MIDI/MusicXML → TargetEvent 序列 hash/快照对比

Transport：seek/loop/tempo 切换后，tick/sample 映射与事件顺序稳定

2. 样例数据集（建议 repo 内维护）

assets/test_midi/：简单音阶、和弦、左右手、带 tempo change

assets/test_musicxml/：简单钢琴两谱表

（后续）assets/test_pdf/：1–3 页印刷谱（用于 OMR 回归）