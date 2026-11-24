// agb.epiano — an electric piano: two-operator FM with a short bright "tine" at the start of each
// note. A web instrument (AGBW.md §6): mono, any number of notes at once.

const TAU = Math.PI * 2

export default {
  id: 'agb.epiano',
  name: 'E-Piano',
  version: '1.0.0',
  kind: 'instrument',
  params: [
    { id: 'tone', name: 'Tone', default: 0.45 }, // how much the sound brightens with a harder touch
    { id: 'decay', name: 'Decay', default: 0.5 }, // how long a held note rings
    { id: 'tine', name: 'Tine', default: 0.4 }, // level of the metallic start
    { id: 'tremolo', name: 'Tremolo', default: 0.15 },
    { id: 'level', name: 'Level', default: 0.6 },
  ],
  create({ sampleRate }) {
    const p = { tone: 0.45, decay: 0.5, tine: 0.4, tremolo: 0.15, level: 0.6 }
    let voices = []
    let lfo = 0
    return {
      setParam(id, v) {
        if (id in p) p[id] = v
      },
      noteOn(pitch, velocity) {
        const f = 440 * 2 ** ((pitch - 69) / 12)
        // Higher notes die away sooner, as on the real instrument.
        const seconds = (0.6 + 5 * p.decay) * 2 ** ((60 - pitch) / 24)
        voices.push({
          pitch,
          f,
          v: velocity,
          cp: 0, // carrier, modulator and tine phases
          mp: 0,
          tp: 0,
          env: 0,
          attack: true,
          k: Math.exp(-1 / (seconds * sampleRate)),
          body: 1, // modulation amount, falls faster than the level
          bk: Math.exp(-1 / (0.35 * seconds * sampleRate)),
          tine: 1,
          tk: Math.exp(-1 / (0.03 * sampleRate)),
          held: true,
          rk: Math.exp(-1 / (0.09 * sampleRate)),
        })
        if (voices.length > 24) voices.shift()
      },
      noteOff(pitch) {
        for (const n of voices) if (n.pitch === pitch) n.held = false
      },
      process(out) {
        const dl = (TAU * 4.6) / sampleRate
        const up = 1 / (0.002 * sampleRate)
        for (let i = 0; i < out.length; i++) {
          let s = 0
          for (const n of voices) {
            if (n.attack) {
              n.env += up
              if (n.env >= 1) {
                n.env = 1
                n.attack = false
              }
            } else {
              n.env *= n.held ? n.k : n.rk
            }
            n.body *= n.bk
            n.tine *= n.tk
            const w = (TAU * n.f) / sampleRate
            n.cp += w
            n.mp += w
            n.tp += w * 14
            if (n.cp > TAU) n.cp -= TAU
            if (n.mp > TAU) n.mp -= TAU
            if (n.tp > TAU) n.tp -= TAU
            const index = (0.25 + 2.6 * p.tone * n.v) * (0.35 + 0.65 * n.body)
            const tine = p.tine * n.v * n.tine * 0.6 * Math.sin(n.tp)
            s += n.env * n.v * Math.sin(n.cp + index * Math.sin(n.mp) + tine)
          }
          lfo += dl
          if (lfo > TAU) lfo -= TAU
          out[i] += s * 0.37 * (0.4 + p.level) * (1 - p.tremolo * 0.5 * (1 + Math.sin(lfo)))
        }
        voices = voices.filter((n) => n.attack || n.env > 1e-5)
      },
      reset() {
        voices = []
        lfo = 0
      },
    }
  },
}
