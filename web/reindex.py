#!/usr/bin/env python3
"""Write songs/index.json: the list the web player shows, from the .agbw files in songs/."""
import json
import pathlib
import zipfile

here = pathlib.Path(__file__).parent / "songs"
out = []
for path in sorted(here.glob("*.agbw")):
    m = json.loads(zipfile.ZipFile(path).read("web.json"))
    artist = "、".join(a.get("name", "") for a in m.get("artists", []))
    out.append({"file": path.name, "title": m.get("title", path.stem), "artist": artist, "seconds": m.get("seconds")})
(here / "index.json").write_text(json.dumps(out, ensure_ascii=False, indent=2) + "\n")
print(f"{len(out)} songs → {here / 'index.json'}")
