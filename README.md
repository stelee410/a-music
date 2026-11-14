# Agent Groovebox

Agent-first 的 groovebox：AI 现场作曲演奏，引擎确定性渲染。按 [`SPEC.md`](SPEC.md) `0.1.0-spec` 实现：

- **P0**：单一 tick 时钟、polymeter 层、JSON 事务（幂等 / revision / 量化 / 原子）、确定性 seeded 离线 WAV 渲染。
- **P1**：`drum.analog808style`、`synth.subtractive`、`synth.moogish` 三个引擎；`preset.*` / `voice.setParams` / `automation.schedule` / `clip.schedule` 这些 op；47 个原创预置；master soft limiter。
- **AI 演奏者** `agb perform`：LLM（agentllm 网关）逐乐句提交事务，现场模式按墙钟实时推进。
- **macOS app** `Agent Groovebox.app`：深色舞台，polymeter 圆环可视化，AI 解说，唱片架。

引擎本身不需要网络；只有 AI 演奏需要访问 LLM。

## `.agbsong`：可演奏的音乐文件格式

[`FORMAT.md`](FORMAT.md) 定义了一种交换格式（草案 `0.1.0`）：一个 `.agbsong` 是一首还没有定稿成音频的音乐，包含乐谱、音色设计、表情、采样与波表、效果器、叠加频率、视觉和封面。任何 AI Agent 写出符合格式的文件，播放器就能演奏，并在播放时移调、变速。

```bash
agb song new      --out my.agbsong --title "我的曲子"       # 生成一个可播放的目录形态起点
agb song validate examples/first-light.agbsong             # 校验格式，并报告本播放器会跳过什么
agb song info     examples/first-light.agbsong
agb song render   examples/first-light.agbsong --out-dir /tmp/fl --stems --transpose 2 --bpm 96
agb song pack     examples/first-light.agbsong --out /tmp/first-light.agbsong   # 目录 → zip，字节可复现
agb song unpack   /tmp/first-light.agbsong --out-dir /tmp/unpacked
```

目前的实现范围：格式的解析、校验、打包已完整；离线渲染支持 `core`（loop、clip、内置引擎、变速）和音色参数自动化，其余特性（表情、采样、波表、效果器、叠加频率、视觉）会被跳过并在输出的 `skipped` 里逐条列出，详见 `FORMAT.md` §14。

macOS app 可以播放 `.agbsong`：首页左下角“打开 .agbsong”，或 `open -a "Agent Groovebox" --args /path/to/song.agbsong`。播放时调音台、主效果、打击垫、疗愈声场照常可用；改速度、移调和与 AI 对话还没接到文件播放上。示例：`examples/first-light.agbsong`（冥想电子）、`examples/when-the-mist-lifts.agbsong`（原创爵士抒情曲）。

## macOS app

```bash
app/build-app.sh            # → dist/Agent Groovebox.app（构建引擎 + SwiftUI app，ad-hoc 签名）
app/build-app.sh --install  # 同时复制到 /Applications
```

- 第一次用 AI 演奏时，会从 `~/Dev/AI-passport/private/agentllm.key` 把 key 导入 macOS 钥匙串。之后在 **设置** 里可以修改 key、网关地址和模型回退顺序（默认 `c-music-express,gemini-3-flash-preview,gpt-5-mini,deepseek-v4-pro`）。
- 每次重新编译后签名会变，macOS 会再询问一次钥匙串授权，点"始终允许"即可。
- **现场模式**：渲染始终比播放位置提前 2 小节。AI 的每个乐句提交到下一个还没渲染的小节线。AI 来晚了，乐队就循环当前乐谱等它，界面上会显示"晚到 N 小节"。
- **录音室模式**：先按顺序写完所有乐句，再播放。
- **离线演示**：用内置的 mock 演奏者，不调用 AI。
- 演奏记录保存在 `~/Music/Agent Groovebox/<时间戳>/`，包括 `project.json`、`perform.json`、`llm/`（每次 LLM 原始回复）、`renders/full/mix.wav` 和各轨 stems。可以在"唱片架"里重播，或者接着演。

## 人与 AI 一起演奏

整体以 AI 演奏为主体：AI 负责作曲和编曲，人在台边指挥、调音，并且可以自己参与演奏。

| 分工 | 谁 | 何时生效 | 实现 |
|---|---|---|---|
| 音符、配器、音色、结构、自动化 | AI | 下一乐句 | LLM 发出的事务 |
| 对话：人说话，AI 回答并用音乐回应 | 人 → AI | AI 回复后，落在下一个空闲小节 | `chat` 命令会作废正在准备的乐句，带上这句话重新请求 |
| BPM | 人（AI 被请求时也可以改） | 下一小节 | `human.listener` 事务写入 tempo map |
| 收尾 | 人 | AI 立刻写终章 | `finish` 命令 |
| 每轨推子 / 静音 | 人 | 即时 | 每个小节按轨道分别渲染（stems），app 端实时混音 |
| 滤波、混响、延迟、调音（A=432…446） | 人 | 即时 | AVAudioEngine：EQ、TimePitch、Delay、Reverb |
| 打击垫：5 组 × 8 个（鼓组 / 打击乐 / 颂钵 / 音叉 / 钟与氛围），每个垫子可调音高、衰减、音色、音量，每组可调空间混响；可拖入自己的采样；对齐节拍可选 | 人 | 即时：鼠标按下就触发，低延迟采样器，I/O 缓冲 256 帧 | 默认音色由 `agb oneshot` 用引擎渲染，全部原创 |
| 疗愈声场：频率发生器（Solfeggio）、双耳节拍（δθαβγ）、白/粉/褐噪声 | 人 | 即时，音乐不播放时也能单独使用 | 实时立体声合成节点 |
| 录音（你听到的全部声音） | 人 | 即时 | 在限幅器上挂 tap，写入 `live-*.caf` |

人的每个动作都会汇总成 `human_state` 发给 AI，例如"鼓推子 50%，θ 6Hz 双耳节拍，528Hz 频率发生器，A=432"。prompt 要求 AI 不要和人的混音对着干；疗愈发声器开着时，和声要与发声器的频率协和，音乐保持舒缓。疗愈声音只作放松辅助，不是医疗用途。

`agb perform` 从 stdin 读取 NDJSON 命令：

```json
{"type":"chat","text":"再安静一点"}
{"type":"set_tempo","bpm":90}
{"type":"human_state","state":{"binauralBeat":{"carrierHz":200,"beatHz":6}}}
{"type":"finish"}
```

它新增的事件有 `chat`（`who: listener|ai`）、`change.answer`（这一句是在回应人）、`human_change`、`finishing`。`chunk` 事件里多了 `stems`（每轨一个 WAV）和 `tempoMap`。对话记录保存在 `perform.json` 的 `dialogue` 字段。

无界面自检：`AGB_HEADLESS_CHECK=/tmp/report.json app/.build/release/AgentGroovebox`。它不弹窗口，扬声器也不出声，会跑一场 mock 演奏，覆盖对话、BPM、打击垫、疗愈和录音，最后分析录音，并把舞台界面渲染成 `report-stage.png`。

## AI 演奏（命令行）

```bash
export AGENTLLM_API_KEY=$(cat ~/Dev/AI-passport/private/agentllm.key)
agb perform --project ./night --brief "深夜城市的数学摇滚电子乐" --phrases 8 --bars 8 --realtime
agb perform --project ./night --continue --phrases 4      # 接着演
agb perform --project ./demo --mock --phrases 4           # 离线 mock
```

stdout 是 NDJSON 事件流：`session` / `phrase_request` / `llm_reply` / `txn_rejected` / `change` / `chunk` / `phrase_skipped` / `done` / `fatal`。每个 `chunk` 是一个小节的 float32 WAV。渲染从 tick 0 累积进行，只输出新增的小节，所以相邻 chunk 无缝衔接（有测试逐样本验证）。

LLM 只在控制面使用（SPEC §1）：它输出 `{title, commentary, bars, ops}`，ops 里的 tick 都相对乐句起点。agent 负责换算成绝对 tick，用 `ifMatchRevision` 加量化 tick 提交事务。引擎拒绝时，把错误码反馈给 LLM，最多重试 3 次；仍然失败就跳过这一句，乐谱继续循环。

客户端会容忍几种常见的回复瑕疵：markdown 围栏、尾逗号、网关在流分片边界插入的换行（实测 agentllm 会在字符串中间和数字中间插 `\n`），以及漏写 `preset:` 前缀。track 只给 `presetId` 时，引擎会从预置推断 `engineId`。

## 快速开始

需要 Rust ≥ 1.75（`rustup` 安装即可）。

```bash
cargo build --release
alias agb=./target/release/agb          # 或：cargo install --path crates/agb-cli

# 样例 1：建立 polymeter 工程（7 / 5 / 8 步三层），在 step 280 定义 scene
agb txn apply --project ./my-song --file examples/polymeter_280.json

# 同一事务再发一次 → idempotentReplay: true，revision 不变
agb txn apply --project ./my-song --file examples/polymeter_280.json

# 只校验不落盘
agb txn apply --project ./my-song --file examples/polymeter_280.json --dry-run

# 样例 2：经事务渲染到相遇点（含 500ms tail），输出 renders/meet_280/
agb txn apply --project ./my-song --file examples/render_p0.json

# 样例 3：P1 形态（preset.load + automation）→ UNSUPPORTED_IN_PHASE，整事务不应用
agb txn apply --project ./my-song --file examples/preset_load.json

# 直接用 CLI 离线渲染（不走事务、不改 revision）
agb render offline --project ./my-song --from-tick 0 --to-tick 134400 \
    --seed 7 --stems --tail-ms 500 --out-dir ./my-song/renders/run1
agb render offline --project ./my-song --out ./my-song/renders/mix.wav   # 默认渲染一个完整 LCM 周期

# 查询
agb project snapshot --project ./my-song [--at-tick 134400]
agb preset search --tags mathrock,bass --engine synth.waveform
agb engine list
agb engine params synth.waveform

# stdin 模式
agb txn apply --project ./my-song --stdin < txn.json
```

所有命令向 stdout 打印**一个 JSON 文档**。退出码：`0` 成功，`1` 错误响应（含稳定错误码），`2` 参数用法错误。

### 验证两次渲染哈希一致（DoD 5）

```bash
agb render offline --project ./my-song --to-tick 134400 --seed 7 --out-dir ./my-song/renders/a
agb render offline --project ./my-song --to-tick 134400 --seed 7 --out-dir ./my-song/renders/b
shasum -a 256 ./my-song/renders/{a,b}/mix.wav
```

### panic（DoD 7）

```bash
echo '{"commandId":"panic_1","actor":"human.me","quantize":{"type":"immediate"},"ops":[{"op":"system.panic"}]}' \
  | agb txn apply --project ./my-song --stdin
# 此后渲染输出全零（metrics.json: nearSilent=true）；写 score 的事务返回 PANIC_ACTIVE
echo '{"commandId":"clear_1","actor":"human.me","quantize":{"type":"immediate"},"ops":[{"op":"system.panic.clear"}]}' \
  | agb txn apply --project ./my-song --stdin
```

### 测试

```bash
cargo test --workspace
```

## 渲染输出

| 文件 | 内容 |
|------|------|
| `mix.wav` | 单声道，默认 PCM16（`--format f32` 可选 float32） |
| `stems/{trackId}.wav` | `--stems` 时每轨一个（已乘 master 增益、已限幅） |
| `events.json` | 每个 noteOn（tick / globalStep / layerStep / pitch …）、`layerCycleStarts`、`commonOrigins`、`sceneLaunches` |
| `metrics.json` | 峰值 / dBFS / RMS / 削波计数 / 是否静音 / tail 能量 / 偷 voice 次数 / 各文件 SHA-256 |
| `render.json` | 输入回显：sampleRate、seed、from/to、tailMs、specVersion、engineVersion、revision … |

## 工程结构

```
crates/
  agb-cli       二进制 `agb` + 命令实现（lib）+ P0 验收测试
  agb-protocol  Transaction / Op / 响应 / 错误码 / Schema 导出
  agb-engine    时钟纯函数、事务（幂等·revision·量化·late·原子）、调度器、存储
  agb-score     Project/Track/Layer/Note/Scene、timeline 及其物化
  agb-dsp       Voice 接口、synth.waveform、确定性数学
  agb-render    离线块渲染、voice 预算与 steal、master/限幅、WAV 编码
  agb-presets   预置目录（factory/presets/*.json 编译期嵌入）
  agb-assets    资产引用、哈希、许可登记与 factory 策略
  agb-song      `.agbsong` 文件格式：数据模型、目录/zip 容器、校验、转成引擎工程
  agb-agent     AI 演奏者：agentllm 客户端（模型回退）、prompt、乐句循环、mock 演奏者
app/            SwiftUI macOS app（Theme / OrbitView / ScoreRiver / Session / ChunkPlayer）+ build-app.sh
schemas/        transaction / project / preset JSON Schema（由代码导出）
factory/        presets（54 个原创 MIT 参数预置）、kits（空）、licenses
examples/       SPEC §11 三个样例（逐字复制）
tests/golden/   金样 WAV SHA-256 清单
```

## P0 完成定义对照（SPEC §12.2 / §12.3）

| DoD / 测试 | 位置 |
|------------|------|
| `T_clock_lcm` | `crates/agb-engine/src/clock.rs` |
| `T_txn_idempotent` / `T_txn_conflict` / `T_txn_atomic` / `T_late_reject` | `crates/agb-engine/src/tests.rs` |
| `T_render_deterministic` / `T_render_tail` / `T_panic` | `crates/agb-render/src/lib.rs` |
| `T_schema`、schema 与代码同步、金样哈希、events.json LCM、CLI 冒烟 | `crates/agb-cli/tests/p0.rs` |

## 实现中做出的决定（规范未写死之处）

规范之外的行为都在这里写明；需要改动时以 SPEC 为准再调整。

**时间线与量化**
- 每个成功的写事务把 score 变更作为一条 `timeline` 记录（`effectiveTick` + `revision`）写进 `project.json`。tick `T` 处生效的 score = 按 `(effectiveTick, revision)` 顺序应用所有 `effectiveTick ≤ T` 的记录。`tracks` 字段是物化后的 head 视图，方便阅读；**权威数据是 `timeline`**。手写的、只有 `tracks` 没有 `timeline` 的工程，加载时会自动生成一条 tick 0 的 genesis 记录。
- 写入过去的 tick 时，之后每个变更点的状态都会重新校验，所以往过去插入的变更不能让未来的状态变非法。
- `project.setTempo`：生效 tick 等于 origin 时修改起始速度；否则在该 tick 插入一个 tempo map 点（`project.tempoMap`），之前的音频不受影响。没有变速时，tick→sample 的换算与 P0 逐字节一致，金样 hash 不变。
- `transportTick`（扩展字段）是 late 判定所用的逻辑播放位置，通过 `agb transport set --project … --tick N` 设置；它不是事务，不增加 revision。
- `next_bar` = 不早于 transport 的下一个小节线（`4 × ppqn` tick）。`shift_to_next_legal` 移到同一个小节线；`apply_asap_with_flag` 在 transport 当前位置生效，并返回 `LATE_APPLIED_ASAP` warning。
- `immediate` 只允许 `system.panic*` / `render.offline`；写 score 的 op 用 immediate 会返回 `SCHEMA_VIOLATION`。late 判定只针对写 score 的事务。

**事务**
- 检查顺序：JSON 解析 → schema → 信封 → 幂等重放 → 未实现 op（`UNSUPPORTED_IN_PHASE`）→ `ifMatchRevision` → 量化 / late → 逐个 op → 整条时间线校验 → dry-run 到此为止 → 渲染 → 落盘。任何一步失败，磁盘上什么都不变。
- **每个成功提交的事务 revision 都 +1**，包括只做渲染或 panic 的事务。这与 SPEC 样例的 revision 链一致（样例 2 `ifMatch 1` → 样例 3 `ifMatch 2`）。
- 幂等：`.agb-idempotency/sha256(commandId).json` 保存 payload 哈希和首次响应。失败的事务和 dry-run **不**登记，修正后可以用同一个 `commandId` 重发。
- 有写锁 `.agb.lock`，超过 60 秒视为过期锁；`project.json` 用 tmp + rename 原子写入。
- 新增错误码：`PROJECT_NOT_FOUND`、`SPEC_VERSION_UNSUPPORTED`、`PROJECT_LOCKED`、`IO_ERROR`。`REVISION_CONFLICT` 在顶层和 `error` 里都带 `serverRevision`。

**乐谱语义**
- 层内 `notes` 非空时以 `notes` 为准；只有 `steps` 时，每个 `1` 触发 `C2`（MIDI 36），时长 1 step，力度 0.8。两者同时给出且不一致时返回 warning。
- 音名采用科学音高记法：`C4 = 60`，`E1 = 28`。
- 相位：所有层共享以 `originTick` 为锚点的全局 step 网格，`local = (g − o + phaseOffsetSteps) mod lengthSteps`，其中 `o` 是最近一次已发动的 `hard_reset_on_scene` 场景所在的 step；`free_phase` 场景不改变 `o`。`launchAtTick` 必须对齐 step。
- scene 的 `trackPatches` 从发动时刻起覆盖该轨的 `presetId`。
- Track 扩展字段 `params`：按轨覆盖引擎参数。生效顺序：引擎默认值 ← 预置 ← 轨 `params`。

**DSP / 渲染**
- 单声道。`synth.waveform`：sine / saw（PolyBLEP）/ square + PWM（PolyBLEP）/ tri / noise，线性 ADSR。
- 确定性：振荡器、dB 和音高换算用自写的 `detmath`（只用 IEEE 四则运算），不依赖各平台 libm，所以金样哈希可以跨平台比对。噪声按 `(seed, trackId, noteId)` 设种子；同一个 seed 得到同样的 PCM。
- PCM16：先限幅到 [-1, 1]，再 `× 32767`，四舍五入时 0.5 远离零取整（round half away from zero）。
- 全局 voice 上限 32；超出时按工程的 `stealPolicy`（`oldest` / `quietest` / `same_pitch`）直接硬切被偷的 voice。
- master 默认 −6 dB，然后在 0 dBFS 硬限幅，同时统计削波次数。
- 窗口为 `[fromTick, toTick)`；到 `toTick` 仍按着的音在 `toTick` 释放，release 在 `tailMs` 内渲染完。默认 `tailMs = 500`，默认 `toTick` 为一个完整的 LCM 周期。
- panic 生效期间渲染输出全零，`events` 清空，score 保留。

**预置**：P0 内置 8 个 `synth.waveform` 预置。`contentHash` = 去掉 `contentHash` 字段后的规范 JSON（键排序、紧凑格式）的 SHA-256，由测试校验。改动预置后，用下面的命令打印新哈希：
`cargo test -p agb-presets print_content_hashes -- --ignored --nocapture`

**维护命令**
- 更新 schema：`cargo run -p agb-cli -- schema export --out-dir schemas`（测试会检查 schema 与代码是否同步）
- DSP 有意改动后更新金样：先提升 `agb-dsp` 的 `ENGINE_VERSION`，再运行 `AGB_UPDATE_GOLDEN=1 cargo test -p agb-cli golden`

## synth.modal（模态合成）

用于模拟敲击后持续共振的物体：颂钵、水晶钵、音叉、管钟、寺钟、锣。每种模型都有各自的非谐泛音比例，例如颂钵是 1 : 2.71 : 5.15 : 8.27…，音叉是基频加上快速衰减的 6.27 倍"金属声"。每个泛音由一对略微失谐的模态组成，产生颂钵特有的拍频；高次泛音衰减得更快；敲击瞬间用一段带通噪声模拟槌头。

- 参数：`model`、`freqHz`（大于 0 时直接按这个频率发声，例如 528）、`decay`、`brightness`、`beat`、`strike`、`gainDb`。
- 预置 7 个：颂钵、深钵、水晶钵、音叉、管钟、寺钟、风锣。
- AI 作曲时也可以使用这个引擎。

鼓组新增：冲击底鼓（35）、叮叮镲（51 / 53 bell）、铃鼓（54）、手鼓（60/61）、康加（62/63/64）、沙锤（70）、响棒（75）。所有鼓件的电平都已平衡到 −5 dBFS 左右。

## P1 补充说明

- `drum.analog808style`：按音高选择乐器（36 底鼓、38 军鼓、39 拍手、42 闭镲、46 开镲、41–50 嗵鼓、49 镲、56 牛铃），闭镲会掐断开镲。全部是参数化合成，不含采样。
- `synth.subtractive`：2 个 OSC 加噪声，经 TPT SVF（lp/hp/bp）、滤波包络和放大包络。`synth.moogish` 是同一个 voice 换成受 ladder 启发的 24dB 低通加 drive，名字只是风格描述，不是对任何厂商产品的复刻。
- `automation.schedule`：每 64 个样本求值一次，作用于正在发声的 voice 和新触发的 voice。`ramp` 在 `[fromTick, toTick]` 区间线性变化，之后保持终值；`step` 在区间内取 `from`，之后取 `to`。
- `preset.load`：同时切换引擎和预置，并清空轨上的 `params`。`tailPolicy: cut_old` 会在生效 tick 硬切旧 voice。
- 扩展 op `project.setMix`（`masterGainDb` / `masterLimiter: hard|soft`）。AI 演奏使用 soft limiter。
- 预置共 47 个，未达到 SPEC 中 80–120 的设计目标。
- P0 的金样 hash 在 P1 之后仍然逐字节一致。

## 尚未实现（按 SPEC 分期）

- `drum.pcm707style`：需要一套合法授权的 PCM kit，目前返回 `UNSUPPORTED_IN_PHASE`。
- P2+：wavetable、FM/PM、pluck、granular、cpal 实时音频输出、HTTP/MCP 适配。app 目前播放的是引擎离线渲染出的音频块。

许可：MIT（见 `LICENSE`）。不嵌入 GPL 代码，也不包含原厂样本。
