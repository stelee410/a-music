// The engine in a worker of the page: renders the song a stretch at a time and posts the audio.
//
//   in:  { type: 'load', wasm: ArrayBuffer, song: Uint8Array, samples: [[id, Uint8Array]…], transpose }
//        { type: 'want', index }      — render from this stretch on (after a jump)
//   out: { type: 'ready', info, stretches: [{ start, end }…] (samples) }
//        { type: 'chunk', index, start, audio: Float32Array, level: Float32Array, low: Float32Array }
//        { type: 'done' } | { type: 'error', message }

import { createEngine, stretches } from './engine-core.js'
import { instruments } from '../instruments/index.js'

/** Samples per value of the loudness curves the scenes draw from (60 per second at 48 kHz). */
const HOP = 800

let engine = null
let plan = [] // [fromTick, toTick]
let done = []
let next = 0
let generation = 0

/** Loudness of a stretch, HOP samples at a time: of everything, and of the lows (below ≈150 Hz). */
function curves(audio, start, sampleRate) {
  const first = Math.floor(start / HOP)
  const n = Math.ceil((start + audio.length) / HOP) - first
  const level = new Float32Array(n)
  const low = new Float32Array(n)
  const k = Math.exp((-2 * Math.PI * 150) / sampleRate)
  let lp = 0
  for (let i = 0; i < n; i++) {
    const a = Math.max(0, (first + i) * HOP - start)
    const b = Math.min(audio.length, (first + i + 1) * HOP - start)
    let s = 0
    let sl = 0
    for (let j = a; j < b; j++) {
      const x = audio[j]
      lp = k * lp + (1 - k) * x
      s += x * x
      sl += lp * lp
    }
    level[i] = Math.sqrt(s / Math.max(1, b - a))
    low[i] = Math.sqrt(sl / Math.max(1, b - a))
  }
  return { first, level, low }
}

async function run(mine, info) {
  for (;;) {
    let i = done.indexOf(false, next)
    if (i < 0) i = done.indexOf(false)
    if (i < 0) break
    const [from, to] = plan[i]
    const audio = engine.render(from, to, i === plan.length - 1)
    if (mine !== generation) return
    done[i] = true
    next = i + 1
    const start = engine.tickToSample(from)
    const c = curves(audio, start, info.sampleRate)
    self.postMessage({ type: 'chunk', index: i, start, audio, level: c.level, low: c.low, first: c.first }, [audio.buffer, c.level.buffer, c.low.buffer])
    // Let a 'want' or a new 'load' in before the next stretch.
    await new Promise((r) => setTimeout(r))
    if (mine !== generation) return
  }
  self.postMessage({ type: 'done' })
}

self.onmessage = async (e) => {
  const m = e.data
  try {
    if (m.type === 'load') {
      const mine = ++generation
      engine = await createEngine(m.wasm, instruments)
      for (const [id, wav] of m.samples) {
        try {
          engine.addSample(id, wav)
        } catch (err) {
          self.postMessage({ type: 'note', message: String(err.message || err) })
        }
      }
      const info = engine.load(m.song, { transpose: m.transpose || 0 })
      plan = stretches(info.lengthTicks, info.ppqn)
      done = plan.map(() => false)
      next = 0
      self.postMessage({
        type: 'ready',
        info,
        hop: HOP,
        stretches: plan.map(([a, b], i) => ({ start: engine.tickToSample(a), end: engine.tickToSample(b) + (i === plan.length - 1 ? info.tailSamples : 0) })),
      })
      run(mine, info)
    } else if (m.type === 'want') {
      if (m.index >= 0 && m.index < plan.length) next = m.index
    }
  } catch (err) {
    self.postMessage({ type: 'error', message: String(err.message || err) })
  }
}
