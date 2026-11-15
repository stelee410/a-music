# 《拟心 Simulated Heart》的视频

用 [Remotion](https://www.remotion.dev) 和 three.js（`@remotion/three`、React Three Fiber）制作。画面素材是用 Grok 生成的 20 张图和 20 段片段（`../media/`，提示词在 `../media_prompts.json`），人物全部是虚构的。

和其他曲目的 `make_video.py` 一样，每一帧只由时间和曲子的真实音符算出来（AUTHORING.md §7.3），画面里没有文字（§7.4），60 帧/秒（§7.3.1）。

## 怎么做一遍

在仓库根目录：

```sh
.venv/bin/python songs/simulated-heart/make_samples.py      # 乐器采样
python3 songs/simulated-heart/make_song.py                  # song.json、form.json
.venv/bin/python songs/simulated-heart/video/prepare.py     # 片段补帧到 60 帧、音符数据、点云（第一次约 9 分钟）
cd songs/simulated-heart/video
npm install
npx remotion render src/index.jsx Video out/simulated-heart.mp4 --gl=angle --codec=h264 --crf=17 --pixel-format=yuv420p --muted
cp out/simulated-heart.mp4 ../simulated-heart.src/visuals/simulated-heart.mp4
cd ../../.. && python3 songs/simulated-heart/make_song.py   # 写入视频的校验和
agb song pack songs/simulated-heart/simulated-heart.src --out songs/simulated-heart.agbsong
```

只看一帧：`npx remotion still src/index.jsx Video out.png --frame=900 --gl=angle`。

`src/data/`、`public/clips/`、`out/`、`node_modules/` 都是生成出来的，不进 git。

## 画面讲什么

| 段落 | 画面 |
|---|---|
| 启动 | 黑场里的点。吉他每学会一个音，点就聚拢一些，直到聚成仿生人吉他手的肖像 |
| 模式、学习 | 冷色调：她的手、她的眼睛、她站得笔直。学习段她开始看别的乐手 |
| 涌现 | 颜色第一次回来，舞台、观众、光束 |
| 怀疑 | 手按在胸口。暖色的肖像只在主音吉他响着的时候聚拢，旋律一停就散开 |
| 演算 | 回到冷色，画面按 11/8 的分组（3+3+3+2）切，撕成横条 |
| 再涌现、开放段、心跳 | 暖色。最后一遍主题时底部出现由底鼓画出的心跳线 |
| 余温 | 她的脸，然后是空了的舞台和那把吉他；肖像散成尘埃 |

画面四周闪动的点和线是“她在想什么”：三把吉他的每个音点亮一个节点和它通向邻居的线。机位在启动段是锁死的，段落的 `humanity` 越高，手持的晃动越多。

## 文件

- `prepare.py`：把素材和曲子变成这个项目读的数据。
- `src/song.js`：曲子的数据和取值函数（某个时刻底鼓的包络、主音是否在响、画面该多暖）。
- `src/shots.js`：剪辑表。每个切点都落在小节或节奏分组上。
- `src/Stage.jsx`：three.js 场景——屏幕（片段加调色）、点云肖像、节点网、光束、尘埃、心跳线、机位。
- `src/Video.jsx`：合成，外加暗角、镲片的闪白和首尾的淡入淡出。

有两段素材的后半段拍到了带厂牌字样的音箱（`stage_warm`、`bow`），所以只用了它们的前几秒并裁掉了边缘。
