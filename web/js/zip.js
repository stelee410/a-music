// Reading a zip held in memory: just what an .agbw needs (stored and deflated entries, no zip64,
// no encryption). Works in browsers and in Node.

const u16 = (v, o) => v.getUint16(o, true)
const u32 = (v, o) => v.getUint32(o, true)

/** The entries of a zip: Map of name → { size, read(): Promise<Uint8Array> }. */
export function readZip(buffer) {
  const bytes = buffer instanceof Uint8Array ? buffer : new Uint8Array(buffer)
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
  // The end record is at the very end, after at most a 64 KiB comment.
  let end = -1
  for (let i = bytes.length - 22; i >= Math.max(0, bytes.length - 22 - 65535); i--) {
    if (u32(view, i) === 0x06054b50) {
      end = i
      break
    }
  }
  if (end < 0) throw new Error('不是 zip 文件')
  const count = u16(view, end + 10)
  let at = u32(view, end + 16)
  const entries = new Map()
  const decoder = new TextDecoder()
  for (let n = 0; n < count; n++) {
    if (at + 46 > bytes.length || u32(view, at) !== 0x02014b50) throw new Error('zip 目录损坏')
    const method = u16(view, at + 10)
    const packed = u32(view, at + 20)
    const size = u32(view, at + 24)
    const nameLen = u16(view, at + 28)
    const extraLen = u16(view, at + 30)
    const commentLen = u16(view, at + 32)
    const local = u32(view, at + 42)
    const name = decoder.decode(bytes.subarray(at + 46, at + 46 + nameLen))
    at += 46 + nameLen + extraLen + commentLen
    if (name.endsWith('/')) continue
    entries.set(name, {
      size,
      async read() {
        if (u32(view, local) !== 0x04034b50) throw new Error(`zip 条目损坏：${name}`)
        const start = local + 30 + u16(view, local + 26) + u16(view, local + 28)
        const data = bytes.subarray(start, start + packed)
        if (method === 0) return data
        if (method !== 8) throw new Error(`${name}：不支持的压缩方式 ${method}`)
        const stream = new Blob([data]).stream().pipeThrough(new DecompressionStream('deflate-raw'))
        const out = new Uint8Array(await new Response(stream).arrayBuffer())
        if (out.length !== size) throw new Error(`${name}：解压后长度不对`)
        return out
      },
    })
  }
  return entries
}
