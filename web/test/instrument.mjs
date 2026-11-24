// A web instrument inside a song: it sounds, and the song rendered stretch by stretch (as the page
// plays it) is the same as the song rendered in one go.
//
//   node web/test/instrument.mjs song.agbw

import { readFileSync } from 'node:fs'
import { createEngine, stretches } from '../js/engine-core.js'
import { openAgbw } from '../js/agbw.js'
import { instruments } from '../instruments/index.js'

const file = await openAgbw(readFileSync(process.argv[2]))
const wasm = readFileSync(new URL('../agb.wasm', import.meta.url))

async function render(withInstruments, inStretches) {
  const engine = await createEngine(wasm, withInstruments ? instruments : new Map())
  for (const [id, wav] of file.samples) engine.addSample(id, wav)
  const info = engine.load(file.song)
  const plan = inStretches ? stretches(info.lengthTicks, info.ppqn) : [[0, info.lengthTicks]]
  const parts = plan.map(([a, b]) => engine.render(a, b, b === info.lengthTicks))
  const mix = new Float32Array(parts.reduce((n, p) => n + p.length, 0))
  let at = 0
  for (const p of parts) {
    mix.set(p, at)
    at += p.length
  }
  return { mix, info }
}
const differ = (a, b) => {
  let worst = 0
  for (let i = 0; i < Math.min(a.length, b.length); i++) worst = Math.max(worst, Math.abs(a[i] - b[i]))
  return a.length === b.length ? worst : Infinity
}

const wanted = file.manifest.instruments || []
console.log('web instruments in the song:', wanted.join(', ') || 'none')
const a = await render(true, true)
const whole = await render(true, false)
const without = await render(false, true)
console.log('skipped with instruments:', a.info.skipped.map((s) => s.message).join(' | ') || 'nothing')
console.log('skipped without:', without.info.skipped.map((s) => s.message).join(' | ') || 'nothing')
const d1 = differ(a.mix, whole.mix)
const d2 = differ(a.mix, without.mix)
let peak = 0
for (const x of a.mix) peak = Math.max(peak, Math.abs(x))
console.log(`in stretches vs in one go: largest difference ${d1.toExponential(2)}`)
console.log(`with vs without the instruments: largest difference ${d2.toExponential(2)}`)
console.log(`peak ${(20 * Math.log10(peak)).toFixed(2)} dBFS`)
// A singer is not the same twice over: its breath and vibrato run on from wherever it was started,
// and that is the start of the piece in one case and a few seconds before its first note in the
// other. So with a sung track the two renders are both right and not equal.
const sung = (file.timeline.tracks || []).some((t) => t.sung)
if (sung) console.log('(a sung track: the first comparison is not held against the song)')
if ((!sung && d1 > 1e-5) || (wanted.length && d2 < 1e-3) || peak >= 1) process.exitCode = 1
