// What a scene may ask about the music: built from timeline.json of the .agbw. Every answer
// depends on the time alone, so a frame can be drawn again exactly as it was.

/** First index whose start is > t, in notes sorted by start. */
function upper(notes, t) {
  let lo = 0
  let hi = notes.length
  while (lo < hi) {
    const mid = (lo + hi) >> 1
    if (notes[mid][0] <= t) lo = mid + 1
    else hi = mid
  }
  return lo
}

export function makeMusic(timeline) {
  const tracks = (timeline.tracks || []).map((t, index) => {
    const notes = t.notes || []
    let lowest = 127
    let highest = 0
    for (const n of notes) {
      if (n[2] < lowest) lowest = n[2]
      if (n[2] > highest) highest = n[2]
    }
    const drums = t.role === 'drums' || /^drum\./.test(t.engine || '')
    return { ...t, index, notes, drums, lowest: notes.length ? lowest : 60, highest: notes.length ? highest : 60 }
  })
  const bars = timeline.bars?.length ? timeline.bars : [0]
  const seconds = timeline.seconds || 0
  const perBar = timeline.meter?.[0] || 4
  const sections = timeline.sections || []

  const music = {
    seconds,
    bpm: timeline.bpm || 120,
    tracks,
    sections,
    /** A track by id, or the first with that role. */
    track: (key) => tracks.find((t) => t.id === key) || tracks.find((t) => t.role === key) || null,
    /** Notes [start, length, pitch, velocity] that began in (t − window, t], oldest first. */
    recent(track, t, window) {
      const b = upper(track.notes, t)
      let a = b
      while (a > 0 && track.notes[a - 1][0] > t - window) a--
      return track.notes.slice(a, b)
    },
    /** How hard the track was just struck: velocity of its latest notes, dying away over `decay` seconds. */
    pulse(track, t, decay = 0.25) {
      if (!track) return 0
      let best = 0
      for (let i = upper(track.notes, t) - 1; i >= 0; i--) {
        const n = track.notes[i]
        const age = t - n[0]
        if (age > decay * 6) break
        const v = n[3] * Math.exp(-age / decay)
        if (v > best) best = v
      }
      return best
    },
    /** Where in which bar: { index, phase 0…1, length in seconds }. */
    bar(t) {
      let lo = 0
      let hi = bars.length
      while (lo < hi) {
        const mid = (lo + hi) >> 1
        if (bars[mid] <= t) lo = mid + 1
        else hi = mid
      }
      const i = Math.max(0, lo - 1)
      const from = bars[i]
      const to = i + 1 < bars.length ? bars[i + 1] : Math.max(seconds, from + 0.001)
      return { index: i, phase: Math.min(1, Math.max(0, (t - from) / (to - from))), length: to - from }
    },
    /** Where in which beat: { index, phase 0…1 }. */
    beat(t) {
      const b = music.bar(t)
      const x = b.phase * perBar
      return { index: b.index * perBar + Math.floor(x), phase: x - Math.floor(x) }
    },
    /** The section at t: { index, id, name, phase } or null. */
    section(t) {
      const i = sections.findIndex((s) => t >= s.from && t < s.to)
      if (i < 0) return null
      const s = sections[i]
      return { index: i, id: s.id, name: s.name, phase: (t - s.from) / Math.max(0.001, s.to - s.from) }
    },
  }
  return music
}
