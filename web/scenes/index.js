// The scenes of this player. To add one: write the module (see orbits.js and AGBW.md §7), import
// it here and add it to the list.

import orbits from './orbits.js'
import spectrum from './spectrum.js'

export const DEFAULT_SCENE = 'orbits'
export const scenes = new Map([orbits, spectrum].map((s) => [s.id, s]))
