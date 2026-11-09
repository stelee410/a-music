# Agent Groovebox — 工程规范（SPEC）

| 字段 | 值 |
|------|-----|
| 文档版本 | `0.1.0-spec` |
| 状态 | **规范 only** — 未实现、无可运行产物承诺 |
| 目标读者 | 用户本机 coding agent / 工程师 |
| 语言 | 中文（标识符与 JSON 字段用英文） |
| 派生自 | Agent-first Groovebox 设计讨论与研究笔记（非厂商官方） |
| 首里程碑 | **确定性 seeded offline 渲染 WAV**（CLI + JSON 事务） |

> **本文件是权威实现规格。** 先前设计 Page 仅作历史上下文；实现以本文为准。VM 空声卡等探测结论**不**构成用户本机开发限制。

---

## 0. 一句话范围

做一个 **Agent-first、headless** 的 groovebox 引擎：单一共享 tick 时钟；每轨/每层可有**不同 loop 长度**（polymeter）；结构化 JSON 事务作曲/换音色；**P0 交付 deterministic offline WAV**。UI、HTTP、MCP、实时出声均为后续适配，不是首里程碑。

---

## 1. 默认决策（消除歧义）

下列项已拍板；实现勿再向用户追问。可用配置覆盖的已标明。

| 议题 | 默认 |
|------|------|
| 权威位置 | 本机进程内 headless engine（单一时钟权威） |
| 技术栈 | **Rust 2021 workspace** + CLI；offline PCM 自研；**不**以浏览器/Tone.js 为权威核心 |
| 协议 | **stdio / 文件 JSON 事务**优先；HTTP、MCP 为后置薄适配同一 schema |
| waveform | **同时**支持基础波形 OSC + wavetable（用户原词未明时的假设，现固化） |
| 工程许可倾向 | 自研代码默认 **Apache-2.0 或 MIT**；**不**默认嵌入 GPL（Dexed/Surge XT） |
| 707 样本 | factory 仅 **原创 / CC0 / 明确可再分发**；用户私有 kit 可本地引用但不打进 factory |
| Moog ladder | P1 用通用减法 + `moogish` 预置标签；**ladder 深化在 P2** |
| 晚到事件 | 默认 `reject` |
| 音色切换音尾 | 默认 `release_old` |
| 场景相位 | 默认 `hard_reset_on_scene` |
| 采样率 | 默认 `48000` Hz；位深导出 float32 WAV 或 PCM16（render 可选，默认 PCM16） |
| PPQN | 默认 `960`；UI/网格可用 `step = 八分音符 = ppqn/2` |
| 主增益 | 默认 master −6 dBFS headroom；硬限幅在 0 dBFS |
| 随机性 | 一切随机（粒子等）必须有 `seed`；同 seed 同工程 → 同 PCM |
| LLM | 只允许在 Control Plane **提交前**使用；**禁止**音频线程 / render 内调用 LLM |

---

## 2. 非目标（明确不做）

- 冒称 Roland / Moog / Yamaha **官方复刻**，或打包/分发原厂 ROM/固件样本  
- 网络 **逐 note 实时 RPC** 作为权威演奏路径  
- 多进程各持一个 Transport 时钟  
- DX7 sysex / 精确兼容（延后；可选研究 msfa，不进 P0–P1）  
- 默认嵌入 Dexed、Surge XT、JUCE 宿主  
- 把 UI 自动化当 Agent API  
- 声称 metrics「好听」；声称已测延迟/CPU 性能数字  
- P0 一次做完全部 DSP 引擎与 400–500 预置  

---

## 3. 技术栈（锁定）

### 3.1 默认栈

```
Rust workspace
├── agb-cli          # 二进制：subcommand + JSON 事务
├── agb-protocol     # 命令/响应/错误码类型 + JSON Schema 导出
├── agb-engine       # 时钟、轨/层、场景、事务、revision、幂等
├── agb-score        # pattern/notes/automation 数据结构
├── agb-dsp          # voice 接口 + P0/P1 合成（可渐进）
├── agb-render       # offline 块渲染 → WAV / stems / metrics
├── agb-presets      # catalog、元数据、迁移
└── agb-assets       # 资产索引、hash、license registry
```

| 依赖方向 | 选择 | 备注 |
|----------|------|------|
| CLI 参数 | `clap` | 标准 |
| 序列化 | `serde` + `serde_json` | 命令与工程文件 |
| WAV 写 | `hound` 或等价 | offline 导出 |
| 测试 | `cargo test` + 金样哈希 | P0 必备 |
| 实时音频 | **P3+ 可选** `cpal`（Apache-2.0） | 基于公开文档的可行性，非本机实测 |
| 采样宿主 | **P1+ 可选** `sfizz`（BSD-2-Clause） | 707style / 用户 SFZ；样本另授权 |
| Faust | **可选离线代码生成** | 编译器 LGPL；生成码取决于输入库许可（见 Faust FAQ） |
| Tone.js / Web Audio | **非核心** | 仅当未来做「订阅时间线的本机试听前端」时可选 |

### 3.2 为何不用「浏览器权威 + Tone」做默认

先前研究中 Tone.js（MIT）与 Web Audio lookahead 调度适合**浏览器试听**；但 Agent-first 需要稳定 CLI、确定性 offline、无页面生命周期。故：**Rust headless 为权威**；浏览器最多做只读渲染器。

参考：  
- https://web.dev/articles/audio-scheduling  
- https://tonejs.github.io/docs/15.1.22/functions/Offline.html  
- https://raw.githubusercontent.com/Tonejs/Tone.js/dev/LICENSE.md  
- https://github.com/RustAudio/cpal  
- https://raw.githubusercontent.com/sfztools/sfizz/develop/LICENSE  
- https://faustdoc.grame.fr/manual/faq/

---

## 4. 架构

```
Agent / 人
    │  JSON Transaction（幂等 commandId + revision）
    ▼
Control Plane（agb-cli / 未来 HTTP·MCP）
    │  validate → commit at quantize tick
    ▼
Engine（单一 PPQN 时钟 + score + preset 注册表）
    │  不可变时间线切片
    ▼
Voice Rack（DSP） ──► Offline Render ──► mix.wav / stems / metrics.json
```

**规则：**

1. 只有 Engine 持有权威 `transportTick` 与 `revision`。  
2. Render 不得自行改 score；只读已提交状态。  
3. 跨机器若将来同步：传 **已提交时间线或 artifact**，不传逐音触发。

---

## 5. 时间模型

### 5.1 定义

| 术语 | 定义 |
|------|------|
| **Polymeter** | 同一全局脉冲；不同层 `lengthSteps` 不同；共享 BPM 与 `originTick` |
| **Polyrhythm** | 同一时间窗内不同均匀分割（如 3:2）；MVP 可用不同 length 近似，专用 tuplet API 可后置 |
| **Tuplet** | 有理细分进整数 tick；**不是**第二全局 BPM |
| **多 BPM** | **非目标**（P0–P2）；将来需 tempo map |

### 5.2 换算（必须实现为纯函数）

```
stepsPerQuarter = 2          # 默认：八分音符为 1 step（四分 = 2 steps）
ticksPerStep    = ppqn / stepsPerQuarter   # 960/2 = 480
secPerTick      = 60 / (bpm * ppqn)
sampleIndex     = round(originSample + tick * sampleRate * secPerTick)
```

默认：`bpm=120`，`ppqn=960`，`sampleRate=48000`，`originTick=0`，`originSample=0`。

### 5.3 相遇周期（验收用）

三层 loop 步长（八分音符 step）：`7`、`5`、`8`（4/4 一小节 = 8 个八分）。

\[
\mathrm{LCM}(7,5,8)=280
\]

- 7 步层：40 圈  
- 5 步层：56 圈  
- 8 步层：35 圈  

从同一 `originTick` 齐走后，再同时回到各自 step 0 的全局 step 数为 **280**（tick = `280 * ticksPerStep`）。

### 5.4 相位策略

| 值 | 行为 |
|----|------|
| `hard_reset_on_scene`（默认） | 场景切换时各层 phase 归零（相对 scene 原点） |
| `free_phase` | 保持各层当前偏移连续 |

---

## 6. 仓库与文件布局（建议）

```
agent-groovebox/
├── Cargo.toml                 # workspace
├── crates/
│   ├── agb-cli/
│   ├── agb-protocol/
│   ├── agb-engine/
│   ├── agb-score/
│   ├── agb-dsp/
│   ├── agb-render/
│   ├── agb-presets/
│   └── agb-assets/
├── schemas/                   # JSON Schema（由 protocol 导出或手写同步）
│   ├── transaction.schema.json
│   ├── project.schema.json
│   └── preset.schema.json
├── factory/
│   ├── presets/               # 预置 JSON
│   ├── kits/                  # 合法 PCM / SFZ（无原厂 ROM）
│   └── licenses/
├── examples/
│   ├── polymeter_280.json     # 本文样例 1
│   ├── render_p0.json         # 本文样例 2
│   └── preset_load.json       # 本文样例 3
├── tests/golden/              # 金样 wav hash 清单
└── SPEC.md                    # 可复制本规范或软链
```

工程运行时目录（`projectRoot`）：

```
my-song/
├── project.json
├── assets/
├── renders/
└── .agb-idempotency/          # commandId → 首次响应缓存（实现可选路径）
```

---

## 7. 数据模型

### 7.1 Project（`project.json`）

| 字段 | 类型 | 默认 | 约束 |
|------|------|------|------|
| `specVersion` | string | `"0.1.0"` | semver；不兼容则拒开 |
| `name` | string | `"untitled"` | 1–128 字符 |
| `tempoBpm` | f64 | `120.0` | `20..=400` |
| `ppqn` | u32 | `960` | 必须能被 `stepsPerQuarter` 整除 |
| `stepsPerQuarter` | u32 | `2` | ≥1 |
| `sampleRate` | u32 | `48000` | `44100` 或 `48000`（P0） |
| `originTick` | u64 | `0` | |
| `revision` | u64 | `0` | 每次成功写事务 +1 |
| `masterGainDb` | f64 | `-6.0` | `-60..=0` |
| `stealPolicy` | enum | `"oldest"` | `oldest` \| `quietest` \| `same_pitch` |
| `tracks` | Track[] | `[]` | ≤8（P0 预算） |
| `scenes` | Scene[] | `[]` | ≤8（预算） |
| `assets` | AssetRef[] | `[]` | |
| `licensesRegistry` | object | `{}` | assetId → license 记录 |
| `seed` | u64 | `0` | 工程级默认 RNG seed |

### 7.2 Track

| 字段 | 类型 | 默认 | 约束 |
|------|------|------|------|
| `id` | string | — | 唯一；`[a-zA-Z0-9_-]{1,64}` |
| `engineId` | string | `"synth.waveform"` | 见 §9 |
| `presetId` | string\|null | `null` | |
| `channel` | u8 | `0` | 路由/总线标签 |
| `mute` | bool | `false` | |
| `gainDb` | f64 | `0.0` | `-60..=12` |
| `owner` | string\|null | `null` | 多 Agent ACL；P0 可忽略强制 |
| `layers` | Layer[] | `[]` | ≤4（预算） |
| `fx` | FxStep[] | `[]` | P0 可空；P1+ limiter 默认开 |

### 7.3 Layer

| 字段 | 类型 | 默认 | 约束 |
|------|------|------|------|
| `id` | string | — | 轨内唯一 |
| `lengthSteps` | u32 | `8` | ≥1；polymeter 核心 |
| `phaseOffsetSteps` | i32 | `0` | |
| `grid` | string | `"8th"` | 文档用；真实调度用 steps |
| `mute` | bool | `false` | |
| `gainDb` | f64 | `0.0` | |
| `steps` | u8[]\|null | `null` | 鼓触发：0/1，长度=`lengthSteps` |
| `notes` | Note[]\|null | `null` | 音高事件 |

**Note**

| 字段 | 类型 | 单位/约束 |
|------|------|-----------|
| `step` | u32 | 层内步，`0..lengthSteps` |
| `pitch` | string\|u8 | `"E1"` 或 MIDI 0–127 |
| `durSteps` | u32 | ≥1 |
| `velocity` | f64 | `0..=1`，默认 `0.8` |

### 7.4 Scene

| 字段 | 类型 | 默认 |
|------|------|------|
| `id` | string | — |
| `launchAtTick` | u64 | — |
| `phasePolicy` | enum | `hard_reset_on_scene` |
| `trackPatches` | object | 可选：轨 → presetId 覆盖 |

---

## 8. 控制面协议

### 8.1 传输（P0）

```bash
# 单事务文件
agb txn apply --project ./my-song --file examples/polymeter_280.json

# dry-run
agb txn apply --project ./my-song --file examples/polymeter_280.json --dry-run

# 离线渲染
agb render offline --project ./my-song --from-tick 0 --to-tick 134400 --out ./my-song/renders/mix.wav

# 查询
agb project snapshot --project ./my-song
agb preset search --tags mathrock,bass --engine synth.moogish
```

stdin 模式（可选）：`agb txn apply --project ./my-song --stdin < txn.json`

### 8.2 Transaction 信封

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `commandId` | string | 是 | 幂等键；UUID 或稳定字符串 |
| `actor` | string | 是 | `agent.*` 或 `human.*` |
| `ifMatchRevision` | u64\|null | 否 | 有则必须等于当前 revision |
| `dryRun` | bool | 否 | 默认 false |
| `quantize` | Quantize | 是 | 生效点 |
| `latePolicy` | enum | 否 | 默认 `reject`：`reject` \| `shift_to_next_legal` \| `apply_asap_with_flag` |
| `ops` | Op[] | 是 | 原子批；失败则全部不应用 |

**Quantize**

```json
{ "type": "tick", "tick": 280 }
```
或 `{ "type": "next_bar" }`（按 `stepsPerQuarter*4` 步为 bar，P0 可先只实现 `tick`）  
或 `{ "type": "immediate" }`（仅允许 `system.panic` / 只读类；写 score 的 immediate **拒绝**）

### 8.3 成功响应

```json
{
  "ok": true,
  "commandId": "cmd_7f8_5f8_4f4_001",
  "revision": 13,
  "effectiveTick": 134400,
  "idempotentReplay": false,
  "warnings": []
}
```

同 `commandId` 重放：返回**首次**响应，且 `idempotentReplay: true`，**不得**再次改状态。

### 8.4 错误码

| code | 含义 |
|------|------|
| `INVALID_JSON` | 解析失败 |
| `SCHEMA_VIOLATION` | 字段/范围非法 |
| `REVISION_CONFLICT` | ifMatch 不匹配；body 含 `serverRevision` |
| `IDEMPOTENCY_PAYLOAD_MISMATCH` | 同 id 不同 payload |
| `LATE_REJECTED` | 量化点已过且 latePolicy=reject |
| `UNKNOWN_TRACK` / `UNKNOWN_PRESET` / `UNKNOWN_ENGINE` | 引用缺失 |
| `ASSET_MISSING` | 样本/波表缺失 |
| `LICENSE_FORBIDDEN` | 资产不可用于 factory/导出策略 |
| `BUDGET_EXCEEDED` | 轨/层/voice/粒子密度超预算 |
| `PANIC_ACTIVE` | 需 clear 后才接受非 panic 写 |
| `UNSUPPORTED_IN_PHASE` | 当前里程碑未实现的 op |

HTTP 映射（未来）：`REVISION_CONFLICT` → 409；其它 4xx/5xx 另表。P0 不必实现 HTTP。

### 8.5 Op 一览（P0 必须实现标 ★）

| op | P0 | 说明 |
|----|----|------|
| `project.setTempo` | ★ | `{ tempoBpm }` |
| `track.upsert` | ★ | |
| `layer.upsert` | ★ | |
| `clip.schedule` | 可后 | 绝对 tick 音符；P0 可用 layer notes 代替 |
| `scene.define` | ★ | |
| `scene.launch` | ★ | 或 define 内含 launchAtTick |
| `preset.search` | P1 | |
| `preset.load` | P1 | |
| `voice.setParams` | P1 | |
| `automation.schedule` | P1 | `mode: ramp\|step` |
| `render.offline` | ★ | 也可仅 CLI，不走 txn |
| `system.panic` | ★ | 立即；忽略 quantize 写限制 |
| `system.panic.clear` | ★ | |

---

## 9. 音色引擎

### 9.1 法律边界

- 命名：`*style` / `*ish` / `inspired`。  
- **禁止**分发 Roland/Moog/Yamaha 原厂样本。  
- 808：模拟参数模型（可参考 DAFx14 电路分析**思路**，不承诺复刻）。  
  - https://www.roland.com/master/promos/808-Day-2020/roland_tr-808/  
  - https://www.dafx14.uni-erlangen.de/papers/dafx14_kurt_james_werner_a_physically_informed,_ci.pdf  
- 707：PCM + 包络；规格叙述参考 Roland 公开页（25 kHz / 8-bit 等为**资料**，非必须逐字节模仿）。  
  - https://www.roland.com/global/products/rc_tr-707/  
  - https://articles.roland.com/mod-squad-a-history-of-tr-707-and-tr-727-modifications/

### 9.2 引擎表

| engineId | 类型 | 引入阶段 |
|----------|------|----------|
| `synth.waveform` | 基础波形 OSC（sine/saw/square/tri/noise + PWM） | **P0** |
| `drum.analog808style` | 模拟鼓模型 BD/SD/CH/OH 至少 | **P1** |
| `drum.pcm707style` | 合法 PCM kit | **P1** |
| `synth.subtractive` | OSC→Filter→Amp | **P1** |
| `synth.moogish` | 减法 + ladder-inspired（深化 P2） | **P1 预置 / P2 滤波** |
| `synth.wavetable` | 波表 + mipmap | **P2** |
| `synth.fm` | FM 与 **PM 分字段** | **P2** |
| `synth.granular` | 粒子 | **P2–P3** |
| `synth.pluck_mathrock` | pluck 感（≠真吉他） | **P2** |

### 9.3 统一 Voice 接口（逻辑）

```text
note_on(pitch, vel, tick)
note_off(pitch, tick)
set_param(path, value)      # 经自动化或事务
steal(policy)
render(buffer_f32_interleaved)  # 无堆分配（实时路径）；offline 可同码
reset()
describe_params() -> ParamMeta[]
```

**ParamMeta：** `path`, `unit`, `min`, `max`, `default`, `kind: continuous|discrete`, `safeMax?`

### 9.4 P0 `synth.waveform` 最小参数

| path | 单位 | 默认 | 范围 |
|------|------|------|------|
| `osc.wave` | enum | `saw` | sine/saw/square/tri/noise |
| `osc.pulseWidth` | 0–1 | `0.5` | square 有效 |
| `amp.attackMs` | ms | `5` | 0–5000 |
| `amp.decayMs` | ms | `200` | |
| `amp.sustain` | 0–1 | `0.7` | |
| `amp.releaseMs` | ms | `100` | |
| `gainDb` | dB | `-6` | |

P0 可用朴素 bandlimited 或 wavetable mip；**不**要求过采样实测数据。

### 9.5 FM vs PM

- `modType: "fm" | "pm"` **必填**，禁止无标注混用。  
- P2：2–4 ops，少量算法，feedback 限幅。  
- DX7 兼容：**非目标**直至单独 RFC。Dexed=GPL-3；msfa=Apache-2.0。

### 9.6 Granular（P2+）

`density`（/s，有上限）、`grainMs`（1–200）、`window`（hann/tukey）、`pitch`/`rate`、`position`/`spray`、`seed`、`sourceAssetId`。缺资产 → `ASSET_MISSING`。同 seed 必须 offline 一致。

### 9.7 预置

**远期计划（非现货）：** 约 400–500 factory。  
**P0：** ≤ **8** 个内置预置（waveform 为主）。  
**P1：** 约 **80–120**（设计目标）。  

**Preset ID：** `preset:{engine}:{role}:{name}@v{major}.{minor}`  
例：`preset:synth.waveform:bass:simple_saw@v1.0`

元数据必填：`engine`, `engineVersion`, `parameters`, `macros`（≤8）, `tags`, `role`, `timbre`, `articulation`, `pitchRange`, `velocityCurve`, `gain`, `seed?`, `assets[]`, `license`, `provenance`, `contentHash`, `cpuClass`（`low|med|high` 为**分级标签**非实测）。

---

## 10. Offline 渲染（首里程碑核心）

### 10.1 CLI

```bash
agb render offline \
  --project ./my-song \
  --from-tick 0 \
  --to-tick 134400 \
  --seed 7 \
  --stems \
  --tail-ms 500 \
  --out-dir ./my-song/renders/run1
```

输出：

- `mix.wav`  
- `stems/{trackId}.wav`（若 `--stems`）  
- `events.json`（实际 noteOn tick 列表）  
- `metrics.json`（峰值、削波计数、是否全静音、hash）  
- `render.json`（输入参数回显：sampleRate、seed、from/to、specVersion）

### 10.2 确定性条件（金样可比）

下列全部固定才可对 hash：

1. 同一 `project.json` content（含 revision 指向的已提交 score）  
2. 同一 `seed`  
3. 同一 `sampleRate`、`from-tick`、`to-tick`、`tail-ms`  
4. 同一 master/轨增益与引擎版本  
5. 浮点：建议 render 内部 f32；导出 PCM16 时使用**文档化的**舍入（例如 round half away from zero）；金样比对应导出文件 SHA-256  

`tail-ms`：渲染窗结束后继续算 release，避免截断爆音。

### 10.3 安全

- master 默认 −6 dBFS；可选 soft clip + 硬限 0 dBFS  
- `system.panic`：立即将输出置零、清实时队列；score 保留  
- voice 预算：全局 **≤32**（设计预算）  
- 超预算 → `BUDGET_EXCEEDED` 或 steal（按 `stealPolicy`）

---

## 11. 可复制 JSON 样例（一致时间线）

约定（三样例共用）：

- `tempoBpm = 120`, `ppqn = 960`, `stepsPerQuarter = 2` → `ticksPerStep = 480`  
- step 280 → tick `280 * 480 = 134400`  
- LCM 相遇 = **280 steps** = **134400 ticks**

### 样例 1 — 建立 polymeter 工程并在 280 step 定义 scene

文件：`examples/polymeter_280.json`

```json
{
  "commandId": "cmd_7f8_5f8_4f4_001",
  "actor": "agent.composer.a",
  "ifMatchRevision": 0,
  "dryRun": false,
  "quantize": { "type": "tick", "tick": 0 },
  "latePolicy": "reject",
  "ops": [
    {
      "op": "track.upsert",
      "trackId": "drums",
      "engineId": "synth.waveform",
      "channel": 0,
      "gainDb": -6.0
    },
    {
      "op": "layer.upsert",
      "trackId": "drums",
      "layerId": "kick78",
      "lengthSteps": 7,
      "grid": "8th",
      "steps": [1, 0, 0, 1, 0, 0, 1],
      "notes": [
        { "step": 0, "pitch": "C2", "durSteps": 1, "velocity": 0.9 },
        { "step": 3, "pitch": "C2", "durSteps": 1, "velocity": 0.8 },
        { "step": 6, "pitch": "C2", "durSteps": 1, "velocity": 0.8 }
      ]
    },
    {
      "op": "track.upsert",
      "trackId": "bass",
      "engineId": "synth.waveform",
      "channel": 1,
      "gainDb": -8.0
    },
    {
      "op": "layer.upsert",
      "trackId": "bass",
      "layerId": "riff58",
      "lengthSteps": 5,
      "grid": "8th",
      "notes": [
        { "step": 0, "pitch": "E1", "durSteps": 2, "velocity": 0.85 },
        { "step": 3, "pitch": "B1", "durSteps": 1, "velocity": 0.75 }
      ]
    },
    {
      "op": "track.upsert",
      "trackId": "keys",
      "engineId": "synth.waveform",
      "channel": 2,
      "gainDb": -10.0
    },
    {
      "op": "layer.upsert",
      "trackId": "keys",
      "layerId": "pad44",
      "lengthSteps": 8,
      "grid": "8th",
      "notes": [
        { "step": 0, "pitch": "E3", "durSteps": 8, "velocity": 0.6 }
      ]
    },
    {
      "op": "scene.define",
      "sceneId": "meet_280",
      "launchAtTick": 134400,
      "phasePolicy": "hard_reset_on_scene"
    }
  ]
}
```

### 样例 2 — 渲染至相遇点（含 tail）

文件：`examples/render_p0.json`（若走 txn；或等价 CLI 参数）

```json
{
  "commandId": "cmd_render_meet_280",
  "actor": "agent.composer.a",
  "ifMatchRevision": 1,
  "dryRun": false,
  "quantize": { "type": "tick", "tick": 0 },
  "latePolicy": "reject",
  "ops": [
    {
      "op": "render.offline",
      "fromTick": 0,
      "toTick": 134400,
      "tailMs": 500,
      "seed": 7,
      "stems": true,
      "outDir": "renders/meet_280"
    }
  ]
}
```

**一致性断言：** `toTick` 必须为 `280 * (ppqn/stepsPerQuarter) = 134400`。`events.json` 中三层在 step 模 length==0 的全局 step 280 应对齐（scene 点）。

### 样例 3 — 未来量化换预置 + 截止频率 ramp（P1 形态；P0 可标 UNSUPPORTED）

文件：`examples/preset_load.json`

```json
{
  "commandId": "cmd_preset_mathrock_01",
  "actor": "agent.composer.a",
  "ifMatchRevision": 2,
  "dryRun": false,
  "quantize": { "type": "tick", "tick": 134400 },
  "latePolicy": "reject",
  "ops": [
    {
      "op": "preset.load",
      "trackId": "bass",
      "presetId": "preset:synth.waveform:bass:simple_saw@v1.0",
      "tailPolicy": "release_old"
    },
    {
      "op": "automation.schedule",
      "trackId": "bass",
      "param": "osc.pulseWidth",
      "mode": "ramp",
      "fromTick": 134400,
      "toTick": 138240,
      "from": 0.5,
      "to": 0.2
    }
  ]
}
```

注：`138240 - 134400 = 3840 ticks = 8 steps`（在 120BPM 下为 4 个四分音符）。P0 若未实现 automation，应返回 `UNSUPPORTED_IN_PHASE` 且**整事务失败**（原子性）。

---

## 12. 分期与完成定义

### 12.1 预算（非性能测试结果）

| 项 | 预算 |
|----|------|
| 轨 | ≤8 |
| 层/轨 | ≤4 |
| 全局 voice | ≤32 |
| 场景 | ≤8 |
| P0 预置 | ≤8 |
| P1 预置目标 | ~80–120 |
| 远期 factory | ~400–500 |

### 12.2 P0 — DoD（必须全部满足）

1. `cargo test` 通过（含协议与时钟纯函数）。  
2. 能 `txn apply` 样例 1；幂等重放不双写。  
3. `ifMatchRevision` 冲突不部分应用。  
4. `latePolicy=reject`：量化 tick 已过去则拒绝。  
5. offline 渲染样例 2：**两次同 seed 同输入 → mix SHA-256 相同**。  
6. `events.json` 可断言 LCM：全局 step 280 为三层共同原点。  
7. `system.panic` 后渲染输出近静音（峰值低于阈值阈值，如 −60 dBFS）。  
8. README 说明如何本地运行上述命令（不依赖云）。  

### 12.3 P0 测试计划（自动）

| 测试 ID | 内容 |
|---------|------|
| `T_clock_lcm` | `lcm(7,5,8)==280`；tick 换算 280→134400 |
| `T_txn_idempotent` | 同 commandId 两次 apply，revision 只 +1 一次 |
| `T_txn_conflict` | 错误 revision → `REVISION_CONFLICT`，磁盘工程不变 |
| `T_txn_atomic` | 批中后 op 非法 → 前 op 也不落地 |
| `T_late_reject` | transport 逻辑时间已过目标 tick → reject |
| `T_render_deterministic` | 两次 render hash 相等 |
| `T_render_tail` | tailMs>0 时尾段能量大于截断对照（或长度公式断言） |
| `T_panic` | panic 后 mix 峰值阈值 |
| `T_schema` | 样例 JSON 通过 schema |

### 12.4 P1

808style 最小套件、707style 合法 kit、subtractive/moogish 预置子集、preset.* API、总线 limiter、约 80–120 预置目标、缺资产错误。

### 12.5 P2

wavetable、FM/PM、pluck、automation ramp、预置扩至 ~300、采样率 44.1/48 迁移规则。

### 12.6 P3

granular 深化、预置 ~400–500、可选 cpal 实时（文档可行：cpal 跨平台 I/O）、可选 MCP/HTTP 适配、可选 msfa 研究（仍非 DX7 承诺）。

---

## 13. 非功能需求

| 类别 | 要求 |
|------|------|
| 确定性 | 见 §10.2 |
| 安全 | 限幅、panic、预算、拒绝非法资产 |
| 可测 | 纯函数与金样；无必连网络 |
| 可移植 | macOS/Linux/Windows 以 Rust 目标三元组为准 |
| 文档 | 每个 op 有 schema；错误码表稳定 |
| 许可 | 依赖与采样分册登记；默认宽松自研许可 |

---

## 14. 版本与迁移

- `specVersion` / preset `@vMAJOR.MINOR`：MAJOR 变更需 `migrations[]`。  
- 旧工程打不开时：明确错误，禁止静默扭曲 tick。  

---

## 15. 参考来源（研究中实际查阅）

- Web Audio 调度：https://web.dev/articles/audio-scheduling  
- AudioWorklet：https://developer.mozilla.org/en-US/docs/Web/API/Web_Audio_API/Using_AudioWorklet  
- Tone.js / Offline / MIT：https://tonejs.github.io/ ；https://tonejs.github.io/docs/15.1.22/functions/Offline.html ；https://raw.githubusercontent.com/Tonejs/Tone.js/dev/LICENSE.md  
- cpal：https://github.com/RustAudio/cpal  
- sfizz BSD：https://raw.githubusercontent.com/sfztools/sfizz/develop/LICENSE  
- Faust FAQ 许可：https://faustdoc.grame.fr/manual/faq/  
- TR-808 / 707 公开叙述与 DAFx14：见 §9.1 链接  
- Dexed GPL / msfa Apache / Surge GPL：https://raw.githubusercontent.com/asb2m10/dexed/master/LICENSE ；https://github.com/google/music-synthesizer-for-android ；https://raw.githubusercontent.com/surge-synthesizer/surge/main/LICENSE  

---

## 16. 实现检查清单（给 coding agent）

- [ ] 按 §3 建 Rust workspace（空实现亦可，先协议与时钟）  
- [ ] 实现 Transaction 幂等与 revision  
- [ ] 实现 layer polymeter 调度 + LCM 测试  
- [ ] 实现 `synth.waveform` 最小发音 + offline WAV  
- [ ] 跑通样例 1→2 金样哈希  
- [ ] 再扩展 P1 引擎与预置——**禁止**跳过 P0 堆 DSP  

---

*文档结束。实现以本 SPEC 为准；未列出的花哨功能默认延后。*
