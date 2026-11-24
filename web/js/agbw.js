// Opening an .agbw (AGBW.md): the manifest, the song for the engine, the timeline to draw from,
// the samples and the cover.

import { readZip } from './zip.js'

export const MIMETYPE = 'application/vnd.agb.web+zip'
/** The major version of the format this player reads. */
export const FORMAT_MAJOR = 0

const utf8 = new TextDecoder()
// Nothing in an .agbw is ever used as a path outside the file; this only keeps odd names out.
const safe = (p) => typeof p === 'string' && p.length > 0 && !p.startsWith('/') && !p.includes('\\') && !p.split('/').includes('..')

export async function openAgbw(buffer) {
  const zip = readZip(buffer)
  const need = async (name) => {
    const e = zip.get(name)
    if (!e) throw new Error(`.agbw 里缺少 ${name}`)
    return e.read()
  }
  const mime = utf8.decode(await need('mimetype')).trim()
  if (mime !== MIMETYPE) throw new Error('这不是 .agbw 文件（要播放 .agbsong，请先用 agb song web 转换）')
  const manifest = JSON.parse(utf8.decode(await need('web.json')))
  if (manifest.format !== 'agbw') throw new Error('web.json：format 不是 agbw')
  const major = Number(String(manifest.formatVersion).split('.')[0])
  if (major !== FORMAT_MAJOR) throw new Error(`这个播放器读不了 agbw ${manifest.formatVersion} 版的文件`)
  const songName = manifest.song || 'song.json'
  const timelineName = manifest.timeline || 'timeline.json'
  if (!safe(songName) || !safe(timelineName)) throw new Error('web.json：文件名不合法')
  const song = await need(songName)
  const timeline = JSON.parse(utf8.decode(await need(timelineName)))
  const assets = JSON.parse(utf8.decode(song)).assets || {}

  // The samples of the song, by asset id. Anything that is not a WAV the engine would refuse anyway.
  const samples = new Map()
  for (const [id, a] of Object.entries(assets)) {
    if (!safe(a.path) || !/\.wav$/i.test(a.path) || !zip.has(a.path)) continue
    samples.set(id, await zip.get(a.path).read())
  }

  let cover = null
  if (safe(manifest.cover) && zip.has(manifest.cover)) {
    const type = /\.svg$/i.test(manifest.cover) ? 'image/svg+xml' : /\.png$/i.test(manifest.cover) ? 'image/png' : /\.jpe?g$/i.test(manifest.cover) ? 'image/jpeg' : null
    // Shown through <img>, where an SVG cannot run scripts or load anything.
    if (type) cover = new Blob([await zip.get(manifest.cover).read()], { type })
  }
  return { manifest, song, timeline, samples, cover }
}
