#!/usr/bin/env python3
"""Prepares what the Remotion project reads. Run from the repository root:

    .venv/bin/python songs/simulated-heart/video/prepare.py

1. media/*.mp4 (Grok, 24 fps) → video/public/clips/*.mp4 at 60 fps, motion-interpolated, so that no
   frame is shown twice (AUTHORING.md §7.3.1). Skipped for clips that are already there.
2. simulated-heart.src/song.json + form.json → video/src/data/song.json: every note as seconds.
3. Two stills → point clouds (video/src/data/cloud_*.json): the picture as 3D points, used when the
   android's portrait gathers out of noise at the start and scatters into stars at the end.
"""
import json, os, subprocess, sys
import numpy as np
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
SONG = os.path.dirname(HERE)
MEDIA = os.path.join(SONG, 'media')
CLIPS = os.path.join(HERE, 'public', 'clips')
DATA = os.path.join(HERE, 'src', 'data')
os.makedirs(CLIPS, exist_ok=True); os.makedirs(DATA, exist_ok=True)

# 1. clips
for f in sorted(os.listdir(MEDIA)):
    if not f.endswith('.mp4'): continue
    out = os.path.join(CLIPS, f)
    if os.path.exists(out) and '--force' not in sys.argv: continue
    subprocess.run(['ffmpeg', '-loglevel', 'error', '-y', '-i', os.path.join(MEDIA, f), '-an',
                    '-vf', 'minterpolate=fps=60:mi_mode=mci:mc_mode=aobmc:me_mode=bidir:vsbmc=1,scale=1280:720:force_original_aspect_ratio=increase,crop=1280:720',
                    '-c:v', 'libx264', '-preset', 'slow', '-crf', '15', '-g', '12', '-pix_fmt', 'yuv420p', out], check=True)
    print('clip', f, flush=True)

# 2. the song in seconds
song = json.load(open(os.path.join(SONG, 'simulated-heart.src', 'song.json')))
form = json.load(open(os.path.join(SONG, 'form.json')))
Q, BPM = song['time']['ppqn'], song['time']['bpm']
sec = lambda tick: round(tick / Q * 60 / BPM, 4)
tracks = {}
for t in song['tracks']:
    notes = []
    for c in t.get('clips', []):
        for n in c['notes']:
            notes.append([sec(c['atTick'] + n['tick']), sec(n['durTicks']), n['pitch'], n['velocity']])
    tracks[t['id']] = sorted(notes)
BAR = 4 * Q
out = {
    'bpm': BPM, 'duration': sec(song['time']['lengthTicks']),
    'sections': [{'id': s['id'], 'from': sec(s['bar'] * BAR), 'to': sec((s['bar'] + s['bars']) * BAR), 'humanity': s['humanity'], 'meter': s['meter']}
                 for s in form['sections']],
    'measures': [{'t': sec(m['tick']), 'section': m['section'], 'groups': [sec(g) for g in m['groups']]} for m in form['measures']],
    'tracks': tracks,
}
json.dump(out, open(os.path.join(DATA, 'song.json'), 'w'), separators=(',', ':'))
print('song.json', {k: len(v) for k, v in tracks.items()})

# 3. point clouds
def cloud(name, count=30000, seed=7):
    im = Image.open(os.path.join(MEDIA, name + '.jpg')).convert('RGB').resize((640, 360), Image.LANCZOS)
    a = np.asarray(im, dtype=np.float32) / 255
    lum = a @ np.array([0.299, 0.587, 0.114], np.float32)
    gy, gx = np.gradient(lum)
    weight = (lum ** 1.4 + 3.0 * np.hypot(gx, gy)).ravel() + 0.004     # bright areas and edges get the points
    rng = np.random.default_rng(seed)
    idx = rng.choice(weight.size, size=count, p=weight / weight.sum())
    y, x = np.divmod(idx, 640)
    px = (x + rng.random(count)) / 640 * 2 - 1
    py = 1 - (y + rng.random(count)) / 360 * 2
    col = a[y, x]
    z = (lum[y, x] - 0.45) * 0.5
    data = {'n': count,
            'pos': np.round(np.stack([px * 16 / 9, py, z], 1).ravel() * 1000).astype(int).tolist(),
            'col': np.round(col.ravel() * 255).astype(int).tolist()}
    json.dump(data, open(os.path.join(DATA, f'cloud_{name}.json'), 'w'), separators=(',', ':'))
    print('cloud', name, count)

cloud('android_cold'); cloud('android_warm', seed=8)
