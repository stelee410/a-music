#!/bin/sh
# Serve the web player on this machine: http://127.0.0.1:8377/
cd "$(dirname "$0")"
[ -f agb.wasm ] || ./build.sh
exec python3 -m http.server "${PORT:-8377}" --bind 127.0.0.1
