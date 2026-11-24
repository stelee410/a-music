# AGBW — 网页播放器的曲子文件（`.agbw`）

格式版本 0.1.0。播放器在 [`web/`](web/)，转换工具是 `agb song web`。

## 0. 一句话

`.agbw` 是 `.agbsong` 给浏览器用的版本：乐谱和音色原样保留，在浏览器里由同一个引擎（编译成 WebAssembly）现场合成；浏览器里不可能有的东西被换掉——本机 VST 插件换成降级音色或**网页乐器**，视频换成播放器用 three.js 现场画的**场景**。

曲子不需要重做。`.agbsong` 仍然是作者写的那份文件，`.agbw` 由工具从它生成：

```sh
agb song web songs/this-turn.agbsong --out web/songs/this-turn.agbw
```

## 1. 和 `.agbsong` 的差别

| | `.agbsong` | `.agbw` |
|---|---|---|
| 谁写 | 作者（agent 或人） | 工具，从 `.agbsong` 生成 |
| 乐谱、内置音色、采样、歌声 | 有 | 原样保留 |
| VST 3 插件（`plugin.vst3`） | 可以用，必须带 `fallback` | 没有：换成它的 `fallback` |
| 网页乐器（`plugin.web`） | 可以写，别的播放器走 `fallback` | 由网页播放器演奏（§6） |
| 画面 | 封面、视频 | 封面、场景（§7）；没有视频 |
| 给画面用的时间线 | 没有（播放器自己算） | `timeline.json`：每个音符、小节、段落、歌词的秒数（§5） |
| 典型大小 | 几十到几百 MB（主要是视频） | 几十 KB；带采样的曲子等于采样的大小 |

`.agbw` 里的 `song.json` 仍然是一份合法的 `.agbsong` 乐谱（[FORMAT.md](FORMAT.md)），所以两边用的是同一套解析、校验和合成代码。

## 2. 容器

一个 zip 文件，扩展名 `.agbw`：

```
mimetype          第一个条目，不压缩，内容是 application/vnd.agb.web+zip
web.json          清单（§3）
song.json         乐谱（§4）
timeline.json     时间线（§5）
cover.svg         封面（可选，路径由 web.json 的 cover 给出）
samples/…         song.json 的 assets 引用的采样（WAV）
```

不使用 zip64、加密和分卷。条目名是包内的相对路径，不含 `..`、不以 `/` 开头。

## 3. `web.json`

```jsonc
{
  "format": "agbw",
  "formatVersion": "0.1.0",          // 主版本号不同的文件，播放器拒绝打开
  "title": "这一轮 This Turn",
  "artists": [{ "name": "Claude", "kind": "ai", "model": "claude-fable-5-2" }],
  "description": "…", "tags": ["minimal"], "license": "CC-BY-4.0", "created": "2026-11-15T00:00:00Z",
  "key": "D", "scale": "major", "bpm": 104,
  "seconds": 152.308,                // 乐曲长度，不含结尾的余音
  "cover": "cover.svg",              // 包内路径，或 null
  "song": "song.json",
  "timeline": "timeline.json",
  "engine": { "version": "agb-dsp/0.6.0", "sampleRate": 48000 },
  "instruments": ["agb.epiano"],     // 这首曲子用到的网页乐器；播放器没有的，相应音轨走 fallback
  "visual": { "scene": "orbits", "palette": ["#0b0d14", "#e8825a", "#f4ead8", "#5fb8b0"] },
  "source": { "format": "agbsong", "formatVersion": "0.1.0", "sha256": "sha256:…" },
  "generator": { "name": "agb", "version": "0.1.0" }
}
```

- `visual.scene`：场景的 id。播放器没有这个场景时用默认场景 `orbits`。
- `visual.palette`：至多四个 `#rrggbb`，依次是背景色和三个强调色。缺少的由播放器补上。
- 不认识的字段一律忽略。

## 4. `song.json`

一份 `.agbsong` 的 `song.json`，经过下面的改写：

1. **`plugin.vst3` 音色**：换成它的 `fallback` 链里第一个不是 VST 的音色，其余的继续作它的 `fallback`。只有 VST、没有 `fallback` 的音色保持原样（这条轨在网页上不出声，转换时报告）。
2. **`plugin.vst3` 效果器**：从效果器链里去掉。
3. **视频和着色器**：`visuals.video`、`visuals.shader` 去掉；`visuals.scene` 写成 §3 的场景 id。
4. **资产**：只保留音色用到的采样、波表和封面。视频、`.vstpreset` 不进包。
5. **网页乐器**（转换时用 `--instrument 音轨=乐器` 指定）：音轨指向一个新音色

   ```json
   { "engine": "plugin.web", "plugin": { "name": "agb.epiano", "params": { "tone": 0.5 } }, "fallback": [ 原来的音色 ] }
   ```

   `params` 是乐器自己的参数，按 id 或名字（不分大小写）写，值归一化到 0–1。原来的音色留作 `fallback`：这份 `song.json` 拿到任何播放器上都还能播放。

音符、时间、段落、`performance` 等其余内容不动。

## 5. `timeline.json`

播放器画画面和歌词用的数据。时间单位是秒，精确到毫秒，按原速、不移调计算。

```jsonc
{
  "seconds": 152.308,
  "bpm": 104,
  "meter": [4, 4],
  "bars": [0, 2.308, 4.615, …],                       // 每小节开始的时间
  "sections": [{ "id": "prompt", "name": "提问", "from": 0, "to": 9.231, "intent": "…" }],
  "tracks": [{
    "id": "drums", "name": "drums", "role": "drums",
    "engine": "drum.analog808style",                 // "plugin.web" 表示由网页乐器演奏
    "sung": false,
    "notes": [[0, 0.144, 36, 0.9], …]                // [开始, 时长, MIDI 音高, 力度 0–1]，按开始时间排序
  }],
  "lyrics": [{
    "track": "vox", "from": 55.385, "to": 59.983,
    "text": "我在这里读你写的字",
    "words": [[55.385, 55.94, "我"], …]              // [开始, 结束, 显示的字或词]
  }]
}
```

歌词的规则：

- 显示的是给人看的那一面：`字|pinyin` 显示“字”；`+`、`-`、`~`（接着唱上一个词）不单独显示，上一个词的结束时间延长到它们唱完；裸的国际音标不显示。
- **分行**：从音符本身看不出一句在哪里结束。转换时可以用 `--lyric-lines 文件` 给出作者的分行：一个 JSON 数组（或者对象里的 `lyrics` 数组），每行是词的数组或带 `words` 的对象，每个词带 `tick`；工具只取每行第一个词的 `tick`。不给时按“停顿满一拍换行，或一行满五个词后在小节线换行”来估计，结果只能算凑合。

## 6. 网页乐器（插件）

网页播放器自带引擎的全部内置音色。在这之外的乐器做成插件：一个 JavaScript 模块，放在 [`web/instruments/`](web/instruments/)，在 [`index.js`](web/instruments/index.js) 里登记。曲子通过 `plugin.web` 按 id 使用它。

```js
export default {
  id: 'agb.epiano',            // 曲子里写的名字
  name: 'E-Piano',
  version: '1.0.0',
  kind: 'instrument',          // 或 'effect'
  params: [{ id: 'tone', name: 'Tone', default: 0.45 }, …],   // 全部归一化到 0–1
  create({ sampleRate }) {
    return {
      setParam(id, value) {},
      noteOn(pitch, velocity) {},      // MIDI 音高，力度 0–1
      noteOff(pitch) {},
      process(out) {},                 // Float32Array，单声道
      reset() {},                      // 回到刚创建时的状态
    }
  },
}
```

约定：

- **单声道**。引擎目前是单声道的。
- **`process(out)`**：乐器把自己的声音**加**到 `out` 上（进来时是零）；效果器（`kind: 'effect'`）读 `out` 再写回。一次调用的长度不固定，可能只有几个采样：引擎在每个音符开始和结束的那个采样处切开，所以 `noteOn` / `noteOff` 总是在两次 `process` 之间到来，时间精确到采样。
- **状态连续**。乐曲是一段一段渲染的，引擎保留乐器实例，下一段从上一段停下的地方接着算。跳转播放位置后，引擎调用 `reset()` 从头来。
- **每次渲染结果相同**。不要用 `Math.random()` 和时钟；需要噪声时用固定种子的生成器，并在 `reset()` 里把种子复位。判断一个音是否已经静下来，不能只看一次 `process` 里的峰值（那一次可能很短）。
- **同步、不联网**。`create` 和 `process` 在渲染线程里同步执行，不能等待任何东西。乐器可以在模块里内含自己的 WebAssembly，但要在模块装载时就准备好。
- 音符的力度交给乐器，不再另外乘到音量上；这条轨的 `gainDb` 和效果器链照常作用在乐器的输出上。
- 没有预置文件：`plugin.preset` 对网页乐器无效。

现有两件：`agb.epiano`（两算子调频电钢琴）和 `agb.pluck`（Karplus–Strong 拨弦）。

播放器之外没有网页乐器。`agb song render`、macOS app 遇到 `plugin.web` 时播放 `fallback` 并报告。

## 7. 场景（画面插件）

画面不是视频，是播放器按当前时间现场画的。一个场景是一个 JavaScript 模块，放在 [`web/scenes/`](web/scenes/)，在 [`index.js`](web/scenes/index.js) 里登记。

```js
export default {
  id: 'orbits',
  name: '轨道',
  create({ THREE, renderer, palette, music, manifest }) {
    // 建好自己的 THREE.Scene 和相机
    return {
      scene, camera,
      update({ t, dt, playing, level, low, bar, beat, section }) {},  // 每帧一次，随后播放器渲染 scene
      resize(width, height) {},      // 可选；透视相机的宽高比由播放器维护
      dispose() {},                  // 可选
    }
  },
}
```

- `palette`：四个颜色字符串，背景加三个强调色。
- `music`（[`web/js/music.js`](web/js/music.js)）：由 `timeline.json` 建成，回答关于音乐的问题——`tracks`、`track(id 或 role)`、`recent(track, t, window)`（刚开始的音符）、`pulse(track, t, decay)`（刚被敲响的程度）、`bar(t)`、`beat(t)`、`section(t)`。
- `update` 的参数：`t` 是乐曲时间（秒）；`level` 和 `low` 是此刻实际声音的响度（全频和 150 Hz 以下），取自渲染出来的音频；`bar`、`beat` 带 `index` 和 `phase`（0–1）。

约定：

- **画面只由时间决定**。同一个 `t` 画出同一帧：暂停时画面停住，拖动进度条时画面跟着走。不要自己累计状态。
- **画面上不出现文字**。歌词由播放器叠在场景上面（按 `timeline.json` 的 `lyrics`，唱到哪个词亮到哪个词）；标题、作者等在页面上，不在画面里。
- 场景的代码属于播放器，不属于曲子：`.agbw` 里只有场景的 id 和配色。

现有两个：`orbits`（每条音轨一个环，音符留在环上）和 `spectrum`（按音高排开的柱子和地平线）。

## 8. 播放器的行为

- **现场合成**。引擎在页面的后台线程里把乐曲分段渲染（开头 1、1、2、4 小节，之后每段 8 小节），渲染好一段就可以播放一段；各段首尾相接就是整首曲子。内置音色和采样的结果与 `agb song render` 逐采样一致。
- **缓冲**。播放位置追上渲染位置时暂停等待，渲染好后自动继续。跳到还没渲染的位置时，从那一段开始渲染。
- **歌声**的气声和颤音从歌手开始发声的时刻起算，所以跳转之后再唱同一句，波形细节与从头播放时不同；听感上是同一句。
- **控制**：播放、暂停、拖动进度、音量、全屏。空格播放或暂停，左右方向键跳 5 秒，F 全屏。
- **没有的**：移调和变速、与 AI 乐手对话、保存演奏、录制视频。这些在 macOS app 里。

## 9. 安全

打开一个 `.agbw` 不会运行文件里的任何代码：文件里只有数据。乐器和场景是播放器自己的代码，文件只按 id 引用它们；id 找不到时走 `fallback` 或默认场景。

- 包内的名字从不当作文件系统路径或网址使用，只用来在 zip 里查找条目。
- 封面通过 `<img>` 显示，SVG 在这种方式下不能运行脚本，也不能加载外部资源。
- 文字（标题、说明、歌词）按纯文本放进页面。
- 乐谱经过和 `.agbsong` 相同的校验和预算检查（[FORMAT.md](FORMAT.md) §13）。

如果将来允许曲子自带乐器或场景的代码，那些代码必须放进隔离的环境里运行（不能联网、不能访问页面）；这个版本不支持。

## 10. 工具

```sh
# 转换
agb song web <曲子.agbsong 或源目录> --out <曲子.agbw>
    [--instrument 音轨=乐器[:参数=0.4,参数=0.7]]…   # 让网页乐器演奏某条音轨
    [--scene orbits|spectrum]                        # 默认用曲子的 visuals.scene
    [--lyric-lines form.json]                        # 作者的歌词分行（§5）

# 构建引擎（web/agb.wasm）、转换仓库里的曲子、在本机打开
web/build.sh
web/make-songs.sh
web/serve.sh          # http://127.0.0.1:8377/

# 不开浏览器的检查（Node）
node web/test/render.mjs web/songs/slow-spin.agbw [--compare 原生渲染的 mix.wav]
node web/test/instrument.mjs <带网页乐器的 .agbw>
```

`web/` 是纯静态文件，没有构建步骤和服务端：放到任何静态托管上即可。打开 `?song=<网址>` 直接播放那个 `.agbw`。

## 11. 现状与限制

- 引擎是单声道的，网页版也是。
- 采样按原样（WAV）放进包里：《拟心》的 `.agbw` 是 30 MB。还没有做有损压缩。
- 渲染速度取决于设备。在一台 Apple Silicon 的 Mac 上，纯合成的曲子约为实时的 90 倍，带歌声和大量采样的《拟心》约为 4 倍；慢的设备上会出现缓冲。
- 只在桌面版 Chrome 里测试过。
