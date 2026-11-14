# 曲目开发规范

这份规范说明怎样为 Agent Groovebox 做一首曲子：从构思到一个可以双击打开、可以拿去现场演出的 `.agbsong` 文件。它面向写曲子的 AI Agent 和人。

- [`FORMAT.md`](FORMAT.md) 规定文件里**可以写什么**。
- 本文规定我们**怎么做一首曲子**，以及交付前必须达到的标准。
- 参照实现：[`songs/neon-strut/`](songs/neon-strut/)（《霓虹快步》，funk，降 B 大调），它完全按本文制作。

文中“必须”是硬性要求，不满足不能提交；“应当”是默认做法，偏离时在曲目说明里写明原因。

## 1. 目录与命名

```
songs/
  <slug>.agbsong            交付物：一个文件，提交到 git
  <slug>/
    make_song.py            生成 <slug>.src/song.json 和 form.json
    make_video.py           生成 <slug>.src/visuals/<slug>.mp4
    form.json               曲式和和弦表（由 make_song.py 写出，make_video.py 读取）
    <slug>.src/             源文件夹
      song.json
      cover.svg
      visuals/<slug>.mp4    构建产物，不提交（已在 .gitignore 里）
```

- `slug` 用小写英文和连字符，例如 `neon-strut`。
- 源文件夹必须以 `.src` 结尾，不能叫 `.agbsong`；`.agbsong` 只能是打包后的单个文件。
- 一首曲子一个生成脚本。**不手改 `song.json`**：要改音乐就改 `make_song.py` 再重新生成，这样每个音符都有出处，别的 Agent 也能接着改。
- 脚本只用 Python 标准库；视频脚本另外需要 Pillow 和 ffmpeg。
- 脚本必须是确定性的：固定随机种子，同一份脚本两次运行得到相同的 `song.json`。

## 2. 制作流程

1. **写曲式表**。先在 `make_song.py` 顶部列出段落：id、名字、小节数、这一段要发生什么。音乐、视频、AI 乐手的提示都从这张表来。
2. **写各声部**，生成 `song.json`。
3. **校验**：`agb song validate <slug>.src`。
4. **渲染并检查数字**：`agb song render <slug>.src --out-dir <dir> --stems`，看 `metrics.json`（见 §8）。
5. **做封面**（§6）。
6. **做视频**（§7），然后再运行一次 `make_song.py`，让 `song.json` 里的视频哈希更新。
7. **打包**：`agb song pack <slug>.src --out songs/<slug>.agbsong`。
8. **在 app 里打开**，完整听一遍、看一遍。
9. **跑测试并提交**（§9）。

## 3. `song.json` 的必填内容

| 字段 | 要求 |
|------|------|
| `meta.title` | 中文名加英文名，例如 `霓虹快步 Neon Strut` |
| `meta.artists` | 写明作者；AI 作者写 `kind: "ai"` 和 `model` |
| `meta.description` | 一两句话：风格、调、速度、曲式 |
| `meta.tags`、`meta.license`、`meta.created` | 必填。许可默认 `CC-BY-4.0` |
| `meta.cover` | 必须有封面 |
| `time` | `ppqn: 960`；`bpm`；`meter`；`stepsPerQuarter`：十六分律动用 4，三连音或摇摆用 12；`lengthTicks` 必填，且是整小节 |
| `tuning.key`、`tuning.scale` | 必填。播放器靠它显示当前的调和移调后的调 |
| `seed` | 必填 |
| `sections` | 必须从第 1 小节到最后一小节连续覆盖全曲，不留空、不重叠 |
| `performance` | 现场用的曲子必填，见 §5 |

`requires` 只写 `core`（需要时加 `automation`）。播放器还没实现的特性（`fx`、`overlay`、`expression`、`sampler`、`wavetable`、`pan`）可以写，但只能列在 `uses` 里，而且曲子不能依赖它们才成立：去掉它们之后必须仍然是一首完整的音乐。`agb song validate` 的 `player.skipped` 会列出被跳过的部分，交付时应当为空，或在曲目说明里逐条解释。

## 4. 音乐内容

### 4.1 原创

- 旋律、和声进行的具体写法、歌词必须是原创的。可以写某种风格，不能复刻某一首已有的作品，改了名字也不行。
- 用户点名要某首已有的曲子时，写一首同风格的原创曲，并告诉用户。

### 4.2 段落

每个段落写四项：`id`（英文小写）、`name`（中文）、`fromTick`/`toTick`、`intent`。

`intent` 用一句话写清这一段**发生了什么变化**：谁进来、谁退出、和声去哪里、力度怎么走。AI 乐手即兴时读它，视频按它分镜，人在舞台上也靠它知道走到哪里了。不要写“气氛渐强”这类看不出做法的话。

### 4.3 音轨

播放器的预算是硬限制：**最多 8 轨，每轨最多 4 个 loop，同时发声最多 32 个音**。

- `id` 用英文小写（`drums`、`bass`、`lead`…），`name` 用中文，`role` 写声部职能。
- 鼓和打击乐轨写 `transposable: false`。
- **写定的声部用 clip，每个段落一个 clip**（`atTick` 是段落开头，`lengthTicks` 是段落长度）。这样 AI 乐手可以清掉并改写某一段，而不动别的段落。
- **反复的律动型用 loop**，用 `fromTick`/`toTick` 限定它响的范围。loop 是 AI 乐手最容易在现场替换的东西，适合放打击乐、固定音型。
- 鼓的过门、切分重音、幽灵音需要逐个控制力度时，用 clip 写。
- 同一轨上不要让同一个音高的两个音重叠。

鼓的音高对应（`drum.analog808style`）：

| 音高 | 乐器 | 音高 | 乐器 |
|------|------|------|------|
| 36 | 底鼓 | 46 | 开镲 |
| 35 | 短促底鼓 | 49 / 52 / 55 / 57 | 吊镲 |
| 37 | 鼓边 | 51 / 59 | 叮叮镲 |
| 38 / 40 | 军鼓 | 53 | 镲帽 |
| 39 | 拍手 | 54 | 铃鼓 |
| 42 / 44 | 闭镲 | 56 | 牛铃 |
| 41–50 的其余音 | 通鼓（低到高） | 60 / 61 | 邦戈（高 / 低） |

### 4.4 音色

- 用内置预设：`agb preset search` 列出全部，`agb preset show <id>` 看参数。写成 `"preset": "preset:<引擎>:<类别>:<名字>@v1.0"`。
- 需要微调时在 `params` 里覆盖数值参数（例如 `amp.releaseMs`），不要整套重写。
- 用了播放器不支持的引擎（采样、波表）必须给 `fallback`，保证这一轨有声音。

### 4.5 力度、节奏、和声

- 力度是 0–1。重音、普通音、闷音（幽灵音）之间至少差 0.2，听得出层次。
- 人性化用脚本里的固定种子做：力度 ±0.03–0.05，起音 ±3–6 tick。鼓的起音不要偏移。
- 低音声部每小节第一拍应当落在和弦根音上，除非这一段的 `intent` 写明了别的做法。
- 和弦乐器不要同时把音区铺满：同一时刻的和弦音控制在 4 个以内，给贝斯和鼓留位置。全体齐奏的地方最容易超过 32 个音，用三音和弦。
- 结尾必须有交代：落在主和弦上，最后一小节让它响完，`lengthTicks` 包含这一小节。

### 4.6 混音

引擎目前是单声道，没有效果器，平衡只能靠 `gainDb`。

- 每轨设 `gainDb`，`master.gainDb` 一般在 -3 到 -5，`master.limiter` 用 `soft`。
- 交付标准见 §8。

## 5. 现场演出用的曲子

现场演出时，人会在播放中移调、变速，并且通过对话让 AI 乐手改编。文件必须为此做好准备。

- **`performance.transpose` 和 `performance.bpm`** 写出这首曲子还好听的范围。移调后每一轨的音都必须在 MIDI 0–127 之内，低音不能掉到听不清的音区。
- **`performance.agent`** 必填：
  - `brief`：一句话说清风格、调、速度、核心和声。
  - `persona`：这支乐队怎么演奏。
  - `rules`：3–6 条具体、可检查的规则（例如“贝斯每小节第一拍落在根音上”）。
  - `allow`：允许 AI 改什么（`notes`、`tempo`、`transpose`、`fx`）。不想让 AI 动的就不写。
  - `sectionsAreFixed: true`：段落结构不变，AI 只在段落里面改。
- **留一个开放段**：只有节奏组、专门给现场乐手或 AI 即兴的段落，`id` 用 `open`，在 `intent` 里写明。
- **开头要有预备拍**：第一小节用鼓的过门或数拍把乐队带进来，不要第一拍就全体进入。
- **和弦表**写进 `form.json`（每小节的和弦和它落在第几拍），视频用它给台上的人提示。

## 6. 封面

- 正方形 SVG，`viewBox="0 0 1024 1024"`，放在源文件夹根目录，叫 `cover.svg`。
- 画面里有曲名。字体写系统字体并给出后备（`Helvetica Neue, Helvetica, Arial, sans-serif`；中文 `PingFang SC, Hiragino Sans GB, sans-serif`）。
- 不引用外部文件，不嵌入位图。
- 配色与 `visuals.palette` 和视频一致。
- 不使用真实人物的形象、商标或别人的作品。

## 7. 视频

视频是曲子的舞台画面，也是台上乐手的提示屏。

### 7.1 技术规格

| 项目 | 要求 |
|------|------|
| 比例 | 16:9（默认，1280×720）或 4:3。其它比例校验不通过 |
| 容器与编码 | MP4，H.264，`yuv420p`，30 fps，`+faststart` |
| 关键帧 | 至少每 2 秒一个（`-g 60`），播放器对齐时要跳转 |
| 音轨 | 不带音轨（`-an`）。声音只来自实时合成 |
| 长度 | `sync: "song"` 时等于曲子在写定速度下的长度 |
| 体积 | 不超过 40 MB。逐行扫描线、满屏噪点会让体积翻几倍，不要用 |
| 资产声明 | `assets.video` 写 `sha256` 和 `license`（含 `source`，说明画面是怎么来的） |

### 7.2 同步方式

- `sync: "song"`：画面跟随曲子的小节位置。按音乐分镜的视频用这个。现场变速时播放器按小节对齐并同比例改变视频速度，所以**画面必须按写定的 `time.bpm` 制作，曲子不能有 `tempoMap`**。
- `sync: "loop"`：循环播放，与位置无关。只用于氛围画面。

现场用的曲子必须用 `sync: "song"`。

### 7.3 画面怎么做

- **画面是时间的纯函数**：`make_video.py` 读 `song.json` 里的真实音符和 `form.json` 的曲式，每一帧只由时间算出来，不保存状态。鼓点、铜管、旋律在画面上的反应来自音符本身，不是按感觉摆的。改了音乐，重新渲染就自动对上。
- **每个段落有自己的场景**，段落切换时画面必须有看得出的变化。不能全曲一个画面。
- 脚本要能只渲染一帧（`--still <秒> out.png`）和一小段（`--clip <起> <止> out.mp4`），做的时候逐段检查，不要每次都渲染全曲。
- 画面里的文字用系统自带字体。
- 不使用真实人物的形象、商标或别人的作品。

### 7.4 提示条（现场用的曲子必须有）

画面上始终显示，位置固定，不被场景里的东西挡住：

- 当前段落的名字，这一段的第几小节 / 共几小节，以及进度；
- 当前和弦，下一个和弦，还有几拍换；
- 四个拍点灯，第一拍用不同的颜色；
- 全曲的小节号。

开放段另外用大字显示“还剩几小节”，段落结束前一小节给出倒数。

提示条上的和弦按文件写定的调显示。移调之后播放器界面会显示新的调，提示条不会变，演出前要让乐手知道这一点。

## 8. 交付标准

以下每一条都必须满足。

**格式**

```bash
agb song validate songs/<slug>.agbsong
```

- `valid: true`，`errors` 为空，`warnings` 为空。
- `player.playable: true`，`player.skipped` 为空（或已逐条说明）。

**声音**

```bash
agb song render songs/<slug>/<slug>.src --out-dir /tmp/<slug> --stems
```

看 `/tmp/<slug>/metrics.json`：

- `voicesStolen` 为 0，`maxConcurrentVoices` 不超过 32。不为 0 时用 `--from-tick`/`--to-tick` 分段渲染找到是哪几小节，把那里的和弦减音或缩短时值。
- `mix.clipCount` 为 0，`mix.peakDbfs` 在 -3 到 -0.5 之间。
- 每一轨的 `allSilent`、`nearSilent` 都是 false。
- 再用 `--transpose` 和 `--bpm` 分别取 `performance` 范围的两端各渲染一次，上面几条仍然成立。

**画面**

- 视频长度与曲子长度相差不超过 0.1 秒。
- 每个段落至少看过一帧静帧，另外看过全曲的缩略拼图，确认提示条没有被挡住、和弦与小节号正确。

**在 app 里**

- 打开 `.agbsong`：装载动画之后默认进入视频，画面与声音从第一小节对齐。
- 移调、变速各试一次。
- 现场用的曲子：全屏模式下和 AI 乐手对话一次，确认它能在开放段即兴。

没有亲自验证的项目，在交付说明里明确写出“未验证”，不要写成已完成。

## 9. 提交

- 提交：`make_song.py`、`make_video.py`、`form.json`、`<slug>.src/song.json`、`<slug>.src/cover.svg`、`songs/<slug>.agbsong`。
- 不提交：`<slug>.src/visuals/*.mp4`（由脚本重新生成，成品已经在 `.agbsong` 里）、渲染出来的 WAV。
- `songs/<slug>.agbsong` 里的 `song.json` 必须与源文件夹里的完全相同。`cargo test -p agb-cli --test song` 会检查这一点，并检查每个提交的曲目都满足 §8 的格式标准。
- 提交信息写明曲名、风格、调和速度。
