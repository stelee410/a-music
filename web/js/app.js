// The page: the list of songs, the stage, the transport, the lyrics.

import { openAgbw } from './agbw.js'
import { Player } from './player.js'
import { Stage, paletteOf } from './stage.js'
import { instruments } from '../instruments/index.js'

const $ = (id) => document.getElementById(id)
const el = (tag, props = {}, ...kids) => {
  const e = Object.assign(document.createElement(tag), props)
  e.append(...kids)
  return e
}
const clock = (s) => `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, '0')}`

const player = new Player()
const stage = new Stage($('canvas'))
let wasm = null // the engine, fetched once
let current = null // the opened .agbw
let coverUrl = null
let lyricLine = null

window.agb = { player, stage } // for looking at it from the console

async function engineBytes() {
  if (!wasm) {
    const r = await fetch(new URL('../agb.wasm', import.meta.url))
    if (!r.ok) throw new Error('找不到引擎 agb.wasm（先运行 web/build.sh）')
    wasm = await r.arrayBuffer()
  }
  return wasm
}

function fail(message) {
  $('error').textContent = message
  $('error').hidden = !message
}

/** Open a song from bytes; `autoplay` only inside a click. */
async function openSong(buffer, { autoplay = false, label = null } = {}) {
  fail('')
  try {
    const file = await openAgbw(buffer)
    const missing = (file.manifest.instruments || []).filter((id) => !instruments.has(id))
    current = file
    lyricLine = null
    $('lyrics').replaceChildren()
    showAbout(file, missing)
    stage.setSong(file, () => {
      const t = player.time
      return { t, playing: player.state === 'playing', level: player.loudness(t), low: player.loudness(t, 'low') }
    })
    await player.load(file, { wasm: await engineBytes() })
    if (autoplay) player.play()
    for (const b of $('list').querySelectorAll('button')) b.setAttribute('aria-current', String(b.dataset.label === label))
  } catch (e) {
    fail(String(e.message || e))
  }
}

function showAbout(file, missing) {
  const m = file.manifest
  const p = paletteOf(m)
  const root = document.documentElement.style
  root.setProperty('--bg', p[0])
  root.setProperty('--a', p[1])
  root.setProperty('--b', p[2])
  root.setProperty('--c', p[3])
  document.title = `${m.title} · Agent Groovebox`
  $('title').textContent = m.title || '无题'
  $('byline').textContent = (m.artists || []).map((a) => (a.model ? `${a.name}（${a.model}）` : a.name)).join('、')
  $('facts').textContent = [m.key && `${m.key}${m.scale ? ' ' + m.scale : ''}`, m.bpm && `${Math.round(m.bpm)} BPM`, m.seconds && clock(m.seconds), m.license].filter(Boolean).join(' · ')
  $('description').textContent = m.description || ''
  if (coverUrl) URL.revokeObjectURL(coverUrl)
  coverUrl = file.cover ? URL.createObjectURL(file.cover) : null
  $('cover').hidden = !coverUrl
  if (coverUrl) $('cover').src = coverUrl
  $('tracks').replaceChildren(
    ...(file.timeline.tracks || []).map((t) => {
      const web = t.engine === 'plugin.web'
      return el('li', { className: web ? 'web' : '' }, el('b', { textContent: t.name || t.id }), el('span', { textContent: web ? '网页乐器' : t.engine || '' }))
    }),
  )
  $('notes').replaceChildren(...missing.map((id) => el('li', { textContent: `这个播放器没有乐器 ${id}，相应的音轨用备用音色演奏` })))
  $('marks').replaceChildren(
    ...(file.timeline.sections || []).slice(1).map((s) => el('i', { title: s.name, style: `left:${(s.from / Math.max(1, m.seconds)) * 100}%` })),
  )
  $('about').hidden = false
  $('total').textContent = clock(m.seconds || 0)
}

// --- transport ---------------------------------------------------------------------------------

player.addEventListener('state', (e) => {
  const s = e.detail.state
  const going = s === 'playing' || s === 'buffering'
  $('play').disabled = s === 'empty' || s === 'error'
  // SVG elements have no `hidden` property: the attribute is set by hand.
  $('icon-play').toggleAttribute('hidden', going)
  $('icon-pause').toggleAttribute('hidden', !going)
  $('big').hidden = !(s === 'ready' || s === 'ended')
  $('busy').hidden = !(s === 'loading' || s === 'buffering')
  if (s === 'error') fail(`无法播放：${e.detail.message}`)
})
player.addEventListener('info', (e) => {
  $('total').textContent = clock(player.duration)
  $('seek').setAttribute('aria-valuemax', String(Math.round(player.duration)))
  const skipped = (e.detail.skipped || []).map((x) => x.message)
  $('notes').append(...skipped.map((t) => el('li', { textContent: t })))
})
player.addEventListener('progress', drawBuffered)

function drawBuffered() {
  const d = player.duration || 1
  $('buffered').replaceChildren(...player.buffered.map(([a, b]) => el('i', { style: `left:${(a / d) * 100}%;width:${((b - a) / d) * 100}%` })))
}

$('play').onclick = () => player.toggle()
$('big').onclick = () => player.play()
$('volume').oninput = (e) => player.setVolume(Number(e.target.value))
$('full').onclick = toggleFull

function toggleFull() {
  if (document.fullscreenElement) document.exitFullscreen()
  else $('stage').requestFullscreen?.()
}

// The seek bar: click, drag, arrow keys.
{
  const seek = $('seek')
  const at = (e) => {
    const r = seek.getBoundingClientRect()
    return Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)) * player.duration
  }
  seek.onpointerdown = (e) => {
    if (!player.info) return
    seek.setPointerCapture(e.pointerId)
    player.seek(at(e))
    seek.onpointermove = (m) => player.seek(at(m))
  }
  seek.onpointerup = seek.onpointercancel = () => (seek.onpointermove = null)
}
window.addEventListener('keydown', (e) => {
  if (e.target instanceof HTMLInputElement || e.metaKey || e.ctrlKey || e.altKey) return
  if (e.key === ' ') {
    e.preventDefault()
    if (player.info) player.toggle()
  } else if (e.key === 'ArrowLeft') player.seek(player.time - 5)
  else if (e.key === 'ArrowRight') player.seek(player.time + 5)
  else if (e.key === 'f' || e.key === 'F') toggleFull()
})

// In full screen the pointer hides when it rests.
{
  let timer = 0
  $('stage').addEventListener('pointermove', () => {
    $('stage').classList.remove('idle')
    clearTimeout(timer)
    timer = setTimeout(() => $('stage').classList.add('idle'), 2000)
  })
  $('stage').addEventListener('dblclick', toggleFull)
}

// --- every frame: the playhead and the lyrics -----------------------------------------------------

function tick() {
  requestAnimationFrame(tick)
  if (!player.info) return
  const t = player.time
  const d = player.duration || 1
  $('now').textContent = clock(t)
  $('played').style.width = `${(t / d) * 100}%`
  $('knob').style.left = `${(t / d) * 100}%`
  $('seek').setAttribute('aria-valuenow', String(Math.round(t)))
  drawLyrics(t)
}
requestAnimationFrame(tick)

/** The line being sung: shown a moment before its first word and held a moment after its last. */
function drawLyrics(t) {
  const lines = current?.timeline.lyrics || []
  const line = lines.find((l) => t >= l.from - 0.35 && t <= l.to + 0.7) || null
  const box = $('lyrics')
  if (line !== lyricLine) {
    lyricLine = line
    if (line) {
      const cjk = (s) => /[⺀-鿿가-힯＀-￯]/.test(s)
      box.replaceChildren(
        ...line.words.flatMap((w, i) => {
          const span = el('span', { textContent: w[2] })
          const prev = i > 0 ? line.words[i - 1][2] : ''
          return i > 0 && !(cjk(prev.slice(-1)) && cjk(w[2][0])) ? [' ', span] : [span]
        }),
      )
    }
    box.classList.toggle('on', !!line)
  }
  if (line) {
    const spans = box.querySelectorAll('span')
    line.words.forEach((w, i) => {
      spans[i].classList.toggle('sung', t >= w[0])
      spans[i].classList.toggle('now', t >= w[0] && t < w[1])
    })
  }
}

// --- where songs come from ----------------------------------------------------------------------

async function openUrl(url, opts) {
  fail('')
  try {
    const r = await fetch(url)
    if (!r.ok) throw new Error(`下载失败（${r.status}）：${url}`)
    await openSong(await r.arrayBuffer(), opts)
  } catch (e) {
    fail(String(e.message || e))
  }
}

$('file').onchange = async (e) => {
  const f = e.target.files[0]
  if (f) openSong(await f.arrayBuffer(), { autoplay: true })
  e.target.value = ''
}
window.addEventListener('dragover', (e) => {
  e.preventDefault()
  $('drop').hidden = false
})
window.addEventListener('dragleave', (e) => {
  if (!e.relatedTarget) $('drop').hidden = true
})
window.addEventListener('drop', async (e) => {
  e.preventDefault()
  $('drop').hidden = true
  const f = e.dataTransfer.files[0]
  if (f) openSong(await f.arrayBuffer(), { autoplay: true })
})

async function start() {
  let songs = []
  try {
    const r = await fetch('songs/index.json')
    if (r.ok) songs = await r.json()
  } catch {}
  const list = $('list')
  if (!songs.length) list.append(el('li', { className: 'empty', textContent: '还没有曲子：打开一个 .agbw 文件' }))
  for (const s of songs) {
    const b = el('button', {}, el('span', { className: 'name', textContent: s.title }), el('span', { className: 'sub', textContent: [s.artist, s.seconds && clock(s.seconds)].filter(Boolean).join(' · ') }))
    b.dataset.label = s.file
    b.onclick = () => openUrl(`songs/${s.file}`, { autoplay: true, label: s.file })
    list.append(el('li', {}, b))
  }
  const asked = new URLSearchParams(location.search).get('song')
  if (asked) openUrl(asked, { label: asked.replace(/^songs\//, '') })
  else if (songs.length) openUrl(`songs/${songs[0].file}`, { label: songs[0].file })
}
start()
