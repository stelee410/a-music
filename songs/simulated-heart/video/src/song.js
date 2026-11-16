// The song as data: every picture is a function of the time and of these notes.
import data from './data/song.json';

export const FPS = 60;
export const DURATION = data.duration;
export const SECTIONS = data.sections;
export const MEASURES = data.measures;
export const TRACKS = data.tracks; // id → [[seconds, length, pitch, velocity]]
export const LYRICS = data.lyrics; // [{reveal, section, words: [[seconds, length, word]]}]

export const section = (t) => SECTIONS.find((s) => t < s.to) || SECTIONS[SECTIONS.length - 1];
export const sec = (id) => SECTIONS.find((s) => s.id === id);

const onsets = (notes) => notes.map((n) => n[0]);
const drum = (key) => onsets(TRACKS.drums.filter((n) => n[2] === key));
export const KICK = drum(36);
export const SNARE = drum(38);
export const CRASH = onsets(TRACKS.drums.filter((n) => n[2] === 49 || n[2] === 52));
export const LEAD = TRACKS.lead;
export const GTR_A = TRACKS.gtr_a;
export const GTR_B = TRACKS.gtr_b;
export const POWER = onsets(TRACKS.power);
export const GROUPS = MEASURES.flatMap((m) => m.groups);

/** Index of the last element ≤ t in a sorted array of times (−1 if none). */
export const lastBefore = (times, t) => {
  let lo = 0, hi = times.length - 1, ans = -1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (times[mid] <= t) { ans = mid; lo = mid + 1; } else hi = mid - 1;
  }
  return ans;
};

/** 1 at a hit, falling to 0 with time constant tau. */
export const env = (times, t, tau) => {
  const i = lastBefore(times, t);
  return i < 0 ? 0 : Math.exp(-(t - times[i]) / tau);
};

/** How much of the lead is sounding now: 1 while a note is held, easing in and out. */
export const leadLevel = (t) => {
  let v = 0;
  for (const [t0, len] of LEAD) {
    if (t0 > t + 0.2) break;
    const a = Math.min(1, Math.max(0, (t - t0) / 0.12));
    const r = Math.min(1, Math.max(0, 1 - (t - (t0 + len)) / 0.9));
    if (t >= t0) v = Math.max(v, a * r);
  }
  return v;
};

export const clamp = (x, a = 0, b = 1) => Math.min(b, Math.max(a, x));
export const smooth = (x) => { const t = clamp(x); return t * t * (3 - 2 * t); };
export const mix = (a, b, t) => a + (b - a) * t;
export const hash = (n) => { const x = Math.sin(n * 127.1 + 311.7) * 43758.5453; return x - Math.floor(x); };

/** How warm the picture is (0 = the cold grade of the machine, 1 = the footage as it is). */
export const warmth = (t) => {
  const s = section(t);
  const p = (t - s.from) / (s.to - s.from);
  switch (s.id) {
    case 'boot': case 'pattern': return 0;
    case 'learn': return 0.3 * smooth((p - 0.5) * 2);
    case 'emerge1': return smooth((t - s.from) / 0.6);
    case 'doubt': return 0.25 + 0.6 * leadLevel(t);
    case 'compute': return 0;
    case 'open': return 0.85;
    case 'heart': return 0.85 + 0.15 * smooth((t - s.from) / 0.4);   // it comes from the open section, already warm
    case 'outro': return 1;
    default: return smooth((t - s.from) / 0.4);
  }
};
