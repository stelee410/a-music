// Shared by the scenes: soft glowing points with their own size, colour and brightness.

export function glowPoints(THREE, capacity) {
  const geometry = new THREE.BufferGeometry()
  const position = new Float32Array(capacity * 3)
  const color = new Float32Array(capacity * 3)
  const size = new Float32Array(capacity)
  const alpha = new Float32Array(capacity)
  geometry.setAttribute('position', new THREE.BufferAttribute(position, 3))
  geometry.setAttribute('color', new THREE.BufferAttribute(color, 3))
  geometry.setAttribute('aSize', new THREE.BufferAttribute(size, 1))
  geometry.setAttribute('aAlpha', new THREE.BufferAttribute(alpha, 1))
  const material = new THREE.ShaderMaterial({
    uniforms: { uScale: { value: 1 } },
    vertexShader: `
      attribute float aSize; attribute float aAlpha; attribute vec3 color;
      uniform float uScale; varying float vAlpha; varying vec3 vColor;
      void main() {
        vAlpha = aAlpha; vColor = color;
        vec4 mv = modelViewMatrix * vec4(position, 1.0);
        gl_PointSize = aSize * uScale / max(0.1, -mv.z);
        gl_Position = projectionMatrix * mv;
      }`,
    fragmentShader: `
      varying float vAlpha; varying vec3 vColor;
      void main() {
        float d = length(gl_PointCoord - 0.5) * 2.0;
        float a = smoothstep(1.0, 0.0, d);
        gl_FragColor = vec4(vColor, vAlpha * a * a);
      }`,
    transparent: true,
    depthWrite: false,
    blending: THREE.AdditiveBlending,
  })
  const points = new THREE.Points(geometry, material)
  points.frustumCulled = false
  let n = 0
  return {
    points,
    begin() {
      n = 0
    },
    add(x, y, z, c, s, a) {
      if (n >= capacity) return
      position[n * 3] = x
      position[n * 3 + 1] = y
      position[n * 3 + 2] = z
      color[n * 3] = c.r
      color[n * 3 + 1] = c.g
      color[n * 3 + 2] = c.b
      size[n] = s
      alpha[n] = a
      n++
    },
    end() {
      geometry.setDrawRange(0, n)
      for (const k of ['position', 'color', 'aSize', 'aAlpha']) geometry.attributes[k].needsUpdate = true
    },
    /** Point sizes are in scene units at distance 1: tell the shader the height of the picture. */
    setHeight(pixels, fovDegrees) {
      material.uniforms.uScale.value = pixels / (2 * Math.tan((fovDegrees * Math.PI) / 360))
    },
    dispose() {
      geometry.dispose()
      material.dispose()
    },
  }
}

/** The accent colours of the palette spread over n things. */
export function spread(THREE, palette, n) {
  const accents = palette.slice(1).map((c) => new THREE.Color(c))
  return Array.from({ length: n }, (_, i) => {
    if (accents.length === 1 || n === 1) return accents[0].clone()
    const x = (i / Math.max(1, n - 1)) * (accents.length - 1)
    const k = Math.min(accents.length - 2, Math.floor(x))
    return accents[k].clone().lerp(accents[k + 1], x - k)
  })
}
