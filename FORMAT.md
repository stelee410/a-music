# AGB Song — 可演奏的音乐文件格式（FORMAT）

| 字段 | 值 |
|------|-----|
| 格式名 | `agbsong`（暂定名） |
| 格式版本 | `0.1.0`（草案） |
| 扩展名 | `.agbsong`：**一个文件**（zip 压缩包） |
| MIME | `application/vnd.agb.song+zip` |
| 语言 | 中文（标识符与 JSON 字段用英文） |
| 与 `SPEC.md` 的关系 | `SPEC.md` 规定引擎和事务协议；本文规定**交换用的文件格式**。两者共用 tick 时钟、音高记法、引擎 id、预置 id |

本文同时标注**参考实现（本仓库 `agb song …`）目前做到哪一步**，见 §14。规范里写了但播放器还不支持的部分，播放器必须明确报告，不能假装播放了。

---

## 0. 一句话

一个 `.agbsong` 文件是一首**还没有定稿成音频的音乐**：乐谱、音色、采样、效果、叠加频率、视觉和封面都以数据形式保存。任何遵循本格式的 AI Agent 都可以写出它；播放器在播放时合成声音，所以移调、变速、换效果器、叠加频率、让 AI 现场改编，都不需要重新"导出"。

## 1. 设计原则

1. **AI 能直接写**。入口是一个 JSON 文档，字段名完整、单位写在名字里（`durTicks`、`gainDb`、`cutoffHz`）。不要求作者会写二进制 MIDI。
2. **数据，不是程序**。核心格式里没有可执行代码。着色器是可选扩展，播放器可以不运行它。
3. **可降级**。播放器不认识的东西要么有声明好的替代（`fallback`），要么被跳过并报告；只有文件明确写进 `requires` 的特性缺失时才拒绝播放。
4. **可演奏**。文件声明哪些东西允许现场改（移调范围、速度范围、宏旋钮、给 AI 的演奏说明）。现场改动是叠加在文件之上的一层，不改写原曲。
5. **确定性**。同一文件、同一 seed、同一引擎版本，离线渲染得到相同的 PCM。
6. **资产自带来源**。每个采样/波表/图片都带哈希和许可信息。

## 2. 容器

一首曲子就是**一个 `.agbsong` 文件**：一个 zip 压缩包。传播、双击打开、上传下载的都是这一个文件。

创作时可以在一个普通文件夹（源文件夹）里逐个写文件，布局与包内相同，写完用 `agb song pack` 打成 `.agbsong`。源文件夹不是曲子文件，**不得**以 `.agbsong` 命名（建议 `<名字>.src`）；播放器和工具可以直接读源文件夹，方便创作时试听。

```
first-light.agbsong（zip 内部；源文件夹 first-light.src/ 布局相同）
├── mimetype            内容固定为 application/vnd.agb.song+zip（zip 中必须是第一个条目且不压缩）
├── song.json           唯一必需的文件：入口文档
├── cover.svg|png|jpg   封面（由 meta.cover 指向）
├── patches/*.json      可选：拆出去的音色
├── samples/*           采样（WAV / FLAC）
├── wavetables/*        用作波表的采样
├── midi/*.mid          可选：标准 MIDI 文件（导入源）
├── visuals/*           可选：视觉用的图片、着色器、随曲子附带的视频
├── preview.*           可选：参考渲染（给不能合成的播放器，或用于比对）
└── takes/*.json        可选：保存下来的现场演奏（见 §11）
```

规则：

- 路径一律相对包根，用 `/` 分隔；禁止绝对路径、`..`、反斜杠。播放器必须拒绝违规路径（包括 zip 条目名）。
- 文件名大小写敏感。以 `.` 开头的文件不属于包内容，打包时丢弃。
- `song.json` 为 UTF-8、无 BOM，大小上限 16 MiB。包解压后总大小上限 2 GiB。
- **`$ref` 拆分**：`song.json` 中任何位置的对象如果恰好是 `{ "$ref": "patches/bass.json" }`，播放器在解析前用该文件的 JSON 内容替换它。只能指向包内的 `.json` 文件，嵌套深度最多 8 层。这样大文件可以拆开，小文件可以全部内联。

## 3. 版本与扩展机制

这一节是格式能长期演进的基础，优先级高于后面所有具体字段。

### 3.1 版本

`formatVersion` 用 semver。

- **MAJOR.MINOR 相同**：必须能读。
- **MINOR 更高**（文件比播放器新）：播放器照常读，忽略不认识的字段。新 MINOR 只允许增加可选字段和新的特性 id。
- **MAJOR 不同**：拒绝打开并给出明确错误。0.x 阶段把 MINOR 当作兼容线（`0.1.x` 互相兼容，`0.2` 可以不兼容）。

### 3.2 未知字段

播放器**必须忽略**不认识的字段，不报错。编辑器改写文件时**应当保留**它不认识的字段。

### 3.3 特性声明：`requires` 与 `uses`

每个文件声明它用到了哪些特性：

```json
"requires": ["core"],
"uses": ["automation", "expression", "wavetable", "overlay", "visuals", "agent"]
```

- `requires`：缺了就不成其为这首曲子。播放器有任何一项不支持就**拒绝播放**。
- `uses`：用到了，但缺了也能听。播放器不支持时**跳过并报告**。

v0.1 定义的特性 id：

| id | 内容 | 章节 |
|----|------|------|
| `core` | 时间、音轨、loop、clip、内置引擎音色 | §5–§7 |
| `automation` | 参数自动化曲线 | §7.5 |
| `expression` | 逐音符表情与控制器（MIDI/MPE 语义） | §7.4 |
| `sampler` | 采样音色 `sampler.multi` | §8.3 |
| `wavetable` | 波表音色 `synth.wavetable` | §8.4 |
| `fx` | 轨道/总线效果器链 | §9 |
| `overlay` | 叠加频率层 | §10 |
| `visuals` | 视觉效果 | §12 |
| `agent` | 给 AI 演奏者的说明 | §11.3 |

第三方特性用反向域名或 `x-` 前缀：`x-acme.spatial`。

### 3.4 命名空间

- **引擎 id**：`synth.*`、`drum.*`、`sampler.*` 留给本规范；第三方用 `x-vendor.engine`。
- **效果器类型、叠加层类型、视觉场景**：同样规则。
- **`extensions`**：顶层对象，键是特性 id，值任意。大多数对象上也可以带 `extensions` 字段。

### 3.5 音色的降级链

音色可以写 `fallback` 列表。播放器按顺序取第一个它支持的。这样用了新引擎的文件，老播放器也能出声：

```json
"lead": {
  "engine": "synth.wavetable",
  "wavetable": "wt_glass",
  "params": { "position": 0.3 },
  "fallback": [{ "engine": "synth.subtractive", "preset": "preset:synth.subtractive:lead:soft_lead@v1.0" }]
}
```

## 4. `song.json` 顶层

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `format` | string | 是 | 固定 `"agbsong"` |
| `formatVersion` | string | 是 | 如 `"0.1.0"` |
| `requires` / `uses` | string[] | 否 | §3.3；缺省 `requires = ["core"]` |
| `meta` | object | 是 | §4.1 |
| `time` | object | 否 | §5 |
| `tuning` | object | 否 | §6 |
| `seed` | u64 | 否 | 随机种子，默认 0 |
| `assets` | map | 否 | §8.5，资产 id → 资产 |
| `patches` | map | 否 | §8，音色 id → 音色 |
| `tracks` | Track[] | 是 | §7 |
| `master` | object | 否 | 总线：`gainDb`（默认 −6）、`limiter`（`soft`/`hard`）、`fx` |
| `sections` | Section[] | 否 | 段落标记：`id`、`name`、`fromTick`、`toTick`、`intent`（给人和 AI 看的一句话） |
| `overlay` | object | 否 | §10 |
| `visuals` | object | 否 | §12 |
| `performance` | object | 否 | §11 |
| `extensions` | map | 否 | §3.4 |

id 一律匹配 `[a-zA-Z0-9_-]{1,64}`。

### 4.1 `meta`

| 字段 | 说明 |
|------|------|
| `title` | 必填，1–128 字符 |
| `artists` | `[{ "name", "kind": "ai"\|"human", "model"? }]`。AI 作者写明模型名 |
| `description` | 自由文本 |
| `tags` | 字符串数组 |
| `created` | ISO 8601 时间 |
| `license` | 作品许可，SPDX id 或自由文本 |
| `generator` | `{ "name", "version" }`，写出此文件的软件 |
| `cover` | 资产 id，指向封面图片 |

## 5. 时间

```json
"time": {
  "ppqn": 960,
  "bpm": 84,
  "tempoMap": [{ "tick": 61440, "bpm": 92 }],
  "meter": [4, 4],
  "stepsPerQuarter": 4,
  "lengthTicks": 122880,
  "loop": { "fromTick": 0, "toTick": 122880 }
}
```

- 所有时间位置都是**整数 tick**，从 0 开始。`ppqn` 默认 960（每四分音符的 tick 数）。
- `bpm` 是起始速度（20–400）；`tempoMap` 是之后的变速点，按 tick 升序。
- `stepsPerQuarter`：loop 用的步进网格，默认 4（十六分音符）。`ppqn` 必须能被它整除。
- `lengthTicks`：曲子长度。省略时由播放器推算（clip、段落、自动化的最远结束点；只有 loop 时取一个完整的 polymeter 周期）。
- `loop`：可选，播放器循环播放的区间。
- `meter`：拍号，默认 4/4。v0.1 全曲一个拍号。

换算与 `SPEC.md` §5.2 相同：`secPerTick = 60 / (bpm × ppqn)`。

## 6. 调音与调性

```json
"tuning": { "a4Hz": 432, "key": "D", "scale": "dorian" }
```

- `a4Hz`：基准音高，默认 440。
- `key` / `scale`：调性提示。不影响发声，但播放器和 AI 靠它做两件事：移调后知道新的调；判断叠加频率与和声是否协和。
- `scale` 取值：`major`、`minor`、`dorian`、`phrygian`、`lydian`、`mixolydian`、`locrian`、`pentatonic_major`、`pentatonic_minor`、`chromatic`，或自定义名。
- 非十二平均律留给扩展（`extensions["x-tuning.scl"]`）。

## 7. 音轨

```json
{
  "id": "bass",
  "name": "低音",
  "role": "bass",
  "patch": "bass",
  "gainDb": -6,
  "pan": 0,
  "mute": false,
  "transposable": true,
  "loops": [ … ],
  "clips": [ … ],
  "automation": [ … ],
  "controllers": [ … ],
  "fx": [ … ]
}
```

| 字段 | 说明 |
|------|------|
| `patch` | 音色 id（§8），必填 |
| `role` | `drums`/`bass`/`lead`/`pad`/`keys`/`perc`/`fx`/…，自由文本，供 AI 和界面使用 |
| `gainDb` | −60…12，默认 0 |
| `pan` | −1（左）…1（右），默认 0 |
| `transposable` | 全局移调时是否跟随。省略时：`drum.*` 引擎为 false，其余为 true |

一条音轨的音符有两个来源，可以同时使用：

### 7.1 loop（循环层，polymeter 的核心）

```json
{ "id": "kick", "lengthSteps": 7,
  "notes": [{ "step": 0, "pitch": 36, "durSteps": 1, "velocity": 0.9 }],
  "fromTick": 0, "toTick": 61440 }
```

- 长度为 `lengthSteps` 步的片段，从曲子起点开始按全局步进网格无限循环。同一首曲子里各 loop 长度不同就形成 polymeter。
- 相位锚定在曲子起点（tick 0），不是 `fromTick`。`fromTick`/`toTick` 只决定它什么时候出声（省略 = 全曲）。
- `phaseOffsetSteps`：相位偏移。
- 鼓可以用简写 `"steps": [1,0,0,1,0,0,1]`，每个 1 触发 MIDI 36。`notes` 非空时以 `notes` 为准。

### 7.2 clip（线性片段）

```json
{ "atTick": 15360, "lengthTicks": 15360, "repeat": 2,
  "notes": [{ "tick": 0, "pitch": "D3", "durTicks": 1920, "velocity": 0.7 }] }
```

- `notes[].tick` 相对 clip 起点。
- `repeat`：重复次数（默认 1），每次间隔 `lengthTicks`。有 `repeat > 1` 时必须给 `lengthTicks`。
- 给了 `lengthTicks` 时，起点落在它之外的音符不发声；起点在内的音符可以延续到 clip 结束之后。
- `midi`：可选 `{ "asset": "<id>", "track": 0 }`，从包内的标准 MIDI 文件取音符，代替 `notes`。JSON 的 `notes` 是规范形式；SMF 只是导入/导出用的互操作格式。

### 7.3 音符

| 字段 | loop | clip | 说明 |
|------|------|------|------|
| `step` / `tick` | `step` | `tick` | 位置 |
| `pitch` | ✓ | ✓ | `"E1"`、`"C#3"`、`"Bb2"`（C4 = 60）或 MIDI 数字 0–127 |
| `durSteps` / `durTicks` | ✓ | ✓ | 时值，≥ 1 |
| `velocity` | ✓ | ✓ | 0–1，默认 0.8 |
| `releaseVelocity` | ✓ | ✓ | 0–1，可选 |
| `detuneCents` | ✓ | ✓ | 这个音的微调，可选 |
| `expression` | ✓ | ✓ | §7.4 |

### 7.4 表情（特性 `expression`）

语义对应 MIDI 2.0 的逐音符控制器和 MPE，但写成 JSON 曲线，不占用通道。

**逐音符**——写在音符的 `expression` 里，时间是相对音符起点的 tick：

```json
"expression": {
  "bend":     [[0, 0], [480, 2], [960, 0]],
  "pressure": [[0, 0.2], [960, 0.9]],
  "timbre":   [[0, 0.5], [960, 0.8]]
}
```

| 维度 | 单位 | 对应 MIDI |
|------|------|-----------|
| `bend` | 半音，可为小数 | per-note pitch bend |
| `pressure` | 0–1 | polyphonic aftertouch / MPE pressure |
| `timbre` | 0–1 | MPE CC74 |

点之间线性插值。新维度可以直接加（未知维度按 §3.2 忽略）。

**整轨**——写在音轨的 `controllers` 里，时间是绝对 tick：

```json
"controllers": [
  { "name": "modWheel", "points": [[0, 0], [7680, 1]] },
  { "cc": 11, "points": [[0, 0.8]] }
]
```

`name` 取值：`pitchBend`（半音）、`modWheel`、`expression`、`sustain`、`aftertouch`（均 0–1）；或者用 `cc` 写 MIDI 控制器号，值归一化到 0–1。音色通过 `modulation` 决定这些来源控制什么（§8.2）。

### 7.5 自动化（特性 `automation`）

```json
"automation": [
  { "target": "patch.filter.cutoffHz", "points": [[0, 400], [15360, 2400], [30720, 800, "step"]] }
]
```

- `target`：`patch.<参数路径>`（音色参数）、`track.gainDb`、`track.pan`、`fx.<效果器 id>.<参数>`。总线的自动化写在 `master.automation`，target 用 `master.gainDb`、`fx.<id>.<参数>`。
- `points`：`[tick, value]` 或 `[tick, value, curve]`。`curve` 描述**从上一个点到这个点**的走法：`linear`（默认）或 `step`（保持上一个值，到点跳变）。第一个点之前不生效。

## 8. 音色（patch）

### 8.1 通用字段

```json
"bass": {
  "engine": "synth.moogish",
  "preset": "preset:synth.moogish:bass:sub_ladder@v1.0",
  "params": { "filter.cutoffHz": 320 },
  "macros": { "darkness": [{ "param": "filter.cutoffHz", "min": 2000, "max": 150 }] },
  "modulation": [{ "source": "pressure", "param": "filter.cutoffHz", "amount": 1800 }],
  "fallback": [ … ]
}
```

| 字段 | 说明 |
|------|------|
| `engine` | 引擎 id。给了 `preset` 时可省略（从预置推断） |
| `engineVersion` | 作者使用的引擎版本，仅用于诊断 |
| `preset` | 内置预置 id（`SPEC.md` §9.7）。可选：不依赖预置库的文件应把参数完整写在 `params` 里 |
| `params` | 参数路径 → 数值或枚举字符串。生效顺序：引擎默认 ← 预置 ← `params` |
| `macros` | 宏名 → 目标列表。宏值 0–1 线性映射到 `min`…`max`。播放器把宏显示成旋钮（§11） |
| `modulation` | 调制矩阵：`source`（`velocity`、`pressure`、`timbre`、`modWheel`、`cc74`、`keytrack`…）→ `param`，`amount` 用参数自身的单位 |
| `fallback` | §3.5 |

**自带音色设计**：一个可以独立流通的文件应当把音色写全（`engine` + `params`），而不是只引用某个播放器的预置库。预置 id 是方便，不是规范的一部分。

### 8.2 规范定义的引擎

| engineId | 内容 | 参数定义 |
|----------|------|----------|
| `synth.waveform` | 基础波形 + ADSR | `SPEC.md` §9.4 |
| `synth.subtractive` | 2 OSC + 噪声 → SVF → 放大 | `agb engine params synth.subtractive` |
| `synth.moogish` | 同上，24 dB 梯形低通 + drive | 同上 |
| `synth.modal` | 模态合成（钵、钟、音叉、锣） | 同上 |
| `drum.analog808style` | 按音高选鼓件的模拟鼓 | 同上 |
| `sampler.multi` | 多采样 | §8.3 |
| `synth.wavetable` | 波表 | §8.4 |

### 8.3 采样音色 `sampler.multi`（特性 `sampler`）

```json
"kit": {
  "engine": "sampler.multi",
  "zones": [
    { "sample": "kick_a", "keyRange": [36, 36], "rootPitch": 36, "oneShot": true },
    { "sample": "piano_c3", "keyRange": [43, 54], "velRange": [0, 0.6], "rootPitch": 48,
      "loop": { "startFrame": 20000, "endFrame": 61000, "crossfadeFrames": 512 } }
  ],
  "params": { "amp.attackMs": 2, "amp.releaseMs": 300 }
}
```

- 每个 zone：`sample`（资产 id）、`keyRange`、`velRange`、`rootPitch`、`tuneCents`、`gainDb`、`pan`、`oneShot`、`loop`、`startFrame`/`endFrame`、`chokeGroup`。
- 多个 zone 同时命中时全部发声（叠层）；需要轮换用 `roundRobinGroup`：同一组里命中的 zone 每次只响一个，同一个音符在同一位置总是选到同一个。
- `velRange` 按音符自己的力度选层，与音轨音量无关。
- 音色参数 `vibrato.depthCents` 大于 0 时，音符开始 `vibrato.delayMs` 之后出现揉弦，适合主音吉他这类长音。
- 通用的放大包络和滤波参数走 `params`。

### 8.4 波表音色 `synth.wavetable`（特性 `wavetable`）

```json
"lead": {
  "engine": "synth.wavetable",
  "wavetable": "wt_glass",
  "params": { "position": 0.25, "amp.attackMs": 30, "filter.cutoffHz": 4000 }
}
```

- `wavetable` 指向 `kind: "wavetable"` 的资产：一个单声道音频文件，按 `frameSize`（默认 2048）切成等长的帧，每帧是一个周期。
- `position`（0–1）在帧之间扫描并插值。它是最常用的自动化和表情目标。
- 任何采样都可以当波表用：资产上写 `frameSize`，或者写 `"slice": "auto"` 让播放器按检测到的基频切周期。

### 8.5 资产

```json
"assets": {
  "wt_glass": {
    "path": "wavetables/glass.wav",
    "kind": "wavetable",
    "sha256": "sha256:…",
    "frameSize": 2048,
    "license": { "spdx": "CC0-1.0", "redistributable": true, "source": "原创，由脚本生成" }
  },
  "cover": { "path": "cover.svg", "kind": "image" }
}
```

| 字段 | 说明 |
|------|------|
| `path` | 包内相对路径，必填 |
| `kind` | `sample`、`wavetable`、`image`、`video`、`midi`、`shader`、`audio`、`other` |
| `sha256` | `sha256:<hex>`。有则播放器必须校验 |
| `license` | `{ spdx, redistributable, source?, notes? }`。音频资产应当有 |
| `rootPitch`、`frameSize`、`slice` 等 | 按 `kind` 而定的元数据 |

音频用 WAV（PCM16/24/float32）或 FLAC；图片用 PNG、JPEG 或 SVG。不得包含无权再分发的素材（`SPEC.md` §9.1）。

## 9. 效果器（特性 `fx`）

音轨和总线都可以有 `fx` 链，按顺序串联：

```json
"fx": [
  { "id": "dly", "type": "delay", "params": { "timeBeats": 0.75, "feedback": 0.4, "mix": 0.25 } },
  { "id": "verb", "type": "reverb", "params": { "size": 0.8, "decaySec": 4.5, "mix": 0.3 }, "bypass": false }
]
```

v0.1 规定的类型和参数：

| type | 参数 |
|------|------|
| `eq` | `lowGainDb`、`midGainDb`、`midFreqHz`、`highGainDb` |
| `filter` | `mode`（lp/hp/bp）、`cutoffHz`、`resonance` |
| `delay` | `timeBeats` 或 `timeMs`、`feedback`、`mix`、`pingPong` |
| `reverb` | `size`、`decaySec`、`damping`、`mix` |
| `chorus` | `rateHz`、`depth`、`mix` |
| `drive` | `amount`、`tone`、`mix` |
| `compressor` | `thresholdDb`、`ratio`、`attackMs`、`releaseMs`、`makeupDb` |
| `amp` | 吉他音箱和箱体：`gain`（0–1，从干净到高增益）、`bass` / `mid` / `treble`（−1–1）、`presence`（0–1）、`cabinet`（`combo` / `stack` / `none`）、`levelDb` |

- 不认识的 `type`：旁通并报告。
- 效果器链听到的是这条轨在推子之前的声音：`gainDb` 只改变它在混音里的大小，不改变音箱被推得多狠。
- 失真吉他必须用 `amp`，不能把失真录进采样：和弦是先相加、再一起过载的，过载后才有的差音是这个音色的主体。采样用乐器直接输出的干声。
- 文件里的效果器参数是**初始值**。播放器的效果器面板直接改这些参数，这正是本格式不是音频文件的意义所在。
- 不同播放器的混响、延迟算法不同，音色不保证逐样本一致。确定性（原则 5）只约束同一引擎版本。

## 10. 叠加频率（特性 `overlay`）

叠加在音乐之上、独立于乐谱的持续声音层。这是本格式的一等公民，不是扩展。

```json
"overlay": {
  "layers": [
    { "id": "solfeggio", "kind": "tone", "hz": 528, "gainDb": -24, "enabled": true },
    { "id": "theta", "kind": "binaural", "carrierHz": 200, "beatHz": 6, "gainDb": -26 },
    { "id": "air", "kind": "noise", "color": "pink", "gainDb": -36, "enabled": false }
  ]
}
```

| kind | 字段 | 说明 |
|------|------|------|
| `tone` | `hz`，`wave`（默认 `sine`） | 单一频率 |
| `binaural` | `carrierHz`、`beatHz` | 左耳 `carrierHz`，右耳 `carrierHz + beatHz`；需要立体声和耳机 |
| `isochronic` | `hz`、`pulseHz` | 以 `pulseHz` 开关的单音 |
| `noise` | `color`：`white`/`pink`/`brown` | 噪声床 |

通用字段：`id`、`gainDb`、`enabled`（默认 true）、`pan`、`fadeInMs`/`fadeOutMs`、`schedule`（可选，`[[tick, gainDb], …]`，随曲子进行改变电平）。

规则：

- 叠加层**不跟随移调和变速**。528 Hz 就是 528 Hz。
- 叠加层**不经过音乐的效果器链**，直接进总线的限幅器之前。
- 作者应当让和声与叠加频率协和，并在 `tuning` 里写明基准音高（例如 528 Hz 对应 A4 = 444 Hz 时的 C5）。播放器在移调后可以提示新的调与叠加频率的关系，但不自动改频率。
- 文件里的是默认状态。听的人可以在播放器里开关、换频率、加层。
- 这些声音只作放松辅助，不是医疗用途。播放器不应作疗效声明。

## 11. 演奏（播放时可以改什么）

### 11.1 播放器必须提供的现场控制

任何时候都可以改，不改写文件：

| 控制 | 语义 |
|------|------|
| 移调 | 整数半音，作用于所有 `transposable` 的音轨；叠加层不动 |
| 速度 | 绝对 BPM 或倍率，`tempoMap` 按比例缩放 |
| 基准音高 | `a4Hz` |
| 每轨推子 / 静音 / 独奏 | |
| 效果器参数、旁通 | §9 |
| 叠加层 | §10 |
| 宏旋钮 | `performance.controls` |

### 11.2 `performance`

```json
"performance": {
  "transpose": { "min": -7, "max": 7 },
  "bpm": { "min": 60, "max": 110 },
  "controls": [
    { "id": "darkness", "label": "暗度", "default": 0.3,
      "targets": [
        { "target": "patch.bass.filter.cutoffHz", "min": 2000, "max": 150 },
        { "target": "fx.master.verb.mix", "min": 0.1, "max": 0.5 }
      ] }
  ],
  "agent": { … }
}
```

- `transpose` / `bpm`：作者建议的范围，播放器界面据此设定滑块范围。
- `controls`：作者设计的宏旋钮，一个旋钮可以同时推动多个参数。target 写法：`patch.<patchId>.<参数>`、`track.<trackId>.gainDb`、`fx.<trackId|master>.<fxId>.<参数>`、`overlay.<layerId>.gainDb`。

### 11.3 `performance.agent`（特性 `agent`）

给接手演奏的 AI 的说明。它让"和 AI 对话来改变演奏"在不同播放器、不同模型之间可移植：

```json
"agent": {
  "brief": "清晨的冥想电子乐，D dorian，稀疏，颂钵为主",
  "persona": "克制的合成器演奏者，宁可少弹",
  "rules": ["不要加军鼓", "和声保持与 528 Hz 协和"],
  "allow": ["notes", "patches", "fx", "tempo", "overlay"],
  "sectionsAreFixed": false
}
```

- `allow`：AI 可以改动的范围。播放器必须拒绝范围之外的改动。
- `sections[].intent` 告诉 AI 每一段想表达什么。

### 11.4 现场改动的表示

现场改动是**事务**（`SPEC.md` §8 的 Transaction），在小节线上量化生效。人拧旋钮和 AI 改乐句走同一条通道。播放器维护"文件 + 事务列表"，原文件不变。

规则：

- 改动只能落在还没播放的小节线上，已经听到的部分不会变。
- 混音、音色、速度的改动尽快生效；改写的音符保持作者（AI）写它们时对应的位置，来不及的部分被裁掉。
- 要改写已经写好的声部，先用 `clip.clear` 清掉该轨后面的音符，再用 `clip.schedule` 写新的。
- 一次回答里的改动要么全部生效，要么全部不生效。

### 11.5 保存一次演奏：`takes/`

一次演奏可以保存为 `takes/<id>.json`：

```json
{ "id": "take-2026-11-14", "baseSha256": "sha256:…", "transpose": 2, "bpmScale": 1.05,
  "transactions": [ … ], "dialogue": [ { "who": "listener", "text": "再安静一点", "atTick": 30720 } ] }
```

`baseSha256` 是演奏时 `song.json` 的哈希。重放 = 打开文件 + 依次应用事务。要把一次演奏固化成新曲子，就导出为新的 `.agbsong`。

## 12. 视觉（特性 `visuals`）

声明式的：文件描述"画什么、被什么驱动"，由播放器绘制。

```json
"visuals": {
  "palette": ["#0b1020", "#3a6ea5", "#f2c14e", "#f7f3e3"],
  "scene": "orbits",
  "params": { "trail": 0.6, "glow": 0.4 },
  "bindings": [
    { "source": "track.drums.level", "target": "scene.pulse", "scale": 1.2 },
    { "source": "overlay.theta.beatPhase", "target": "scene.breath" },
    { "source": "section", "target": "scene.paletteShift" }
  ],
  "shader": "viz_main"
}
```

- `palette`：主色，播放器界面也可以跟着取色。封面在 `meta.cover`。
- `scene`：规范场景名。v0.1 定义 `orbits`（polymeter 圆环）、`river`（音符流）、`field`（粒子场）、`spectrum`（频谱）。不认识就退回播放器默认场景。
- `bindings.source`：`beat`、`bar`、`section`、`master.level`、`master.spectrum`、`track.<id>.level`、`track.<id>.noteOn`、`overlay.<id>.beatPhase`、`control.<id>`。
- `shader`：可选，指向 `kind: "shader"` 的资产（GLSL ES 3.0 片段着色器，uniform 约定见后续版本）。播放器可以不支持着色器；支持时必须沙箱运行，不给它文件或网络访问。
- `video`：可选，随文件附带的一段预先渲染好的视频（例如由写这首曲子的 agent 渲染），见 §12.1。
- 分段的视觉变化写在 `sections[].visuals`（与顶层 `visuals` 同结构，按字段覆盖）。

### 12.1 封面与视频

一个 `.agbsong` 应当能"直接打开就像一张唱片"：播放器打开文件时显示它自带的画面：有视频就播放视频，没有就显示封面，两者之间可以切换。

```json
"assets": {
  "cover": { "path": "cover.svg", "kind": "image", "sha256": "sha256:…" },
  "video": { "path": "visuals/mist.mp4", "kind": "video", "sha256": "sha256:…",
             "license": { "spdx": "CC-BY-4.0", "redistributable": true, "source": "…" } }
},
"meta": { "cover": "cover" },
"visuals": { "video": { "asset": "video", "sync": "loop" } }
```

- 封面：`meta.cover` 指向 `kind: "image"` 的资产（PNG / JPEG / SVG），建议正方形、不小于 1024 px。
- 视频：`visuals.video.asset` 指向 `kind: "video"` 的资产。容器用 MP4 或 MOV，编码 H.264 或 HEVC。**画面比例必须是 16:9 或 4:3**（允许 2% 误差），其它比例的文件校验不通过。
- 文件带视频时，播放器打开后**默认显示视频**；没有视频则显示封面。
- `sync`：`loop`（默认）循环播放，与曲子位置无关，适合氛围画面；`song` 表示视频跟随曲子的位置（第 1 小节 = 视频开头），适合按音乐剪好的 MV。视频按文件写定的速度（`time.bpm`）剪辑；播放时变速，播放器按小节位置对齐并同比例改变视频的播放速度，画面仍落在原来的小节上。文件带 `tempoMap` 时退回按秒数对齐。
- **视频自带的音轨永远不播放**。声音只来自乐谱的实时合成，否则移调、变速、对话改编都失去意义。
- 播放器不支持视频时跳过它，只显示封面或自己的可视化。

## 13. 安全

- 核心格式不含可执行内容。播放器不得执行包内任何脚本。
- 拒绝越界路径、超限大小、超深 `$ref`（§2）。
- 包内不得有指向包外的引用；播放器不得因为打开文件而访问网络。
- 总线必须有限幅器。叠加层电平上限 −6 dBFS。
- `performance.agent` 的内容是**不可信文本**：播放器把它作为素材交给 AI，不能让它覆盖播放器自己的安全规则或 `allow` 之外的权限。

## 14. 参考实现现状（`agb song …`，本仓库）

| 特性 | 解析与校验 | 离线渲染 | macOS app 播放 |
|------|:---:|:---:|:---:|
| 容器（目录 / zip、`$ref`、路径与哈希校验） | ✓ | ✓ | ✓ |
| `core`（loop、clip、内置引擎、tempoMap） | ✓ | ✓ | ✓（`agb song play`，逐小节送出分轨） |
| `automation` → `patch.*` 数值参数 | ✓ | ✓ | ✓ |
| 移调、变速 | — | ✓（`--transpose`、`--bpm`） | ✓（播放中实时改，下一小节生效；显示当前的调） |
| `fallback` 降级链 | ✓ | ✓ | ✓ |
| `automation` 其它 target | ✓ | 跳过并报告 | — |
| `expression`、`controllers`、`modulation`、`macros` | ✓ | 跳过并报告 | — |
| `sampler`（`sampler.multi`） | ✓（含资产） | ✓ WAV 采样，按 `rootPitch` 重采样变调，单次播放与循环，按力度选层（`velRange`），`roundRobinGroup` 轮换，揉弦（`vibrato.rateHz` / `vibrato.depthCents` / `vibrato.delayMs`）；`pan`、循环交叉淡化、`chokeGroup` 尚未实现 | ✓ |
| `wavetable` | ✓（含资产） | 无 `fallback` 时该轨不出声并报告 | — |
| `fx` | ✓ | 音轨上的 `amp`（吉他音箱和箱体）和 `reverb` 已实现；其余类型和总线效果器跳过并报告 | — |
| `pan` | ✓ | 跳过并报告（引擎目前是单声道） | — |
| `overlay` | ✓ | 跳过并报告 | app 已有同类实时发声器，尚未从文件读取 |
| `agent`（播放时与 AI 对话、`allow` 约束） | ✓ | 不适用 | ✓ |
| `takes/`（演奏记录） | ✓ | 不适用 | 写到工作目录的 `take.json`，还不能写回包内或重放 |
| 封面（`meta.cover`）、视频（`visuals.video`） | ✓ | 不适用 | ✓（舞台上“封面 / 圆环 / 视频”切换） |
| `visuals` 其余部分（场景、绑定、着色器） | ✓ | 不适用 | — |
| clip 的 `midi`（SMF） | ✓（引用） | 跳过并报告 | — |

引擎当前的预算（`SPEC.md` §12.1）：8 轨、每轨 4 个 loop、32 复音。超出时渲染报 `BUDGET_EXCEEDED`。这是播放器的限制，不是格式的限制。

## 15. 最小示例

```json
{
  "format": "agbsong",
  "formatVersion": "0.1.0",
  "meta": { "title": "七与五" },
  "time": { "bpm": 100, "stepsPerQuarter": 2 },
  "patches": {
    "kit":  { "preset": "preset:drum.analog808style:drums:classic_kit@v1.0" },
    "bass": { "engine": "synth.waveform", "params": { "osc.wave": "saw", "amp.releaseMs": 120 } }
  },
  "tracks": [
    { "id": "drums", "patch": "kit",
      "loops": [{ "id": "k", "lengthSteps": 7, "steps": [1, 0, 0, 1, 0, 0, 1] }] },
    { "id": "bass", "patch": "bass",
      "loops": [{ "id": "riff", "lengthSteps": 5,
        "notes": [{ "step": 0, "pitch": "E1", "durSteps": 2 }, { "step": 3, "pitch": "B1" }] }] }
  ]
}
```

完整示例见 `examples/first-light.agbsong`（源文件夹 `examples/first-light.src/`）和 `examples/when-the-mist-lifts.agbsong`。

```bash
agb song validate examples/first-light.agbsong
agb song render   examples/first-light.agbsong --out-dir /tmp/first-light --transpose 2 --bpm 96
agb song pack     examples/first-light.src --out examples/first-light.agbsong    # 源文件夹 → 一个文件
```
