// The web instruments of this player. To add one: write the module (see epiano.js and AGBW.md §6),
// import it here and add it to the list.

import epiano from './epiano.js'
import pluck from './pluck.js'

export const instruments = new Map([epiano, pluck].map((d) => [d.id, d]))
