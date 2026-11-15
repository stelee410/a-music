// The cut: which Grok clip is on the screen when. Every cut falls on a measure or on a rhythmic group.
import {FPS, MEASURES, sec} from './song';

const CLIP_FRAMES = 356; // the clips are 6 s at 60 fps; keep clear of the last frames
const f = (seconds) => Math.round(seconds * FPS);

const shots = [];
const add = (from, to, clip, offset = 0, zoom = 0) => {
  const dur = to - from;
  if (dur <= 0) return;
  shots.push({from, dur, clip, offset: Math.max(0, Math.min(offset, CLIP_FRAMES - dur)), zoom});
};

/** Measures of a section, as frame numbers, with the end of the section appended. */
const bounds = (id) => {
  const s = sec(id);
  return [...MEASURES.filter((m) => m.section === id).map((m) => f(m.t)), f(s.to)];
};

/** One clip per `per` measures. */
const every = (id, per, clips) => {
  const b = bounds(id);
  let k = 0;
  for (let i = 0; i + 1 < b.length; i += per) {
    const c = clips[k % clips.length];
    add(b[i], b[Math.min(i + per, b.length - 1)], c[0], c[1] || 0, c[2] || 0);
    k++;
  }
};

// pattern: the machine, in close-up
every('pattern', 2, [['tapping'], ['eye_cold'], ['android_cold'], ['stage_cold']]);
// learn: looking at the others
every('learn', 2, [['drummer'], ['bass_hands'], ['tapping', 170], ['bassist'], ['android_look'], ['drum_top'], ['red_smile'], ['stage_cold', 170]]);
// the theme
every('emerge1', 2, [['stage_warm', 0, 0.18], ['red_jump'], ['duo'], ['crowd']]);
// doubt
every('doubt', 2, [['android_cold', 90], ['heart'], ['eye_cold', 90], ['android_look', 90]]);
// compute: a cut on every group of the 11/8 (3+3+3+2)
{
  const s = sec('compute');
  const ms = MEASURES.filter((m) => m.section === 'compute');
  const cycle = ['tapping', 'drum_top', 'bass_hands', 'drummer', 'eye_cold', 'bassist', 'stage_cold', 'android_cold'];
  let k = 0;
  ms.forEach((m, mi) => {
    const next = mi + 1 < ms.length ? ms[mi + 1].t : s.to;
    m.groups.forEach((g, gi) => {
      const to = gi + 1 < m.groups.length ? m.groups[gi + 1] : next;
      add(f(g), f(to), cycle[k % cycle.length], (k * 47) % 300, 0.06);
      k++;
    });
  });
}
every('emerge2', 2, [['duo', 150], ['red_jump', 150], ['red_smile', 120], ['crowd_front']]);
every('open', 2, [['crowd', 120], ['bassist', 150], ['drummer', 150], ['drum_top', 150]]);
// (the later part of the wide shot and of the bow show amplifiers with a maker's name: only their first seconds are used, cropped in)
every('heart', 2, [['android_warm'], ['stage_warm', 0, 0.3], ['eye_warm'], ['duo', 40], ['crowd_front', 150], ['crowd', 0], ['heart', 120], ['bow', 0, 0.24]]);
// outro: her face, then the empty stage
{
  const s = sec('outro');
  const a = f(s.from), b = f(s.to);
  add(a, a + 250, 'android_warm', 100);
  add(a + 250, b, 'after', 0);
}

export const SHOTS = shots;
