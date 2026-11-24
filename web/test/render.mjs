// Render an .agbw with the web engine in Node, the way the page does (stretch by stretch), and
// say how fast it went. Optionally compare with a WAV of the native engine or write a WAV.
//
//   node web/test/render.mjs song.agbw [--compare mix.wav] [--wav out.wav]

import { readFileSync, writeFileSync } from 'node:fs'
import { createEngine, stretches } from '../js/engine-core.js'
import { openAgbw } from '../js/agbw.js'
import { instruments } from '../instruments/index.js'

const args = process.argv.slice(2)
const opt = (name) => (args.includes(name) ? args[args.indexOf(name) + 1] : null)
const file = await openAgbw(readFileSync(args[0]))
const engine = await createEngine(readFileSync(new URL('../agb.wasm', import.meta.url)), instruments)
for (const [id, wav] of file.samples) engine.addSample(id, wav)
const info = engine.load(file.song)
for (const s of info.skipped) console.log('skipped:', s.message)

const parts = []
const t0 = performance.now()
let slowest = Infinity
for (const [a, b] of stretches(info.lengthTicks, info.ppqn)) {
  const t = performance.now()
  const audio = engine.render(a, b, b === info.lengthTicks)
  slowest = Math.min(slowest, audio.length / info.sampleRate / ((performance.now() - t) / 1000))
  parts.push(audio)
}
const total = parts.reduce((n, p) => n + p.length, 0)
const mix = new Float32Array(total)
let at = 0
for (const p of parts) {
  mix.set(p, at)
  at += p.length
}
const seconds = total / info.sampleRate
const took = (performance.now() - t0) / 1000
let peak = 0
for (const x of mix) peak = Math.max(peak, Math.abs(x))
console.log(`${file.manifest.title}: ${seconds.toFixed(1)} s in ${took.toFixed(1)} s — ${(seconds / took).toFixed(1)}× real time (slowest stretch ${slowest.toFixed(1)}×), peak ${(20 * Math.log10(peak)).toFixed(2)} dBFS`)
if (total !== info.lengthSamples + info.tailSamples) throw new Error(`length: ${total} samples, expected ${info.lengthSamples + info.tailSamples}`)

function readWav(path) {
  const b = readFileSync(path)
  let o = 12
  let fmt = null
  while (o + 8 <= b.length) {
    const id = b.toString('latin1', o, o + 4)
    const len = b.readUInt32LE(o + 4)
    if (id === 'fmt ') {
      fmt = { tag: b.readUInt16LE(o + 8), channels: b.readUInt16LE(o + 10), bits: b.readUInt16LE(o + 22) }
      if (fmt.tag === 0xfffe && len >= 26) fmt.tag = b.readUInt16LE(o + 8 + 24) // "extensible": the real kind is further in
    }
    if (id === 'data') {
      const bytes = fmt.bits / 8
      const n = Math.floor(len / bytes / fmt.channels)
      const out = new Float32Array(n)
      for (let i = 0; i < n; i++) {
        const p = o + 8 + i * bytes * fmt.channels
        out[i] = fmt.tag === 3 ? b.readFloatLE(p) : bytes === 2 ? b.readInt16LE(p) / 32768 : bytes === 3 ? b.readIntLE(p, 3) / 8388608 : b.readInt32LE(p) / 2147483648
      }
      return out
    }
    o += 8 + len + (len & 1)
  }
  throw new Error(`${path}: no audio`)
}

const compare = opt('--compare')
if (compare) {
  const ref = readWav(compare)
  let worst = 0
  let where = 0
  for (let i = 0; i < Math.min(ref.length, mix.length); i++) {
    const d = Math.abs(ref[i] - mix[i])
    if (d > worst) {
      worst = d
      where = i
    }
  }
  console.log(`against ${compare}: ${ref.length} vs ${mix.length} samples, largest difference ${worst.toExponential(2)} at ${(where / info.sampleRate).toFixed(2)} s`)
  if (ref.length !== mix.length || worst > 1e-4) process.exitCode = 1
}
const out = opt('--wav')
if (out) {
  const h = Buffer.alloc(44)
  h.write('RIFF', 0)
  h.writeUInt32LE(36 + total * 2, 4)
  h.write('WAVEfmt ', 8)
  h.writeUInt32LE(16, 16)
  h.writeUInt16LE(1, 20)
  h.writeUInt16LE(1, 22)
  h.writeUInt32LE(info.sampleRate, 24)
  h.writeUInt32LE(info.sampleRate * 2, 28)
  h.writeUInt16LE(2, 32)
  h.writeUInt16LE(16, 34)
  h.write('data', 36)
  h.writeUInt32LE(total * 2, 40)
  const d = Buffer.alloc(total * 2)
  for (let i = 0; i < total; i++) d.writeInt16LE(Math.max(-32768, Math.min(32767, Math.round(mix[i] * 32767))), i * 2)
  writeFileSync(out, Buffer.concat([h, d]))
  console.log('wrote', out)
}
