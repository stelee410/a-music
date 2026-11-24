// orbits — every track is a ring. A point runs round each ring, the rings at different speeds
// (one, two, three bars a turn), and every note is left behind as a light where the point was when
// it sounded. The middle breathes with the low end.

import { glowPoints, spread } from './lib.js'

const FOV = 38
const TAU = Math.PI * 2

export default {
  id: 'orbits',
  name: '轨道',
  create({ THREE, renderer, palette, music }) {
    const scene = new THREE.Scene()
    const camera = new THREE.PerspectiveCamera(FOV, 16 / 9, 0.1, 100)
    const tracks = music.tracks.filter((t) => t.notes.length)
    const colors = spread(THREE, palette, Math.max(1, tracks.length))
    const group = new THREE.Group()
    scene.add(group)

    const inner = 0.75
    const step = Math.min(0.5, 3.1 / Math.max(1, tracks.length))
    const outer = inner + step * Math.max(0, tracks.length - 1)
    const rings = tracks.map((track, i) => {
      const radius = inner + i * step
      const pts = []
      for (let k = 0; k <= 160; k++) pts.push(new THREE.Vector3(Math.cos((k / 160) * TAU) * radius, Math.sin((k / 160) * TAU) * radius, 0))
      const line = new THREE.Line(
        new THREE.BufferGeometry().setFromPoints(pts),
        new THREE.LineBasicMaterial({ color: colors[i], transparent: true, opacity: 0.15, blending: THREE.AdditiveBlending, depthWrite: false }),
      )
      group.add(line)
      // A turn takes one, two or three bars: rings side by side drift against each other.
      return { track, radius, line, bars: 1 + (i % 3), color: colors[i] }
    })

    const lights = glowPoints(THREE, 4096)
    group.add(lights.points)
    const white = new THREE.Color(palette[2])
    const tmp = new THREE.Color()

    const core = new THREE.Mesh(
      new THREE.CircleGeometry(1, 64),
      new THREE.ShaderMaterial({
        uniforms: { uColor: { value: new THREE.Color(palette[1]) }, uPower: { value: 0 } },
        vertexShader: 'varying vec2 vUv; void main(){ vUv = uv; gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0); }',
        fragmentShader: 'uniform vec3 uColor; uniform float uPower; varying vec2 vUv; void main(){ float d = length(vUv - 0.5) * 2.0; float a = pow(max(0.0, 1.0 - d), 2.2); gl_FragColor = vec4(uColor, a * uPower); }',
        transparent: true,
        depthWrite: false,
        blending: THREE.AdditiveBlending,
      }),
    )
    scene.add(core)

    const angleAt = (ring, time, barLength) => Math.PI / 2 - (time / (barLength * ring.bars)) * TAU

    return {
      scene,
      camera,
      resize(w, h) {
        lights.setHeight(h * renderer.getPixelRatio(), FOV)
        // Far enough back that the outermost ring fits a wide and a tall picture alike.
        const fit = (outer + 0.7) / Math.tan((FOV * Math.PI) / 360)
        camera.position.set(0, 0, fit * Math.max(1, 1 / Math.min(1, w / h)))
      },
      update({ t, level, low, bar }) {
        const barLength = bar.length
        const fade = barLength * 2.2
        lights.begin()
        for (const ring of rings) {
          const hit = music.pulse(ring.track, t, 0.18)
          ring.line.material.opacity = 0.16 + 0.55 * hit
          for (const n of music.recent(ring.track, t, fade * 3)) {
            const age = t - n[0]
            const a = angleAt(ring, n[0], barLength)
            const spanPitch = Math.max(1, ring.track.highest - ring.track.lowest)
            const r = ring.radius + ((n[2] - ring.track.lowest) / spanPitch - 0.5) * step * 0.5
            const fresh = Math.exp(-age / 0.12)
            const glow = n[3] * Math.exp(-age / fade)
            tmp.copy(ring.color).lerp(white, fresh * 0.8)
            lights.add(Math.cos(a) * r, Math.sin(a) * r, 0, tmp, 0.11 + 0.3 * fresh * n[3] + 0.08 * n[3], Math.min(1, 0.12 + glow + fresh * 0.6))
          }
          // The point that runs round the ring.
          const a = angleAt(ring, t, barLength)
          lights.add(Math.cos(a) * ring.radius, Math.sin(a) * ring.radius, 0, white, 0.09 + 0.14 * hit, 0.6 + 0.4 * hit)
        }
        lights.end()
        const breath = Math.min(1, low * 5)
        core.scale.setScalar(inner * (0.75 + 0.5 * breath) + Math.min(1, level * 3) * 0.2)
        core.material.uniforms.uPower.value = 0.1 + 0.6 * breath + 0.25 * Math.min(1, level * 3)
        group.rotation.z = -t * 0.02
        group.rotation.x = Math.sin(t * 0.11) * 0.22
        group.rotation.y = Math.cos(t * 0.07) * 0.22
      },
      dispose() {
        lights.dispose()
        core.geometry.dispose()
        core.material.dispose()
        for (const r of rings) {
          r.line.geometry.dispose()
          r.line.material.dispose()
        }
      },
    }
  },
}
