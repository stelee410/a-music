// Playing: the worker renders, this schedules what it gets on the audio clock and keeps the
// position. A piece starts as soon as its first bar is there; if the playhead catches up with the
// renderer it waits ("buffering") and goes on by itself.

export class Player extends EventTarget {
  constructor() {
    super()
    this.ctx = null
    this.worker = null
    this.wantPlay = false
    this.info = null
    this.stretches = []
    this.chunks = [] // index → Float32Array
    this.sources = new Map() // index → AudioBufferSourceNode
    this.state = 'empty' // empty | loading | ready | playing | paused | buffering | ended | error
    this.position = 0 // seconds, while not playing
    this.anchor = null // { ctxTime, position } while playing
    this.hop = 800
    this.level = new Float32Array(0)
    this.low = new Float32Array(0)
    this.notes = []
    this.volume = 1
    this.timer = setInterval(() => this.#pump(), 100)
  }

  get sampleRate() {
    return this.info?.sampleRate ?? 48000
  }
  /** Length in seconds, with what rings on after the last bar. */
  get duration() {
    return this.info ? (this.info.lengthSamples + this.info.tailSamples) / this.sampleRate : 0
  }
  get time() {
    if (this.state === 'playing' && this.anchor) return Math.min(this.duration, this.anchor.position + (this.ctx.currentTime - this.anchor.ctxTime))
    return this.position
  }
  /** Stretches rendered so far, as [fromSeconds, toSeconds]. */
  get buffered() {
    return this.stretches.filter((_, i) => this.chunks[i]).map((s) => [s.start / this.sampleRate, s.end / this.sampleRate])
  }
  /** Loudness 0…1 at a time, from the audio itself; `which`: 'level' or 'low'. */
  loudness(t, which = 'level') {
    const a = which === 'low' ? this.low : this.level
    const i = Math.floor((t * this.sampleRate) / this.hop)
    return i >= 0 && i < a.length ? a[i] : 0
  }

  #set(state, detail) {
    this.state = state
    this.dispatchEvent(new CustomEvent('state', { detail: { state, ...detail } }))
  }

  /** Take an opened .agbw (js/agbw.js). The audio context is made at the first play (a click). */
  async load(file, { wasm, transpose = 0 }) {
    this.unload()
    this.#set('loading')
    this.notes = []
    this.worker = new Worker(new URL('./engine-worker.js', import.meta.url), { type: 'module' })
    this.worker.onerror = (e) => this.#set('error', { message: e.message || '渲染线程出错' })
    this.worker.onmessage = (e) => this.#message(e.data)
    this.worker.postMessage({ type: 'load', wasm, song: file.song, samples: [...file.samples], transpose })
  }

  unload() {
    this.#stopSources()
    this.worker?.terminate()
    this.worker = null
    this.wantPlay = false
    this.info = null
    this.stretches = []
    this.chunks = []
    this.position = 0
    this.anchor = null
    this.level = new Float32Array(0)
    this.low = new Float32Array(0)
    this.state = 'empty'
  }

  #message(m) {
    if (m.type === 'ready') {
      this.info = m.info
      this.stretches = m.stretches
      this.hop = m.hop
      const n = Math.ceil((m.info.lengthSamples + m.info.tailSamples) / m.hop) + 1
      this.level = new Float32Array(n)
      this.low = new Float32Array(n)
      this.dispatchEvent(new CustomEvent('info', { detail: m.info }))
    } else if (m.type === 'chunk') {
      this.chunks[m.index] = m.audio
      this.level.set(m.level.subarray(0, Math.max(0, this.level.length - m.first)), m.first)
      this.low.set(m.low.subarray(0, Math.max(0, this.low.length - m.first)), m.first)
      if (this.state === 'loading') this.#set(this.wantPlay ? 'buffering' : 'ready')
      this.dispatchEvent(new CustomEvent('progress'))
      this.#pump()
    } else if (m.type === 'note') {
      this.notes.push(m.message)
    } else if (m.type === 'error') {
      this.#set('error', { message: m.message })
    }
  }

  #indexAt(seconds) {
    const s = seconds * this.sampleRate
    const i = this.stretches.findIndex((x) => s >= x.start && s < x.end)
    return i < 0 ? this.stretches.length - 1 : i
  }

  #stopSources() {
    for (const s of this.sources.values()) {
      s.onended = null
      try {
        s.stop()
      } catch {}
      s.disconnect()
    }
    this.sources.clear()
  }

  /** Schedule every rendered stretch that follows on from the playhead without a hole. */
  #pump() {
    if (!this.info || !this.ctx) return
    if (this.state === 'buffering') {
      if (!this.chunks[this.#indexAt(this.position)]) return
      this.anchor = { ctxTime: this.ctx.currentTime + 0.06, position: this.position }
      this.#set('playing')
    }
    if (this.state !== 'playing') return
    const now = this.time
    if (now >= this.duration - 0.001) {
      this.#stopSources()
      this.position = this.duration
      this.anchor = null
      this.wantPlay = false
      this.#set('ended')
      return
    }
    const sr = this.sampleRate
    let i = this.#indexAt(Math.max(now, this.anchor.position))
    if (!this.chunks[i] && this.ctx.currentTime >= this.anchor.ctxTime) {
      // The playhead has caught up with the renderer.
      this.#stopSources()
      this.position = now
      this.anchor = null
      this.worker.postMessage({ type: 'want', index: i })
      this.#set('buffering')
      return
    }
    for (; i < this.stretches.length && this.chunks[i]; i++) {
      if (this.sources.has(i)) continue
      const st = this.stretches[i]
      const audio = this.chunks[i]
      const buffer = this.ctx.createBuffer(1, audio.length, sr)
      buffer.copyToChannel(audio, 0)
      const src = this.ctx.createBufferSource()
      src.buffer = buffer
      src.connect(this.gain)
      // Where this stretch falls on the audio clock; a stretch already under way starts inside.
      const at = this.anchor.ctxTime + (st.start / sr - this.anchor.position)
      const late = Math.max(0, this.ctx.currentTime + 0.02 - at)
      if (late >= audio.length / sr) continue
      src.start(at + late, late)
      this.sources.set(i, src)
    }
  }

  async play() {
    if (!this.ctx) {
      // Made here, inside the click that asks for sound, as browsers require.
      const AC = window.AudioContext || window.webkitAudioContext
      // At the engine's rate when the browser allows it: nothing is resampled.
      try {
        this.ctx = new AC({ sampleRate: this.sampleRate, latencyHint: 'playback' })
      } catch {
        this.ctx = new AC()
      }
      this.gain = this.ctx.createGain()
      this.gain.gain.value = this.volume
      this.gain.connect(this.ctx.destination)
    }
    this.wantPlay = true
    if (this.ctx.state !== 'running') await this.ctx.resume().catch(() => {})
    if (!this.info || !this.chunks.some(Boolean)) return // starts with the first stretch
    if (this.state === 'playing') return
    if (this.state === 'ended' || this.position >= this.duration - 0.001) this.position = 0
    this.worker.postMessage({ type: 'want', index: this.#indexAt(this.position) })
    this.#set('buffering')
    this.#pump()
  }

  pause() {
    this.wantPlay = false
    if (this.state !== 'playing' && this.state !== 'buffering') return
    this.position = this.time
    this.anchor = null
    this.#stopSources()
    this.#set('paused')
  }

  toggle() {
    if (this.state === 'playing' || this.state === 'buffering') this.pause()
    else this.play()
  }

  seek(seconds) {
    if (!this.info) return
    const going = this.state === 'playing' || this.state === 'buffering'
    this.#stopSources()
    this.position = Math.min(Math.max(0, seconds), Math.max(0, this.duration - 0.01))
    this.anchor = null
    this.worker.postMessage({ type: 'want', index: this.#indexAt(this.position) })
    if (going) {
      this.#set('buffering')
      this.#pump()
    } else {
      if (this.state === 'ended') this.#set('paused')
      this.dispatchEvent(new CustomEvent('progress'))
    }
  }

  setVolume(v) {
    this.volume = Math.min(1, Math.max(0, v))
    if (this.gain) this.gain.gain.value = this.volume
  }
}
