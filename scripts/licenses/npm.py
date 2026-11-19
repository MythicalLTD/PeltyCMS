#!/usr/bin/env python3
"""Render `pnpm licenses list --prod --json` (stdin) as Markdown with license texts."""
import json
import os
import sys

data = json.load(sys.stdin)
pkgs = []
for group, items in data.items():
    for p in items:
        path = (p.get("paths") or [""])[0]
        lic = group
        try:
            with open(os.path.join(path, "package.json"), encoding="utf-8") as f:
                meta = json.load(f)
            v = meta.get("license")
            if isinstance(v, dict):
                v = v.get("type")
            if isinstance(v, str) and v.strip():
                lic = v.strip()
        except OSError:
            pass
        text = ""
        try:
            for name in sorted(os.listdir(path)):
                if name.lower().startswith(("license", "licence", "copying")):
                    with open(os.path.join(path, name), encoding="utf-8", errors="replace") as f:
                        text = f.read().strip()
                    break
        except OSError:
            pass
        for ver in p.get("versions") or [""]:
            pkgs.append((p["name"], ver, lic, p.get("homepage") or "", text))

pkgs.sort(key=lambda x: (x[0].lower(), x[1]))
for name, ver, lic, home, text in pkgs:
    print(f"### `{name} {ver}` ({lic})")
    print()
    if home:
        print(home)
        print()
    if text:
        print("```text")
        print(text.replace("```", "'''"))
        print("```")
    else:
        print(f"License: {lic} (no license file in the package).")
    print()
