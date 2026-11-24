# Agent Groovebox

Agent-first 的 groovebox：AI 现场作曲演奏，引擎确定性渲染。按 [`SPEC.md`](SPEC.md) `0.1.0-spec` 实现：

- **P0**：单一 tick 时钟、polymeter 层、JSON 事务（幂等 / revision / 量化 / 原子）、确定性 seeded 离线 WAV 渲染。
- **P1**：`drum.analog808style`、`synth.subtractive`、`synth.moogish` 三个引擎；`preset.*` / `voice.setParams` / `automation.schedule` / `clip.schedule` 这些 op；47 个原创预置；master soft limiter。
- **AI 演奏者** `agb perform`：LLM（agentllm 网关）逐乐句提交事务，现场模式按墙钟实时推进。
- **macOS app** `Agent Groovebox.app`：深色舞台，polymeter 圆环可视化，AI 解说，唱片架。

引擎本身不需要网络；只有 AI 演奏需要访问 LLM。

## `.agbsong`：可演奏的音乐文件格式

一首曲子是**一个 `.agbsong` 文件**（zip 压缩包），可以直接发给别人、双击打开。`examples/*.src/` 是创作用的源文件夹，`agb song pack` 把它打成同名的 `.agbsong`。

[`FORMAT.md`](FORMAT.md) 定义了一种交换格式（草案 `0.1.0`）：一个 `.agbsong` 是一首还没有定稿成音频的音乐，包含乐谱、音色设计、表情、采样与波表、效果器、叠加频率、视觉和封面。任何 AI Agent 写出符合格式的文件，播放器就能演奏，并在播放时移调、变速。

```bash
agb song new      --out my-song.src --title "我的曲子"     # 生成一个可播放的源文件夹
agb song validate examples/first-light.agbsong             # 校验格式，并报告本播放器会跳过什么
agb song info     examples/first-light.agbsong
agb song render   examples/first-light.agbsong --out-dir /tmp/fl --stems --transpose 2 --bpm 96
agb song pack     my-song.src --out my-song.agbsong           # 源文件夹 → 一个 .agbsong 文件，字节可复现
agb song unpack   /tmp/first-light.agbsong --out-dir /tmp/unpacked
```

做一首曲子的完整规范（目录、流程、音乐与视频的要求、交付标准）见 [`AUTHORING.md`](AUTHORING.md)。按它制作的曲目放在 `songs/`：`songs/neon-strut.agbsong`（《霓虹快步》，原创 funk，降 B 大调，104 BPM，带按曲式逐小节绘制的舞台视频和给台上乐手看的和弦提示条）和 `songs/slow-spin.agbsong`（《午夜慢转》，原创爵士嘻哈，F 小调，86 BPM，摇摆的十六分）和 `songs/chrome-engine.agbsong`（《铬色引擎》，原创赛博朋克工业 techno，E 小调，132 BPM；汽车引擎声是随文件携带的采样，由播放器的采样音色 `sampler.multi` 演奏；视频把 3D 粒子场景和用 Grok 生成的图片、片段剪在一起）。

另有 `songs/simulated-heart/`（《拟心》，原创数学摇滚，E 小调，144 BPM，四人乐队加一个用英文唱的女声；段落依次是 4/4、7/8、4/4、5/4、11/8、4/4，乐器是合成出来的吉他、贝斯和鼓的采样，人声由歌声引擎 `voice.chimera` 按音符上的歌词唱出）。它的视频是一个 Remotion + three.js 项目（[`songs/simulated-heart/video/`](songs/simulated-heart/video/)），素材是用 Grok 生成的动画风格舞台画面。成品 `.agbsong` 有 287 MB，不在仓库里，按该目录的 README 可以重新做出来。

**歌单**：歌单由你自己维护。在“歌单”里新建、改名、删除歌单，用“添加曲目”把 `.agbsong` 文件加进去；打开一首曲子只是打开这一首，不会改动任何歌单。在歌单里点一首或按 ⌘→ 切到下一首；开着“对齐拍子”时，下一首在后台准备好，从当前曲子的下一条小节线开始，播完自动切歌时紧接最后一小节。

**歌声**：音轨可以唱歌（`FORMAT.md` §8.7）。音色写 `voice.chimera`，音符上写歌词（英文单词，或中文拼音），由一个物理建模的人声唱出来；现场演出时 AI 乐手也可以自己建歌声轨、写词来唱。歌声引擎是 Jameson Hodge 的 [CHIMERA](https://github.com/jamesondh/chimera-vocal-synth)（MIT），第三方组件见 [`THIRD_PARTY.md`](THIRD_PARTY.md)。

**VST 3 插件**：音轨可以交给本机安装的 VST 3 插件发声或做效果（`FORMAT.md` §8.6，目前是 macOS）。`agb plugin list` 列出本机的插件，`agb plugin info <名字>` 列出参数。歌曲里只写用哪个插件和参数，并带一个内置的降级音色，所以没装插件的人也能播放。例子：`examples/fm-keys.agbsong`（电钢琴用免费的 [Dexed](https://asb2m10.github.io/dexed/)）。

**播完之后**：一首曲子播到结尾后，可以“继续创作”（AI 乐手从最后一小节接着往下写，可以继续对话），可以“回到主界面”开始一次新的实时创作，也可以“另存为 agbsong”。另存用的是 `agb song export --project <演出的工程目录> --out <文件>.agbsong`：按实际奏出的音符保存，包含演出过程中的全部改动；重新播放与原演出的声音一致。演出中途换过音色或音量的声部，会按音色拆成几条轨。

目前的实现范围：格式的解析、校验、打包已完整；离线渲染支持 `core`（loop、clip、内置引擎、变速）和音色参数自动化，其余特性（表情、采样、波表、效果器、叠加频率、视觉）会被跳过并在输出的 `skipped` 里逐条列出，详见 `FORMAT.md` §14。

macOS app 可以播放 `.agbsong`：首页左下角“打开 .agbsong”，或 `open -a "Agent Groovebox" --args /path/to/song.agbsong`。也可以在 Finder 里双击 `.agbsong` 直接打开（app 注册了这个扩展名）。装载文件时有一段动画：机器人把写着曲名的磁带放进录音机，按下播放键，音乐随之开始。之后舞台显示文件自带的画面：带视频就默认播放视频（静音，声音始终来自实时合成；视频必须是 16:9 或 4:3），没有视频则显示封面；顶部可在“封面 / 圆环 / 视频”之间切换。视频视图右上角的“全屏”进入全屏播放：视频铺满窗口，底部是和乐手对话的输入框，右侧叠加当前段落的演出文字和乐手的回答（“隐藏文字”可关闭），Esc 退出。

播放时调音台、主效果、打击垫、疗愈声场照常可用。主效果里有 BPM 旋钮和**移调**控件（− / +，按半音，双击数字复原），都在下一小节生效；舞台上显示当前的调（例如降 E 大调移 +2 后显示“F 大调”），鼓不随移调变化。

**播放文件时可以和乐手对话**：在对话框里说话（“鼓停一下”“颤音琴即兴四小节”“慢一点”），AI 用一句话回答，并把改动作为事务提交到还没播放的小节线上。混音、音色、速度这类改动在下一个可用小节生效；改写的音符落在 AI 写它们时对应的小节上（给模型留出回答时间，默认约 30 秒，随实际用时调整）。原文件不会被改写，这次演奏的全部事务和对话保存在工作目录的 `take.json` 里。文件 `performance.agent.allow` 之外的改动会被播放器拒绝。没有 API key 时曲子照常播放，只是不能对话。

为此事务协议新增了两个扩展 op：`project.setTranspose`（`semitones`，从生效 tick 起整体移调，`drum.*` 引擎的轨不动）和 `clip.clear`（`trackId`、可选 `toTick`）：清掉某轨从生效 tick 起、由更早事务排定的 clip 音符，让后来的事务可以改写已经写好的声部。示例：`examples/first-light.agbsong`（冥想电子）、`examples/when-the-mist-lifts.agbsong`（原创爵士抒情曲，带封面和视频）。

## macOS app

```bash
app/build-app.sh            # → dist/Agent Groovebox.app（构建引擎 + SwiftUI app，ad-hoc 签名）
app/build-app.sh --install  # 同时复制到 /Applications
```

- 第一次用 AI 演奏时，会从 `~/Dev/AI-passport/private/agentllm.key` 把 key 导入 macOS 钥匙串。之后在 **设置** 里可以修改 key、网关地址和模型回退顺序（默认 `deepseek-v4.1-flash,c-music-express,gemini-3-flash-preview,gpt-5-mini`）。
- 每次重新编译后签名会变，macOS 会再询问一次钥匙串授权，点"始终允许"即可。
- **现场模式**：渲染始终比播放位置提前 2 小节。AI 的每个乐句提交到下一个还没渲染的小节线。AI 来晚了，乐队就循环当前乐谱等它，界面上会显示"晚到 N 小节"。
- **录音室模式**：先按顺序写完所有乐句，再播放。
- **离线演示**：用内置的 mock 演奏者，不调用 AI。
- 演奏记录保存在 `~/Music/Agent Groovebox/<时间戳>/`，包括 `project.json`、`perform.json`、`llm/`（每次 LLM 原始回复）、`renders/full/mix.wav` 和各轨 stems。可以在"唱片架"里重播，或者接着演。

## 网页播放器

`web/` 是一个不需要服务端的网页播放器：同一个引擎编译成 WebAssembly，在浏览器里现场合成；画面不用视频，由 three.js 按音乐现场画；歌词唱到哪里亮到哪里。

```sh
web/build.sh         # 引擎 → web/agb.wasm（需要 Rust 的 wasm32-unknown-unknown 目标，脚本会自动添加）
web/make-songs.sh    # 把 songs/ 里的曲子转成 web/songs/*.agbw
web/serve.sh         # 打开 http://127.0.0.1:8377/
```

网页上播放的是 `.agbw`，由工具从 `.agbsong` 生成，曲子不需要重做：

```sh
agb song web songs/this-turn.agbsong --out web/songs/this-turn.agbw \
    --instrument "voice=preset:synth.fm:keys:tine_piano@v1.0" --lyric-lines songs/this-turn/form.json
```

- 浏览器里没有 VST：用到插件的音色换成它的降级音色，或者在转换时用 `--instrument 音轨=…` 换一件乐器。换成引擎自带的引擎或预置（如 `preset:synth.fm:keys:tine_piano@v1.0`）时，网页、安卓、Mac 三处听到的是同一个声音；换成**网页乐器**（`web/instruments/`，JavaScript 写的乐器插件，现有电钢琴和拨弦两件）时只有网页播放器能演奏，别处用降级音色。
- 视频不进 `.agbw`：《这一轮》从 23 MB 变成 31 KB。画面由**场景**插件（`web/scenes/`）画。
- 内置音色和采样在网页上的声音与 `agb song render` 逐采样一致。
- 网页版只播放：移调变速、与 AI 乐手对话、保存和录制仍然在 macOS app 里。

格式、乐器插件和场景插件的接口见 [AGBW.md](AGBW.md)。

## 双屏掌机上的原生 app（Android）

`android/` 是为双屏安卓掌机写的原生 app（Kotlin + OpenGL ES，引擎是同一份 Rust 代码编成的 `libagb.so` 和 `agb` 程序）。它播放和网页播放器一样的 `.agbw`，在设备上现场合成；也能像 Mac 版那样让 AI 演奏、和 AI 合奏。开发和测试用的是 Retroid Pocket Duo Lite（骁龙 665，上屏 1920×1080，下屏 1280×960 可触摸，自带手柄，Android 15）。

- **上屏是舞台**：场景（“轨道”“天际线”，和网页版是同两个）和唱到哪亮到哪的歌词，没有别的文字。
- **下屏是控制台**，分页：
  - **乐谱**：音轨列表和往前滚动的音符。播放曲目时点音轨静音、拖动音符跳转；演奏时左右拖音轨名是推子（即时生效），点一下静音。
  - **jam**：一块用手指演奏的触摸板。左右是音高（吸附到所选的调和调式上，跨两个八度），上下是滤波器开合；可选 21 种乐器（引擎的预置，含四套鼓）、9 种节奏（连奏、四分到十六分、三连音、附点、切分、两种琶音；鼓组则是按住即奏，从左到右由疏到密）、12 个调、10 种调式。四条音轨的循环录音，1/2/4/8 小节，可叠录、静音、清除，有节拍器；录下的是音符而不是声音，所以改速度后循环照样对拍。第一次打开时取当前曲子的调、调式和速度，可以跟着曲子一起弹。手柄：按住 A 时左摇杆就是手指，X 录、Y 放/停、B 清除、L2/R2 换轨、十字键左右换乐器、上下换节奏。它的声音在引擎里实时合成（`crates/agb-android/src/jam.rs`），走安卓的低延迟输出（AAudio），不经过 app 的其他声音通道。
  - **对话**（演奏时）：你说的话、乐手的回答和它对音乐做的改动；有快捷语，也可以用屏幕键盘打字。
  - **调音台**（演奏时）：滤波、混响、延迟、总音量、录音（WAV，存到 `recordings/`），以及每条音轨的推子和静音。
  - **打击垫**：和 Mac 版相同的 5 组 × 8 个（鼓组、打击乐、颂钵、音叉、钟与氛围），声音由引擎在首次启动时渲染。每个垫子可调音高（±12 半音）、衰减、音色、音量，每组可调空间混响，设置会记住。把自己的 WAV 按垫子的文件名（如 `kit-kick.wav`、`bowl-c.wav`）放进 `Android/data/dev.agb.duo/files/pads/`，下次启动就替换那个垫子。
  - **疗愈**：频率发生器（40–1000 Hz 连续可调，另有 Solfeggio 预设）、双耳节拍（节拍 0.5–40 Hz、载波 100–500 Hz，显示左右耳各是多少赫兹）、白/粉/褐噪声；没有音乐时也能单独用。
  - **调音 A4**（在疗愈页，426–454 Hz，有 A=432 / A=440 键）：整个乐队的音高一起移动，速度不变；打击垫和疗愈声不受影响。做法是把声音按比例放慢或加快重放，同时让引擎按相反比例调整速度。
  - 所有推子都能用**右摇杆**操作：上下换控件，左右微调（推得越轻越细）；触摸一个推子也会把焦点给它。
  - 上面还有三个浮层：**曲目**（也就是播放列表，底下有三个开关：自动下一首、小节线切歌、循环。开着“小节线切歌”时，换歌不会断：下一首先在后台备好，在当前曲子的下一条小节线上无缝切入；曲子快结束时下一首接在最后一小节之后）、**AI**（写一句想听什么让 AI 演奏，或者和乐手合奏当前曲目）、**设置**（API Key、网关地址、模型顺序）。
- **AI 演奏和合奏**与 Mac 版是同一套：app 把引擎的命令行程序（`agb perform` / `agb song play`）打进安装包来运行，逐小节收分轨，在 app 里实时混音。演奏结束后可以“再演 4 句”或“另存为 agbsong”（存到 `exports/`）。对乐手说话、改速度、移调（合奏时）、收尾、暂停都和 Mac 版一样；你动过的推子、效果、打击垫和疗愈声会告诉乐手。
- **API Key**：在“设置”里输入；或者把密钥存成 `agentllm.key` 放进设备的 `Android/data/dev.agb.duo/files/`，app 启动时导入并删除该文件。没有密钥时可以勾“离线演示”，合奏时曲子照常播放但没有人回应。
- **手柄**：L1/R1 翻页（所有页面都是），A 播放/暂停，X 切换场景，L2/R2 变速，十字键上下移调，左摇杆让舞台画面倾斜，右摇杆调推子，按下摇杆开关歌词。播放曲目时十字键左右按小节跳转、Y 曲目列表（十字键选、A 确认、B 关闭）、SELECT 回到原调原速；上一首/下一首在屏幕上和曲目列表里。在打击垫页，十字键和 X Y B A 就是八个垫子，L2/R2 换组。
- 只有一块屏时，控制台叠在舞台画面的右下角。

和 Mac 版相比还没有的：打击垫对齐节拍、把舞台录成视频、唱片架（重听以前的演奏）、多个命名的播放列表（现在只有“全部曲目”这一个）、合奏时的小节线切歌。合奏时舞台上不显示歌词。

```sh
android/build.sh            # 引擎（arm64）+ 曲子 + APK：android/app/build/outputs/apk/release/app-release.apk
android/build.sh install    # 再装到插着的设备上并启动
```

需要 Android SDK 和 NDK、JDK、Gradle，以及 `rustup target add aarch64-linux-android`。曲子取自 `web/songs/*.agbw`（没有就先运行 `web/make-songs.sh`）并打进 APK；自己的 `.agbw` 放到设备的 `Android/data/dev.agb.duo/files/songs/`。

为了在掌机的处理器上跟得上，app 里的歌声用“轻量歌手”（内部按 32 kHz 模拟再升到 48 kHz，16 kHz 以上没有内容，开销约一半），歌声和其余音轨在不同的核上同时渲染。网页乐器（JavaScript）在这里不可用，用到它的音轨换成降级音色；要三端一致，就用引擎自带的乐器（`synth.fm` 等）。播放曲目时移调、变速、静音会从当前小节重新合成，带歌声和大量采样的曲子要等几秒。合奏带歌声的曲子时，这台掌机按小节出分轨的速度只有约 1 倍实时，会断续。

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
  agb-voice     歌声：把音符上的歌词变成音节（英文词典、中文拼音），交给 CHIMERA 唱
  agb-plugin    VST 3 插件宿主：找到并装载本机安装的插件，送音符和参数，取回音频；网页乐器的接口
  agb-song      `.agbsong` 文件格式：数据模型、目录/zip 容器、校验、转成引擎工程
  agb-agent     AI 演奏者：agentllm 客户端（模型回退）、prompt、乐句循环、mock 演奏者
  agb-web       引擎的 WebAssembly 入口（网页播放器用）
  agb-android   引擎的 JNI 入口（Android app 用）
web/            网页播放器：页面、引擎胶水、网页乐器（instruments/）、场景（scenes/）、脚本和 Node 测试
android/        双屏安卓掌机上的原生 app（Kotlin）：舞台、控制台、手柄，和 build.sh
app/            SwiftUI macOS app（Theme / OrbitView / ScoreRiver / Session / ChunkPlayer）+ build-app.sh
schemas/        transaction / project / preset JSON Schema（由代码导出）
factory/        presets（58 个原创 MIT 参数预置）、kits（空）、licenses
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
