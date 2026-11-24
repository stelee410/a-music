// The engine (crates/agb-web, compiled to WebAssembly) behind a small JavaScript interface.
// No DOM, no worker: this file runs the same in a worker of the page and in Node (web/test/).
//
// Web instruments (AGBW.md §6) are handed in as a Map of id → definition; the engine calls them
// through the functions of `agb_host` below while it renders.

const text = new TextDecoder()
const bytesOf = (x) => (typeof x === 'string' ? new TextEncoder().encode(x) : x)

export async function createEngine(wasm, instruments = new Map()) {
  let memory = null
  const str = (ptr, len) => text.decode(new Uint8Array(memory.buffer, ptr, len))
  const sameKey = (a, b) => String(a).toLowerCase() === String(b).toLowerCase()
  const paramIndex = (def, key) => (def.params || []).findIndex((p) => sameKey(p.id, key) || sameKey(p.name ?? p.id, key))

  // Running instruments: handle → { def, unit }.
  const open = new Map()
  let nextHandle = 1
  const start = (def, sampleRate) => {
    const unit = def.create({ sampleRate })
    for (const p of def.params || []) unit.setParam?.(p.id, p.default ?? 0)
    return unit
  }

  const host = {
    plugin_has: (ptr, len) => (instruments.has(str(ptr, len)) ? 1 : 0),
    plugin_has_param: (ptr, len, kptr, klen) => {
      const def = instruments.get(str(ptr, len))
      return def && paramIndex(def, str(kptr, klen)) >= 0 ? 1 : 0
    },
    plugin_open: (ptr, len, instrument, sampleRate) => {
      const def = instruments.get(str(ptr, len))
      if (!def) return -1
      if (!!instrument !== ((def.kind ?? 'instrument') === 'instrument')) return -1
      const handle = nextHandle++
      open.set(handle, { def, sampleRate, unit: start(def, sampleRate) })
      return handle
    },
    plugin_param: (handle, kptr, klen) => paramIndex(open.get(handle).def, str(kptr, klen)),
    plugin_set: (handle, index, value) => {
      const o = open.get(handle)
      o.unit.setParam?.(o.def.params[index].id, value)
    },
    plugin_takes_audio: (handle) => (open.get(handle).def.kind === 'effect' ? 1 : 0),
    plugin_note: (handle, on, pitch, velocity) => {
      const u = open.get(handle).unit
      if (on) u.noteOn?.(pitch, velocity)
      else u.noteOff?.(pitch)
    },
    plugin_process: (handle, ptr, len) => open.get(handle).unit.process(new Float32Array(memory.buffer, ptr, len)),
    plugin_reset: (handle) => {
      const o = open.get(handle)
      if (o.unit.reset) o.unit.reset()
      else o.unit = start(o.def, o.sampleRate)
    },
    plugin_close: (handle) => open.delete(handle),
  }

  const { instance } = await WebAssembly.instantiate(wasm, { agb_host: host })
  const x = instance.exports
  memory = x.memory

  const lastText = () => str(x.agbw_text_ptr(), x.agbw_text_len())
  const withBytes = (data, f) => {
    const b = bytesOf(data)
    const ptr = x.agbw_alloc(b.length)
    new Uint8Array(memory.buffer, ptr, b.length).set(b)
    try {
      return f(ptr, b.length)
    } finally {
      x.agbw_free(ptr, b.length)
    }
  }

  return {
    /** A WAV file of the song, under its asset id. */
    addSample(id, wav) {
      const r = withBytes(id, (ip, il) => withBytes(wav, (wp, wl) => x.agbw_sample(ip, il, wp, wl)))
      if (r !== 0) throw new Error(lastText())
    },
    /** Take `song.json`. Returns { sampleRate, ppqn, lengthTicks, lengthSamples, tailSamples, skipped, … }. */
    load(songJson, { transpose = 0, bpm = 0 } = {}) {
      const r = withBytes(songJson, (p, l) => x.agbw_load(p, l, transpose, bpm))
      if (r !== 0) throw new Error(lastText())
      return JSON.parse(lastText())
    },
    /** The samples (mono Float32Array) of ticks [from, to); with `last`, the ringing after the end too. */
    render(fromTick, toTick, last = false) {
      const n = x.agbw_render(fromTick, toTick, last ? 1 : 0)
      if (n < 0) throw new Error(lastText())
      return new Float32Array(memory.buffer, x.agbw_out_ptr(), n).slice()
    },
    tickToSample: (tick) => x.agbw_tick_to_sample(tick),
    /** After a jump: instruments and singers start afresh at the next stretch. */
    forget: () => x.agbw_forget(),
  }
}

/** The stretches a piece is rendered in: short at first so that it starts soon, then longer. */
export function stretches(lengthTicks, ppqn) {
  const bar = ppqn * 4
  const out = []
  let at = 0
  for (const bars of [1, 1, 2, 4]) {
    if (at >= lengthTicks) break
    out.push([at, Math.min(lengthTicks, at + bars * bar)])
    at += bars * bar
  }
  while (at < lengthTicks) {
    out.push([at, Math.min(lengthTicks, at + 8 * bar)])
    at += 8 * bar
  }
  return out
}
