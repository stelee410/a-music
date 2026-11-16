# 第三方组件

本仓库的代码按 [MIT 许可](LICENSE) 发布。下面的组件属于它们各自的作者，随引擎一起分发时保留各自的许可。

| 组件 | 用在哪里 | 作者与许可 |
|------|----------|------------|
| [CHIMERA](https://github.com/jamesondh/chimera-vocal-synth) 物理建模歌声引擎 | 歌声轨 `voice.chimera`（`crates/agb-voice`），构建时按固定版本从上游取源码 | © 2026 Jameson Hodge，MIT，全文见 [`crates/agb-voice/data/CHIMERA-LICENSE.txt`](crates/agb-voice/data/CHIMERA-LICENSE.txt) |
| CMU Pronouncing Dictionary | 英文歌词的读音（`crates/agb-voice/data/cmudict.txt.gz`） | © 1993–2015 Carnegie Mellon University，BSD 式许可，全文见 [`crates/agb-voice/data/CMUDICT-LICENSE.txt`](crates/agb-voice/data/CMUDICT-LICENSE.txt) |
| [vst3](https://crates.io/crates/vst3) VST 3 接口绑定 | 插件宿主（`crates/agb-plugin`） | MIT 或 Apache-2.0 |
