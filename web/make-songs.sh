#!/bin/sh
# Make the .agbw files of the web player from the songs of this repository, and the list it shows.
# Songs whose .agbsong is not here (the large ones are kept out of git) are passed over.
set -e
cd "$(dirname "$0")/.."
AGB="${AGB:-target/release/agb}"
[ -x "$AGB" ] || cargo build --release -p agb-cli
mkdir -p web/songs
make() {
  name="$1"
  shift
  if [ -f "songs/$name.agbsong" ]; then
    "$AGB" song web "songs/$name.agbsong" --out "web/songs/$name.agbw" "$@" >/dev/null
    echo "web/songs/$name.agbw"
  else
    echo "songs/$name.agbsong is not here: passed over"
  fi
}
# This Turn was written for an FM electric piano plug-in (Dexed): here the engine's own FM plays it,
# the same in the web player and on the handheld. (A web instrument can take a track too:
# --instrument voice=agb.epiano; that one is heard in the web player only.)
make this-turn --instrument "voice=preset:synth.fm:keys:tine_piano@v1.0" --lyric-lines songs/this-turn/form.json
make simulated-heart --lyric-lines songs/simulated-heart/form.json
make slow-spin
make neon-strut
make chrome-engine
python3 web/reindex.py
