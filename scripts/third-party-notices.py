"""List every Rust crate compiled into the app with its licence, for the app's Licenses folder.

    python3 scripts/third-party-notices.py > THIRD-PARTY.txt

This is a list of components and licences, not the full licence texts; `cargo about` can
generate the full texts and is the next step before a 1.0 release (see docs/DISTRIBUTION.md).
"""

import json
import subprocess
import sys
from pathlib import Path

root = Path(__file__).resolve().parent.parent
meta = json.loads(
    subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked", "--filter-platform", "aarch64-apple-darwin"],
        cwd=root, capture_output=True, text=True, check=True,
    ).stdout
)

# Only crates actually reachable from the app (normal dependencies), not build tools for other OSes.
nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
packages = {p["id"]: p for p in meta["packages"]}
root_id = meta["resolve"]["root"]
seen, stack = set(), [root_id]
while stack:
    pid = stack.pop()
    if pid in seen:
        continue
    seen.add(pid)
    for dep in nodes[pid]["deps"]:
        if any(k["kind"] in (None, "build") for k in dep["dep_kinds"]):
            stack.append(dep["pkg"])

rows = sorted((packages[p]["name"], packages[p]["version"], packages[p].get("license") or "see crate") for p in seen if p != root_id)
out = sys.stdout
out.write("Third-party software in this app\n================================\n\n")
out.write("FFmpeg: LGPL v2.1 or later, with LAME (MP3 encoder, LGPL). Licence texts and build details in Licenses/FFmpeg.\n")
out.write("SQLite (via libsqlite3-sys): public domain.\n\n")
out.write("Rust crates (name, version, licence):\n\n")
width = max(len(n) for n, _, _ in rows)
for name, version, lic in rows:
    out.write(f"  {name.ljust(width)}  {version.ljust(10)}  {lic}\n")
