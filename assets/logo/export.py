#!/usr/bin/env python3
"""Export the final shepsky logo (happy-v5) to every asset the app needs.

Usage: python3 -I export.py <render.py> <dist_dir>   (needs rsvg-convert)

The outputs live in the apps: android/app/src/main/res/drawable/
ic_launcher_{foreground,monochrome}.xml and ic_stat_shepsky.xml,
docs/images/icon.{png,svg} (icon-248.png, logo-gold.svg), and the iOS app
icon (AppIcon-1024.png).
"""
import importlib.util
import os
import subprocess
import sys

spec = importlib.util.spec_from_file_location("render", sys.argv[1])
render = importlib.util.module_from_spec(spec)
spec.loader.exec_module(render)

GRID = render.VARIANTS["happy-v5"]
PALETTE = render.PALETTE
GOLD = "#f6c177"
N = len(GRID)  # 25x25
assert all(len(r) == N for r in GRID)

# Pixels that become HOLES in single-color versions (notification / themed
# icon), so the forehead stripe, eyes and open mouth still read.
MONO_HOLES = set("WEt")  # stripe, eyes, tongue crease (mouth stays solid: holes there read as fangs)


def runs(grid, keep):
    """Yield (x, y, w, ch) horizontal runs of pixels whose char passes keep()."""
    for y, row in enumerate(grid):
        x = 0
        while x < len(row):
            ch = row[x]
            if not keep(ch):
                x += 1
                continue
            w = 1
            while x + w < len(row) and row[x + w] == ch:
                w += 1
            yield x, y, w, ch
            x += w


def svg(grid, bg=None, pad=0, mono=None):
    v = N + 2 * pad
    parts = [
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {v} {v}" width="{v}" height="{v}" '
        f'shape-rendering="crispEdges">'
    ]
    if bg:
        parts.append(f'<rect width="{v}" height="{v}" fill="{bg}"/>')
    if mono:
        keep = lambda c: c != "." and c not in MONO_HOLES  # noqa: E731
        for x, y, w, _ in runs(grid, keep):
            parts.append(f'<rect x="{x + pad}" y="{y + pad}" width="{w}" height="1" fill="{mono}"/>')
    else:
        for x, y, w, ch in runs(grid, lambda c: c != "."):
            parts.append(f'<rect x="{x + pad}" y="{y + pad}" width="{w}" height="1" fill="{PALETTE[ch]}"/>')
    parts.append("</svg>")
    return "\n".join(parts)


def vector_paths(grid, mono=False):
    """Android vector-drawable <path>s: one path per color (or one, mono)."""
    by_color = {}
    keep = (lambda c: c != "." and c not in MONO_HOLES) if mono else (lambda c: c != ".")
    for x, y, w, ch in runs(grid, keep):
        color = "#FFFFFFFF" if mono else "#FF" + PALETTE[ch][1:].upper()
        by_color.setdefault(color, []).append(f"M{x},{y}h{w}v1h-{w}z")
    return [f'<path android:fillColor="{c}" android:pathData="{"".join(d)}"/>' for c, d in by_color.items()]


def vector_drawable(grid, size_dp, scale, mono=False, comment=""):
    # Center the 25x25 grid in a size_dp x size_dp viewport at `scale` dp/px.
    offset = (size_dp - N * scale) / 2
    paths = "\n        ".join(vector_paths(grid, mono))
    return f"""<?xml version="1.0" encoding="utf-8"?>
<!-- {comment} Generated from the shepsky pixel-art logo; edit the grid, not this file. -->
<vector xmlns:android="http://schemas.android.com/apk/res/android"
    android:width="{size_dp}dp"
    android:height="{size_dp}dp"
    android:viewportWidth="{size_dp}"
    android:viewportHeight="{size_dp}">
    <group
        android:translateX="{offset:g}"
        android:translateY="{offset:g}"
        android:scaleX="{scale:g}"
        android:scaleY="{scale:g}">
        {paths}
    </group>
</vector>
"""


def png(svg_path, out_path, size):
    subprocess.run(["rsvg-convert", "-w", str(size), "-h", str(size), svg_path, "-o", out_path], check=True)


def main():
    dist = sys.argv[2]
    os.makedirs(dist, exist_ok=True)
    w = lambda name, text: open(os.path.join(dist, name), "w").write(text)  # noqa: E731

    # Masters
    w("logo.svg", svg(GRID))
    w("logo-gold.svg", svg(GRID, bg=GOLD, pad=3))
    w("logo-mono.svg", svg(GRID, mono="#ffffff"))

    # Android adaptive icon. Safe zone = 66dp circle in a 108dp canvas; the
    # farthest pixels (ear tips) sit ~14.6px from center, so 2.2dp/px keeps
    # them at ~32dp < 33dp.
    w("ic_launcher_foreground.xml", vector_drawable(GRID, 108, 2.2, comment="Adaptive icon foreground."))
    w("ic_launcher_monochrome.xml", vector_drawable(GRID, 108, 2.2, mono=True, comment="Themed (monochrome) icon."))
    w("ic_stat_shepsky.xml", vector_drawable(GRID, 24, 0.92, mono=True, comment="Notification small icon."))

    # iOS: single 1024 app icon, no alpha. A 32-unit canvas makes every pixel
    # exactly 32px; offsetting the 25-unit dog by 3.5 keeps edges on 16px
    # boundaries, so the upscale stays crisp.
    dog = svg(GRID).split(">", 1)[1].rsplit("</svg>", 1)[0]
    w("ios-icon.svg",
      '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32" width="32" height="32" shape-rendering="crispEdges">'
      f'<rect width="32" height="32" fill="{GOLD}"/><g transform="translate(3.5 3.5)">{dog}</g></svg>')
    png(os.path.join(dist, "ios-icon.svg"), os.path.join(dist, "AppIcon-1024.png"), 1024)

    # README header + general PNGs (integer scales of the 31px padded tile)
    png(os.path.join(dist, "logo-gold.svg"), os.path.join(dist, "icon-248.png"), 248)
    png(os.path.join(dist, "logo.svg"), os.path.join(dist, "logo-500.png"), 500)
    print("assets in", dist)
    for f in sorted(os.listdir(dist)):
        print(f"  {f:28} {os.path.getsize(os.path.join(dist, f)):>8} bytes")


if __name__ == "__main__":
    main()
