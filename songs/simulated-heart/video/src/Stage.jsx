import React, {useLayoutEffect, useMemo, useRef} from 'react';
import {Sequence, staticFile, useCurrentFrame, useDelayRender} from 'remotion';
import {useOffthreadVideoTexture} from '@remotion/three';
import {useThree} from '@react-three/fiber';
import * as THREE from 'three';
import {SHOTS} from './shots';
import {
  FPS, KICK, SNARE, CRASH, GTR_A, GTR_B, LEAD, POWER, GROUPS, TRACKS,
  env, section, sec, warmth, leadLevel, clamp, smooth, mix, hash, lastBefore,
} from './song';
import cloudCold from './data/cloud_android_cold.json';
import cloudWarm from './data/cloud_android_warm.json';

const ASPECT = 16 / 9;
const CYAN = new THREE.Color('#35e0ff');
const PINK = new THREE.Color('#ff4fb4');
const AMBER = new THREE.Color('#ffb23a');

/** A soft round dot, for every kind of point. */
let dotTexture = null;
const dot = () => {
  if (dotTexture) return dotTexture;
  const c = document.createElement('canvas');
  c.width = c.height = 64;
  const g = c.getContext('2d');
  const grad = g.createRadialGradient(32, 32, 0, 32, 32, 32);
  grad.addColorStop(0, 'rgba(255,255,255,1)');
  grad.addColorStop(0.35, 'rgba(255,255,255,0.55)');
  grad.addColorStop(1, 'rgba(255,255,255,0)');
  g.fillStyle = grad;
  g.fillRect(0, 0, 64, 64);
  dotTexture = new THREE.CanvasTexture(c);
  return dotTexture;
};

// ------------------------------------------------------------------ the screen: one Grok clip, graded
const screenVertex = /* glsl */ `
  varying vec2 vUv;
  void main() { vUv = uv; gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0); }
`;
const screenFragment = /* glsl */ `
  uniform sampler2D map;
  uniform float warm, glitch, seed, alpha, zoom, punch;
  varying vec2 vUv;
  float h(float n) { return fract(sin(n * 127.1 + seed * 31.7) * 43758.5453); }
  void main() {
    vec2 uv = (vUv - 0.5) / (1.0 + zoom + punch) + 0.5;
    // the machine's picture tears into rows
    float row = floor(uv.y * 28.0);
    float tear = step(0.72, h(row)) * (h(row + 9.0) - 0.5) * 0.16 * glitch;
    uv.x += tear;
    float split = 0.006 * glitch + 0.0012;
    vec3 c = vec3(texture2D(map, uv + vec2(split, 0.0)).r, texture2D(map, uv).g, texture2D(map, uv - vec2(split, 0.0)).b);
    float l = dot(c, vec3(0.299, 0.587, 0.114));
    vec3 cold = mix(vec3(l) * vec3(0.50, 0.86, 1.12), c, 0.28);
    cold *= 0.9 + 0.1 * step(0.5, fract(uv.y * 180.0));            // faint scan rows, only while cold
    c = mix(cold, c * vec3(1.04, 1.0, 0.96), warm);
    gl_FragColor = vec4(c, alpha);
    #include <colorspace_fragment>
  }
`;

const Screen = ({clip, zoom, t}) => {
  const texture = useOffthreadVideoTexture({src: staticFile(`clips/${clip}.mp4`)});
  // While rendering, the canvas is drawn once per frame, and the frame of the clip arrives after that.
  // Hold the frame until the new texture is in, then draw the canvas again.
  const frame = useCurrentFrame();
  const {advance} = useThree();
  const {delayRender, continueRender} = useDelayRender();
  const hold = useRef(null);
  useLayoutEffect(() => {
    if (hold.current === null) hold.current = delayRender(`clip ${clip}, frame ${frame}`);
    return () => {
      if (hold.current !== null) { continueRender(hold.current); hold.current = null; }
    };
  }, [frame, clip, delayRender, continueRender]);
  useLayoutEffect(() => {
    if (!texture) return;
    advance(performance.now());
    if (hold.current !== null) { continueRender(hold.current); hold.current = null; }
  }, [texture, advance, continueRender]);
  const s = section(t);
  const compute = s.id === 'compute';
  const cut = env(GROUPS, t, 0.05);
  // The material is made here and its uniforms are written every frame. (Uniforms handed to R3F as a
  // prop are copied when the material is mounted: later changes to the object would not reach it.)
  const material = useMemo(() => new THREE.ShaderMaterial({
    vertexShader: screenVertex, fragmentShader: screenFragment, transparent: true,
    uniforms: {map: {value: null}, warm: {value: 0}, glitch: {value: 0}, seed: {value: 0}, alpha: {value: 1}, zoom: {value: 0}, punch: {value: 0}},
  }), []);
  const uniforms = material.uniforms;
  if (!texture) return null;
  uniforms.map.value = texture;
  uniforms.warm.value = warmth(t);
  uniforms.glitch.value = compute ? 0.25 + 0.75 * cut : s.id === 'pattern' ? 0.12 * cut : 0;
  uniforms.seed.value = Math.floor(t * 24);
  uniforms.zoom.value = zoom + 0.05;
  uniforms.punch.value = 0.022 * env(KICK, t, 0.09) * (s.humanity > 0.3 ? 1 : 0.4);
  uniforms.alpha.value = s.id === 'outro' ? clamp(1 - (t - (s.to - 5.2)) / 3.4) * 0.9 + 0.1 * clamp((s.to - t) / 2) : 1;
  return (
    <mesh scale={1.07} material={material}>
      <planeGeometry args={[2 * ASPECT, 2]} />
    </mesh>
  );
};

// ------------------------------------------------------------------ a picture as points
const cloudVertex = /* glsl */ `
  attribute vec3 away;
  attribute float seed;
  uniform float gather, time, size;
  varying vec3 vColor;
  varying float vFade;
  void main() {
    // each point arrives in its own time
    float g = smoothstep(seed * 0.7, seed * 0.7 + 0.3, gather);
    vec3 p = mix(away + 0.06 * vec3(sin(time * 0.7 + seed * 40.0), cos(time * 0.6 + seed * 31.0), sin(time * 0.5 + seed * 17.0)), position, g);
    vec4 mv = modelViewMatrix * vec4(p, 1.0);
    gl_Position = projectionMatrix * mv;
    gl_PointSize = size * (0.6 + 0.8 * fract(seed * 7.0)) / -mv.z;
    vColor = color;
    vFade = 0.25 + 0.75 * g;
  }
`;
const cloudFragment = /* glsl */ `
  uniform float alpha;
  varying vec3 vColor;
  varying float vFade;
  void main() {
    float d = length(gl_PointCoord - 0.5);
    float a = smoothstep(0.5, 0.05, d);
    gl_FragColor = vec4(vColor * a * alpha * vFade * 0.8, 1.0);
    #include <colorspace_fragment>
  }
`;

const Cloud = ({data, gather, alpha, t, tint}) => {
  const geometry = useMemo(() => {
    const n = data.n;
    const pos = new Float32Array(n * 3), col = new Float32Array(n * 3), away = new Float32Array(n * 3), seed = new Float32Array(n);
    for (let i = 0; i < n; i++) {
      for (let k = 0; k < 3; k++) {
        pos[i * 3 + k] = data.pos[i * 3 + k] / 1000;
        col[i * 3 + k] = Math.pow(data.col[i * 3 + k] / 255, 2.2);
      }
      // where it comes from: a shell around the viewer
      const a = hash(i * 3 + 1) * Math.PI * 2, b = Math.acos(2 * hash(i * 3 + 2) - 1), r = 2.2 + 3.5 * hash(i * 3 + 3);
      away[i * 3] = r * Math.sin(b) * Math.cos(a);
      away[i * 3 + 1] = r * Math.cos(b) * 0.7;
      away[i * 3 + 2] = r * Math.sin(b) * Math.sin(a) * 0.6 - 0.5;
      seed[i] = hash(i * 7 + 5);
    }
    const g = new THREE.BufferGeometry();
    g.setAttribute('position', new THREE.BufferAttribute(pos, 3));
    g.setAttribute('color', new THREE.BufferAttribute(col, 3));
    g.setAttribute('away', new THREE.BufferAttribute(away, 3));
    g.setAttribute('seed', new THREE.BufferAttribute(seed, 1));
    return g;
  }, [data]);
  const material = useMemo(() => new THREE.ShaderMaterial({
    vertexShader: cloudVertex, fragmentShader: cloudFragment, vertexColors: true, transparent: true, depthWrite: false, depthTest: false, blending: THREE.AdditiveBlending,
    uniforms: {gather: {value: 0}, time: {value: 0}, size: {value: 19}, alpha: {value: 1}},
  }), []);
  const uniforms = material.uniforms;
  uniforms.gather.value = gather;
  uniforms.time.value = t;
  uniforms.alpha.value = alpha;
  if (alpha <= 0.002) return null;
  return (
    <points geometry={geometry} material={material} position={[0, 0, 0.02]} frustumCulled={false} />
  );
};

// ------------------------------------------------------------------ the lattice: what the machine is thinking
// Nodes in a band around the edge of the picture, in front of it. A note lights its node and the
// threads to its nearest neighbours; the middle of the picture is left to the people in it.
const NODES = 260;
const nodePos = (i) => {
  // a point on the frame: along the top/bottom or the sides, a little inside or outside the edge
  const side = hash(i * 5 + 1) < 0.64;
  const u = hash(i * 5 + 2) * 2 - 1, band = 0.74 + 0.34 * hash(i * 5 + 3), sign = hash(i * 5 + 4) < 0.5 ? -1 : 1;
  const z = 0.15 + 0.75 * hash(i * 5 + 6);
  const k = 1 - z / 3.732; // nearer the camera, the same place on the picture is closer to the axis
  return side ? [u * ASPECT * 1.02 * k, sign * band * k, z] : [sign * band * ASPECT * k, u * 0.98 * k, z];
};
const NODE_POS = Array.from({length: NODES}, (_, i) => nodePos(i));
const NEIGHBOURS = NODE_POS.map((p, i) =>
  NODE_POS.map((q, j) => [Math.hypot(p[0] - q[0], p[1] - q[1], p[2] - q[2]), j]).filter((x) => x[1] !== i).sort((a, b) => a[0] - b[0]).slice(0, 3).map((x) => x[1]));
const nodeOf = (pitch, voice) => (pitch * 13 + voice * 71) % NODES;
const VOICES = [[GTR_A, 0, CYAN], [GTR_B, 1, PINK], [LEAD, 2, AMBER]].map(([notes, voice, c]) => ({notes, voice, c, times: notes.map((n) => n[0])}));

const Lattice = ({t}) => {
  const s = section(t);
  const strength = {boot: 1, pattern: 1, learn: 1, emerge1: 0.5, doubt: 0.6, compute: 1.3, emerge2: 0.5, open: 0.9, heart: 0.55, outro: 0.3}[s.id];
  const warm = warmth(t);
  const {points, lines} = useMemo(() => {
    const pos = new Float32Array(NODES * 3);
    NODE_POS.forEach((p, i) => pos.set(p, i * 3));
    const pg = new THREE.BufferGeometry();
    pg.setAttribute('position', new THREE.BufferAttribute(pos, 3));
    pg.setAttribute('color', new THREE.BufferAttribute(new Float32Array(NODES * 3), 3));
    const lp = new Float32Array(NODES * 3 * 2 * 3);
    NODE_POS.forEach((p, i) => NEIGHBOURS[i].forEach((j, k) => { lp.set(p, (i * 3 + k) * 6); lp.set(NODE_POS[j], (i * 3 + k) * 6 + 3); }));
    const lg = new THREE.BufferGeometry();
    lg.setAttribute('position', new THREE.BufferAttribute(lp, 3));
    lg.setAttribute('color', new THREE.BufferAttribute(new Float32Array(NODES * 3 * 2 * 3), 3));
    return {
      points: new THREE.Points(pg, new THREE.PointsMaterial({size: 0.11, map: dot(), vertexColors: true, transparent: true, depthWrite: false, depthTest: false, blending: THREE.AdditiveBlending, sizeAttenuation: true})),
      lines: new THREE.LineSegments(lg, new THREE.LineBasicMaterial({vertexColors: true, transparent: true, depthWrite: false, depthTest: false, blending: THREE.AdditiveBlending})),
    };
  }, []);
  const level = new Float32Array(NODES * 3);
  const base = CYAN.clone().lerp(AMBER, warm);
  for (let i = 0; i < NODES; i++) { level[i * 3] = base.r * 0.06; level[i * 3 + 1] = base.g * 0.06; level[i * 3 + 2] = base.b * 0.06; }
  for (const v of VOICES) {
    const cc = v.c.clone().lerp(AMBER, warm * 0.6);
    for (let i = lastBefore(v.times, t); i >= 0 && t - v.times[i] < 1.2; i--) {
      const k = nodeOf(v.notes[i][2], v.voice);
      const e = Math.exp(-(t - v.times[i]) / 0.28) * strength * 1.6;
      level[k * 3] += cc.r * e; level[k * 3 + 1] += cc.g * e; level[k * 3 + 2] += cc.b * e;
    }
  }
  points.geometry.attributes.color.array.set(level);
  points.geometry.attributes.color.needsUpdate = true;
  // a thread glows with the node it starts from and fades towards the neighbour
  const lc = lines.geometry.attributes.color.array;
  for (let i = 0; i < NODES; i++) {
    for (let k = 0; k < 3; k++) {
      const o = (i * 3 + k) * 6;
      for (let c = 0; c < 3; c++) { lc[o + c] = Math.max(0, level[i * 3 + c] - [base.r, base.g, base.b][c] * 0.06) * 0.6; lc[o + 3 + c] = lc[o + c] * 0.15; }
    }
  }
  lines.geometry.attributes.color.needsUpdate = true;
  return (
    <group>
      <primitive object={lines} />
      <primitive object={points} />
    </group>
  );
};

// ------------------------------------------------------------------ light beams (the theme)
const Beams = ({t}) => {
  const s = section(t);
  const on = ['emerge1', 'emerge2', 'heart'].includes(s.id) ? smooth((t - s.from) / 0.5) * smooth((s.to - t) / 0.4) : s.id === 'open' ? 0.35 : 0;
  if (on <= 0.01) return null;
  const hit = env(CRASH, t, 0.5), k = env(POWER, t, 0.18);
  return (
    <group position={[0, 1.6, 0.4]}>
      {[-1.5, -0.9, -0.3, 0.3, 0.9, 1.5].map((x, i) => {
        const swing = Math.sin(t * (0.5 + 0.13 * i) + i * 1.7) * 0.45;
        const c = i % 2 ? PINK : AMBER;
        return (
          <mesh key={i} position={[x * 1.15, 0, 0]} rotation={[0.25, 0, swing]}>
            <group position={[0, -1.6, 0]}>
              <mesh>
                <cylinderGeometry args={[0.02, 0.55, 3.2, 20, 1, true]} />
                <meshBasicMaterial color={c} transparent opacity={on * (0.018 + 0.03 * k + 0.035 * hit)} depthWrite={false} blending={THREE.AdditiveBlending} side={THREE.DoubleSide} />
              </mesh>
            </group>
          </mesh>
        );
      })}
    </group>
  );
};

// ------------------------------------------------------------------ dust in the air / stars
const Dust = ({t}) => {
  const s = section(t);
  const geometry = useMemo(() => {
    const n = 420, pos = new Float32Array(n * 3);
    for (let i = 0; i < n; i++) {
      pos[i * 3] = (hash(i * 3 + 11) - 0.5) * 7;
      pos[i * 3 + 1] = (hash(i * 3 + 12) - 0.5) * 4;
      pos[i * 3 + 2] = -2.5 + 5.2 * hash(i * 3 + 13);
    }
    const g = new THREE.BufferGeometry();
    g.setAttribute('position', new THREE.BufferAttribute(pos, 3));
    return g;
  }, []);
  const warm = warmth(t);
  const c = CYAN.clone().lerp(AMBER, warm);
  const amount = s.id === 'boot' ? 0.5 : s.id === 'outro' ? 0.9 : 0.4 - 0.1 * warm;
  return (
    <points geometry={geometry} position={[Math.sin(t * 0.05) * 0.3, (t * 0.02) % 1 - 0.5, 0]} frustumCulled={false}>
      <pointsMaterial size={0.04} map={dot()} color={c} transparent opacity={amount * 0.8} depthWrite={false} blending={THREE.AdditiveBlending} sizeAttenuation />
    </points>
  );
};

// ------------------------------------------------------------------ the heartbeat: a trace drawn by the kick
const pulse = (d) => (d < 0 ? 0 : Math.exp(-Math.pow((d - 0.03) / 0.018, 2)) - 0.35 * Math.exp(-Math.pow((d - 0.085) / 0.03, 2)));
const Heartbeat = ({t}) => {
  const s = section(t);
  const heart = sec('heart');
  const from = heart.from + (heart.to - heart.from) / 2;
  const on = t >= from ? smooth((t - from) / 1.0) * clamp((sec('outro').to - 1.2 - t) / 1.5) : 0;
  const line = useMemo(() => {
    const g = new THREE.BufferGeometry();
    g.setAttribute('position', new THREE.BufferAttribute(new Float32Array(400 * 3), 3));
    return new THREE.Line(g, new THREE.LineBasicMaterial({color: AMBER, transparent: true, depthWrite: false, depthTest: false, blending: THREE.AdditiveBlending}));
  }, []);
  if (on <= 0.01) return null;
  const p = line.geometry.attributes.position.array;
  const span = 2.4; // seconds across the width; now is at the right
  for (let i = 0; i < 400; i++) {
    const x = i / 399;
    const at = t - (1 - x) * span;
    let y = 0;
    let k = lastBefore(KICK, at);
    for (let j = 0; j < 3 && k - j >= 0; j++) y += pulse(at - KICK[k - j]);
    p[i * 3] = (x * 2 - 1) * ASPECT * 0.96;
    p[i * 3 + 1] = -0.74 + 0.16 * y;
    p[i * 3 + 2] = 0.3;
  }
  line.geometry.attributes.position.needsUpdate = true;
  line.material.opacity = on * 0.9;
  return <primitive object={line} />;
};

// ------------------------------------------------------------------ the camera: a tripod that becomes a hand
const Camera = ({t}) => {
  const {camera} = useThree();
  const s = section(t);
  const h = s.humanity;
  const kick = env(KICK, t, 0.1);
  const shake = s.id === 'compute' ? 0.012 * env(GROUPS, t, 0.06) : 0;
  camera.position.set(
    0.05 * h * Math.sin(t * 0.9) + 0.02 * h * Math.sin(t * 2.3) + shake * (hash(Math.floor(t * 60)) - 0.5) * 6,
    0.035 * h * Math.sin(t * 0.7 + 1) + shake * (hash(Math.floor(t * 60) + 3) - 0.5) * 6,
    3.732 - 0.05 * kick * (0.3 + h) + (s.id === 'boot' ? 0.9 * (1 - smooth((t - s.from) / (s.to - s.from))) : 0),
  );
  camera.rotation.set(0.006 * h * Math.sin(t * 1.1), 0.012 * h * Math.sin(t * 0.5), 0.004 * h * Math.sin(t * 0.8));
  camera.updateProjectionMatrix();
  return null;
};

// ------------------------------------------------------------------ all of it
export const Stage = () => {
  const frame = useCurrentFrame();
  const t = frame / FPS;
  const s = section(t);
  const boot = sec('boot'), outro = sec('outro'), doubt = sec('doubt');
  // boot: her portrait gathers as the notes are learned (28 notes), and gives way to the footage
  const learned = clamp((lastBefore(GTR_A.map((n) => n[0]), t) + 1) / 28);
  let coldGather = 0, coldAlpha = 0, warmGather = 0, warmAlpha = 0;
  if (s.id === 'boot') {
    coldGather = smooth(learned) * 0.9 + 0.1 * env(GTR_A.map((n) => n[0]), t, 0.2);
    coldAlpha = 0.3 + 0.7 * learned;
  } else if (s.id === 'pattern') {
    coldGather = 1; coldAlpha = clamp(1 - (t - s.from) / 0.7);
  } else if (s.id === 'doubt') {
    // a feeling forming: the warm portrait gathers only while the melody is being played
    warmGather = 0.15 + 0.85 * leadLevel(t); warmAlpha = 0.3 * smooth((t - doubt.from) / 1.5) * smooth((doubt.to - t) / 1.0);
  } else if (s.id === 'outro') {
    const p = (t - (outro.from + 250 / FPS)) / (outro.to - outro.from - 250 / FPS);
    warmGather = p < 0 ? 0 : 1 - smooth((p - 0.25) / 0.7);
    warmAlpha = p < 0 ? 0 : 0.85 * smooth(p / 0.12) * clamp((outro.to - t) / 1.6);
  }
  return (
    <>
      <Camera t={t} />
      <Dust t={t} />
      {SHOTS.map((shot, i) => (
        <Sequence key={i} from={shot.from} durationInFrames={shot.dur} layout="none">
          <Sequence from={-shot.offset} layout="none">
            <Screen clip={shot.clip} zoom={shot.zoom} t={t} />
          </Sequence>
        </Sequence>
      ))}
      <Lattice t={t} />
      <Beams t={t} />
      <Cloud data={cloudCold} gather={coldGather} alpha={coldAlpha} t={t} />
      <Cloud data={cloudWarm} gather={warmGather} alpha={warmAlpha} t={t} />
      <Heartbeat t={t} />
    </>
  );
};
