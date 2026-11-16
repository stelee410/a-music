import React from 'react';
import {AbsoluteFill, useCurrentFrame} from 'remotion';
import {FPS, LYRICS, clamp, smooth} from './song';

// The words, a line at a time, as they are sung. The word being sung is lit; the ones to come are
// dim (or, where the machine is still learning the sentence, not there yet). While the picture is
// cold the words are set like a machine's output; when it warms they are set like a person's.

const LINES = LYRICS.map((l, i) => {
  const from = l.words[0][0];
  const last = l.words[l.words.length - 1];
  const next = LYRICS[i + 1] ? LYRICS[i + 1].words[0][0] : Infinity;
  return {...l, from, to: last[0] + last[1], next};
});
const COLD_SECTIONS = ['boot', 'pattern', 'learn', 'compute'];   // where the machine only calculates
const LEAD_IN = 0.3;   // a line is there this long before its first word
const HOLD = 0.8;      // and stays this long after its last, unless the next line needs the place
const FADE = 0.22;

const COLD = {font: '"SF Mono", Menlo, Monaco, monospace', size: 34, spacing: '0.14em', weight: 500, glow: '53,224,255', transform: 'lowercase'};
const WARM = {font: '"Helvetica Neue", Helvetica, Arial, sans-serif', size: 42, spacing: '0.02em', weight: 500, glow: '255,178,58', transform: 'none'};

const Line = ({line, t}) => {
  const out = Math.min(line.to + HOLD, line.next - LEAD_IN);
  const a = smooth((t - (line.from - LEAD_IN)) / FADE) * (1 - smooth((t - out) / FADE));
  if (a <= 0.001) return null;
  const st = COLD_SECTIONS.includes(line.section) ? COLD : WARM;
  return (
    <div style={{position: 'absolute', left: 80, right: 80, bottom: 58, textAlign: 'center', opacity: a,
                 fontFamily: st.font, fontSize: st.size, fontWeight: st.weight, letterSpacing: st.spacing, textTransform: st.transform, lineHeight: 1.25}}>
      {line.words.map(([t0, len, text], i) => {
        const sung = smooth((t - t0) / 0.07);                           // it lights as it starts
        const now = sung * (1 - smooth((t - (t0 + len)) / 0.25));       // and stays lit while it is held
        const base = line.reveal ? 0 : 0.45;
        const alpha = base + (1 - base) * sung;
        const glow = 0.85 * now;
        return (
          <span key={i} style={{display: 'inline-block', margin: '0 0.26em', color: `rgba(255,255,255,${alpha})`,
                               transform: `translateY(${-3 * now}px) scale(${1 + 0.05 * now})`,
                               textShadow: `0 2px 10px rgba(0,0,0,0.95), 0 0 2px rgba(0,0,0,0.9), 0 0 ${18 * glow}px rgba(${st.glow},${glow}), 0 0 ${42 * glow}px rgba(${st.glow},${0.6 * glow})`}}>
            {text}
          </span>
        );
      })}
    </div>
  );
};

export const Lyrics = () => {
  const t = useCurrentFrame() / FPS;
  return (
    <AbsoluteFill>
      {/* a soft band under the words keeps them readable over bright footage */}
      <AbsoluteFill style={{background: 'linear-gradient(to bottom, rgba(0,0,0,0) 70%, rgba(0,0,0,0.42) 100%)',
                            opacity: clamp(LINES.reduce((m, l) => Math.max(m, smooth((t - (l.from - LEAD_IN)) / FADE) * (1 - smooth((t - (l.to + HOLD)) / FADE))), 0))}} />
      {LINES.filter((l) => t > l.from - LEAD_IN - 0.01 && t < l.to + HOLD + FADE).map((l, i) => <Line key={l.from} line={l} t={t} />)}
    </AbsoluteFill>
  );
};
