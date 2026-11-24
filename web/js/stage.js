// The stage: one WebGL canvas, and the scene (scenes/, AGBW.md §7) that draws the song on it.

import * as THREE from '../vendor/three.module.min.js'
import { scenes, DEFAULT_SCENE } from '../scenes/index.js'
import { makeMusic } from './music.js'

const FALLBACK_PALETTE = ['#0b0d14', '#e8825a', '#f4ead8', '#5fb8b0']
const isColor = (c) => typeof c === 'string' && /^#[0-9a-f]{6}$/i.test(c)

/** Four colours whatever the file says: background, then three accents. */
export function paletteOf(manifest) {
  const given = (manifest?.visual?.palette || []).filter(isColor)
  return FALLBACK_PALETTE.map((c, i) => given[i] || (i > 0 && given.length > 1 ? given[1 + ((i - 1) % (given.length - 1))] : c))
}

export class Stage {
  constructor(canvas) {
    this.canvas = canvas
    this.renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: false, powerPreference: 'high-performance' })
    this.renderer.setPixelRatio(Math.min(2, window.devicePixelRatio || 1))
    this.live = null
    this.clock = () => ({ t: 0, playing: false, level: 0, low: 0 })
    this.last = 0
    this.frames = 0
    new ResizeObserver(() => this.resize()).observe(canvas)
    const loop = (now) => {
      this.draw(now)
      requestAnimationFrame(loop)
    }
    requestAnimationFrame(loop)
  }

  /** Show a song. `clock()` answers { t, playing, level, low } whenever a frame is drawn. */
  setSong(file, clock) {
    this.live?.dispose?.()
    this.live = null
    this.clock = clock
    const palette = paletteOf(file.manifest)
    const wanted = file.manifest?.visual?.scene
    const def = scenes.get(wanted) || scenes.get(DEFAULT_SCENE)
    this.sceneId = def.id
    this.music = makeMusic(file.timeline)
    this.renderer.setClearColor(new THREE.Color(palette[0]), 1)
    this.live = def.create({ THREE, renderer: this.renderer, palette, music: this.music, manifest: file.manifest })
    this.resize()
  }

  resize() {
    const w = Math.max(2, this.canvas.clientWidth)
    const h = Math.max(2, this.canvas.clientHeight)
    this.renderer.setPixelRatio(Math.min(2, window.devicePixelRatio || 1))
    this.renderer.setSize(w, h, false)
    const cam = this.live?.camera
    if (cam?.isPerspectiveCamera) {
      cam.aspect = w / h
      cam.updateProjectionMatrix()
    }
    this.live?.resize?.(w, h)
  }

  draw(now) {
    if (!this.live) return
    const c = this.clock()
    const dt = Math.min(0.1, (now - this.last) / 1000)
    this.last = now
    const m = this.music
    this.live.update({ t: c.t, dt, playing: c.playing, level: c.level, low: c.low, bar: m.bar(c.t), beat: m.beat(c.t), section: m.section(c.t) })
    this.renderer.render(this.live.scene, this.live.camera)
    this.frames++
  }
}
