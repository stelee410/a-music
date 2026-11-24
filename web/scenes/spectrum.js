// spectrum — a skyline of the notes: one column per pitch, as high as that pitch was just played,
// in the colour of the track that played it, mirrored in the floor. The floor's lines come towards
// the viewer in time with the beat, and the horizon glows with the drums.

import { spread } from './lib.js'

const WIDTH = 15

export default {
  id: 'spectrum',
  name: '天际线',
  create({ THREE, palette, music }) {
    const scene = new THREE.Scene()
    scene.fog = new THREE.Fog(new THREE.Color(palette[0]), 9, 30)
    const camera = new THREE.PerspectiveCamera(46, 16 / 9, 0.1, 80)
    const tonal = music.tracks.filter((t) => t.notes.length && !t.drums)
    const drums = music.tracks.filter((t) => t.notes.length && t.drums)
    const colors = spread(THREE, palette, Math.max(1, tonal.length))
    // The pitches the song uses, and never fewer than three octaves: the skyline fills the width.
    let LOW = Math.min(...tonal.map((t) => t.lowest), 48)
    let HIGH = Math.max(...tonal.map((t) => t.highest), 72)
    const short = 36 - (HIGH - LOW)
    if (short > 0) {
      LOW -= Math.floor(short / 2)
      HIGH += Math.ceil(short / 2)
    }
    const COLUMNS = HIGH - LOW + 1

    const box = new THREE.BoxGeometry(1, 1, 1)
    box.translate(0, 0.5, 0) // grows upwards from the floor
    const columns = new THREE.InstancedMesh(box, new THREE.MeshBasicMaterial({ transparent: true, opacity: 0.95 }), COLUMNS)
    const mirror = new THREE.InstancedMesh(box, new THREE.MeshBasicMaterial({ transparent: true, opacity: 0.22 }), COLUMNS)
    columns.instanceMatrix.setUsage(THREE.DynamicDrawUsage)
    mirror.instanceMatrix.setUsage(THREE.DynamicDrawUsage)
    scene.add(columns, mirror)

    // The floor: lines across, moving with the beat, and lines away to the horizon.
    const floor = []
    const lineMat = new THREE.LineBasicMaterial({ color: new THREE.Color(palette[3]), transparent: true, opacity: 0.25 })
    const across = new THREE.Group()
    for (let i = 0; i < 24; i++) {
      const g = new THREE.BufferGeometry().setFromPoints([new THREE.Vector3(-WIDTH, 0, 0), new THREE.Vector3(WIDTH, 0, 0)])
      const l = new THREE.Line(g, lineMat)
      floor.push(l)
      across.add(l)
    }
    scene.add(across)
    const away = new THREE.Group()
    for (let i = -14; i <= 14; i++) {
      const g = new THREE.BufferGeometry().setFromPoints([new THREE.Vector3(i, 0, 6), new THREE.Vector3(i, 0, -30)])
      away.add(new THREE.Line(g, lineMat))
    }
    scene.add(away)

    const glow = new THREE.Mesh(
      new THREE.PlaneGeometry(60, 14),
      new THREE.ShaderMaterial({
        uniforms: { uColor: { value: new THREE.Color(palette[1]) }, uPower: { value: 0 } },
        vertexShader: 'varying vec2 vUv; void main(){ vUv = uv; gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0); }',
        fragmentShader: 'uniform vec3 uColor; uniform float uPower; varying vec2 vUv; void main(){ float y = abs(vUv.y - 0.5) * 2.0; float x = abs(vUv.x - 0.5) * 2.0; float a = pow(1.0 - y, 3.0) * (1.0 - x * x); gl_FragColor = vec4(uColor, a * uPower); }',
        transparent: true,
        depthWrite: false,
        blending: THREE.AdditiveBlending,
        fog: false,
      }),
    )
    glow.position.set(0, 0, -22)
    scene.add(glow)

    const height = new Float32Array(COLUMNS)
    const tint = Array.from({ length: COLUMNS }, () => new THREE.Color())
    const m = new THREE.Matrix4()
    const dark = new THREE.Color(palette[0])
    const idle = new THREE.Color(palette[3])
    const c = new THREE.Color()
    const dx = WIDTH / COLUMNS

    return {
      scene,
      camera,
      update({ t, level, low, beat }) {
        height.fill(0)
        for (let k = 0; k < tonal.length; k++) {
          for (const n of music.recent(tonal[k], t, 4)) {
            const col = Math.round(n[2]) - LOW
            if (col < 0 || col >= COLUMNS) continue
            const age = t - n[0]
            // Struck, then held at a lower level for as long as the note lasts, then gone quickly.
            const struck = Math.exp(-age / 0.3)
            const held = age < n[1] ? 0.55 : 0.55 * Math.exp(-(age - n[1]) / 0.18)
            const e = n[3] * Math.max(struck, held)
            if (e > height[col]) {
              height[col] = e
              tint[col].copy(colors[k])
            }
          }
        }
        for (let i = 0; i < COLUMNS; i++) {
          const x = (i - (COLUMNS - 1) / 2) * dx
          const h = 0.04 + height[i] * 4.6
          m.makeScale(dx * 0.78, h, dx * 0.78).setPosition(x, 0, -4)
          columns.setMatrixAt(i, m)
          m.makeScale(dx * 0.78, -h * 0.8, dx * 0.78).setPosition(x, 0, -4)
          mirror.setMatrixAt(i, m)
          c.copy(dark).lerp(tint[i], height[i] > 0.001 ? 0.35 + 0.65 * Math.min(1, height[i] * 1.3) : 0.12)
          if (height[i] <= 0.001) c.lerp(idle, 0.18)
          columns.setColorAt(i, c)
          mirror.setColorAt(i, c)
        }
        columns.instanceMatrix.needsUpdate = true
        mirror.instanceMatrix.needsUpdate = true
        columns.instanceColor.needsUpdate = true
        mirror.instanceColor.needsUpdate = true

        // One line passes for every beat.
        for (let i = 0; i < floor.length; i++) floor[i].position.z = 6 - (i - beat.phase) * 1.5
        let kick = 0
        for (const d of drums) kick = Math.max(kick, music.pulse(d, t, 0.14))
        lineMat.opacity = 0.14 + 0.3 * kick
        glow.material.uniforms.uPower.value = 0.12 + Math.min(0.75, low * 4 + kick * 0.25) + Math.min(0.2, level)
        camera.position.set(Math.sin(t * 0.13) * 1.4, 2.0 + Math.sin(t * 0.09) * 0.3 + kick * 0.05, 5.6)
        camera.lookAt(0, 1.7, -4)
      },
      dispose() {
        box.dispose()
        columns.material.dispose()
        mirror.material.dispose()
        lineMat.dispose()
        glow.geometry.dispose()
        glow.material.dispose()
      },
    }
  },
}
