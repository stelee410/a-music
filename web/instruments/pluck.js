// agb.pluck — a plucked string (Karplus–Strong): a burst of noise goes round a delay line as long
// as one period of the note and loses its highs on every turn. A web instrument (AGBW.md §6).

export default {
  id: 'agb.pluck',
  name: 'Plucked String',
  version: '1.0.0',
  kind: 'instrument',
  params: [
    { id: 'brightness', name: 'Brightness', default: 0.6 },
    { id: 'decay', name: 'Decay', default: 0.6 },
    { id: 'level', name: 'Level', default: 0.6 },
  ],
  create({ sampleRate }) {
    const p = { brightness: 0.6, decay: 0.6, level: 0.6 }
    let voices = []
    // The same noise every time: a render must not differ from the one before.
    let seed = 0x2545f491
    const noise = () => {
      seed ^= seed << 13
      seed ^= seed >>> 17
      seed ^= seed << 5
      return ((seed >>> 0) / 0xffffffff) * 2 - 1
    }
    return {
      setParam(id, v) {
        if (id in p) p[id] = v
      },
      noteOn(pitch, velocity) {
        const f = 440 * 2 ** ((pitch - 69) / 12)
        const period = sampleRate / f
        const n = Math.max(2, Math.floor(period))
        const line = new Float32Array(n)
        // A harder pluck is brighter: the burst is smoothed less.
        const smooth = 1 - (0.25 + 0.75 * p.brightness) * (0.4 + 0.6 * velocity)
        let last = 0
        for (let i = 0; i < n; i++) {
          last = smooth * last + (1 - smooth) * noise()
          line[i] = last * velocity
        }
        const seconds = 0.4 + 6 * p.decay
        voices.push({
          pitch,
          line,
          at: 0,
          frac: period - n, // the part of the period the whole line cannot hold
          prev: 0,
          ap: 0,
          loss: Math.exp(-period / (seconds * sampleRate)),
          damp: 0.5 - 0.42 * p.brightness, // how much each turn mixes in the neighbour: darker
          held: true,
          energy: 1,
        })
        if (voices.length > 24) voices.shift()
      },
      noteOff(pitch) {
        for (const n of voices) if (n.pitch === pitch) n.held = false
      },
      process(out) {
        const g = 0.5 * (0.4 + p.level)
        for (const n of voices) {
          const { line } = n
          const len = line.length
          const loss = n.held ? n.loss : n.loss * 0.985
          let peak = 0
          for (let i = 0; i < out.length; i++) {
            const x = line[n.at]
            const next = line[n.at + 1 === len ? 0 : n.at + 1]
            // Lose highs, then a first-order all-pass for the fraction of a sample.
            const y = loss * ((1 - n.damp) * x + n.damp * next)
            const c = (1 - n.frac) / (1 + n.frac)
            const z = c * y + n.prev - c * n.ap
            n.prev = y
            n.ap = z
            line[n.at] = z
            n.at = n.at + 1 === len ? 0 : n.at + 1
            out[i] += x * g
            const a = x < 0 ? -x : x
            if (a > peak) peak = a
          }
          // How loud the string still is: its latest peak, forgotten over about 50 ms. (The peak of
          // one call alone will not do: a call may be a few samples long and catch a zero crossing.)
          n.energy = Math.max(peak, n.energy * Math.exp(-out.length / (0.05 * sampleRate)))
        }
        voices = voices.filter((n) => n.energy > 1e-5)
      },
      reset() {
        voices = []
        seed = 0x2545f491
      },
    }
  },
}
