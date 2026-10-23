#!/usr/bin/env python3
"""Export the final shepsky logo: the black dog with Dracula-green eyes
(with pupils) on Alucard #FFFBEB. One grid, all assets, plus a QA sheet.

Usage: python3 -I export_v6.py <grid.txt> <dist_dir>
"""
import os
import subprocess
import sys

GRID = [l.rstrip("\n") for l in open(sys.argv[1]) if l.strip()]
N = len(GRID)
assert N == 25 and all(len(r) == N for r in GRID), "grid must be 25x25"

BG = "#FFFBEB"  # Alucard background
PALETTE = {
    "K": "#0e0d14",  # outline
    "B": "#1f1c2b",  # black fur
    "H": "#3a3550",  # ear shading
    "M": "#55506e",  # muzzle
    "W": "#f4f1ea",  # forehead stripe + eye shine
    "E": "#50FA7B",  # Dracula green iris
    "P": "#050508",  # pupil
    "N": "#08070c",  # nose
    "n": "#8a85a6",  # nose glint
    "R": "#6b1a30",  # mouth
    "T": "#eb6f92",  # tongue
    "t": "#c24d70",  # tongue crease
}
# Holes in single-color versions: stripe + whole eyes + tongue crease.
MONO_HOLES = set("WEPt")
assert set("".join(GRID)) - {"."} <= set(PALETTE)


def runs(keep):
    for y, row in enumerate(GRID):
        x = 0
        while x < N:
            ch = row[x]
            if not keep(ch):
                x += 1
                continue
            w = 1
            while x + w < N and row[x + w] == ch:
                w += 1
            yield x, y, w, ch
            x += w


def svg(bg=None, pad=0, mono=None, size=None):
    v = N + 2 * pad
    wh = f'width="{size or v}" height="{size or v}"'
    out = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {v} {v}" {wh} shape-rendering="crispEdges">']
    if bg:
        out.append(f'<rect width="{v}" height="{v}" fill="{bg}"/>')
    if mono:
        for x, y, w, _ in runs(lambda c: c != "." and c not in MONO_HOLES):
            out.append(f'<rect x="{x + pad}" y="{y + pad}" width="{w}" height="1" fill="{mono}"/>')
    else:
        for x, y, w, ch in runs(lambda c: c != "."):
            out.append(f'<rect x="{x + pad}" y="{y + pad}" width="{w}" height="1" fill="{PALETTE[ch]}"/>')
    out.append("</svg>")
    return "\n".join(out)


def vector(size_dp, scale, mono=False, note=""):
    by = {}
    keep = (lambda c: c != "." and c not in MONO_HOLES) if mono else (lambda c: c != ".")
    for x, y, w, ch in runs(keep):
        color = "#FFFFFFFF" if mono else "#FF" + PALETTE[ch][1:].upper()
        by.setdefault(color, []).append(f"M{x},{y}h{w}v1h-{w}z")
    paths = "\n        ".join(f'<path android:fillColor="{c}" android:pathData="{"".join(d)}"/>' for c, d in by.items())
    off = (size_dp - N * scale) / 2
    return f"""<?xml version="1.0" encoding="utf-8"?>
<!-- {note} Generated from the shepsky pixel-art logo (assets/logo); edit the grid, not this file. -->
<vector xmlns:android="http://schemas.android.com/apk/res/android"
    android:width="{size_dp}dp"
    android:height="{size_dp}dp"
    android:viewportWidth="{size_dp}"
    android:viewportHeight="{size_dp}">
    <group
        android:translateX="{off:g}"
        android:translateY="{off:g}"
        android:scaleX="{scale:g}"
        android:scaleY="{scale:g}">
        {paths}
    </group>
</vector>
"""


def png(src, dst, px):
    subprocess.run(["rsvg-convert", "-w", str(px), "-h", str(px), src, "-o", dst], check=True)


def main():
    d = sys.argv[2]
    os.makedirs(d, exist_ok=True)
    w = lambda n, t: open(os.path.join(d, n), "w").write(t)  # noqa: E731
    w("logo.svg", svg())
    w("logo-alucard.svg", svg(bg=BG, pad=3))
    w("logo-mono.svg", svg(mono="#ffffff"))
    w("ic_launcher_foreground.xml", vector(108, 2.2, note="Adaptive icon foreground."))
    w("ic_launcher_monochrome.xml", vector(108, 2.2, mono=True, note="Themed (monochrome) icon."))
    w("ic_stat_shepsky.xml", vector(24, 0.92, mono=True, note="Notification small icon."))
    w("ic_launcher_background_color.txt", BG.upper() + "\n")
    png(os.path.join(d, "logo-alucard.svg"), os.path.join(d, "icon-248.png"), 248)
    png(os.path.join(d, "logo.svg"), os.path.join(d, "logo-500.png"), 500)

    # QA sheet: adaptive canvas + safe zone, circle and squircle masks,
    # themed icon, notification icon at real sizes.
    inner = lambda s: s.split(">", 1)[1].rsplit("</svg>", 1)[0]  # noqa: E731
    dog, mono = inner(svg()), inner(svg(mono="#ffffff"))
    fg = f'<g transform="translate(26.5 26.5) scale(2.2)" shape-rendering="crispEdges">{dog}</g>'

    def panel(x, title, content):
        return (f'<g transform="translate({x} 20)"><svg width="200" height="200" viewBox="0 0 108 108">{content}</svg>'
                f'<text x="0" y="218" font-family="monospace" font-size="12" fill="#c9c9d6">{title}</text></g>')

    sheet = [
        panel(20, "canvas + 66dp safe zone",
              f'<rect width="108" height="108" fill="{BG}"/>{fg}'
              '<circle cx="54" cy="54" r="33" fill="none" stroke="#ff5555" stroke-width="0.6" stroke-dasharray="2 2"/>'),
        panel(240, "circle mask",
              f'<defs><clipPath id="c"><circle cx="54" cy="54" r="36"/></clipPath></defs>'
              f'<g clip-path="url(#c)"><rect width="108" height="108" fill="{BG}"/>{fg}</g>'),
        panel(460, "squircle mask",
              f'<defs><clipPath id="s"><rect x="18" y="18" width="72" height="72" rx="24"/></clipPath></defs>'
              f'<g clip-path="url(#s)"><rect width="108" height="108" fill="{BG}"/>{fg}</g>'),
        panel(680, "themed icon",
              f'<defs><clipPath id="t"><circle cx="54" cy="54" r="36"/></clipPath></defs>'
              f'<g clip-path="url(#t)"><rect width="108" height="108" fill="#3e4f3a"/>'
              f'<g transform="translate(26.5 26.5) scale(2.2)" shape-rendering="crispEdges">'
              + mono.replace("#ffffff", "#c9e7b8") + "</g></g>"),
    ]
    notif = ('<g transform="translate(20 260)"><rect width="300" height="80" fill="#21222c"/>'
             + "".join(f'<svg x="{20 + i * 80}" y="{(80 - s) // 2}" width="{s}" height="{s}" viewBox="0 0 25 25" '
                       f'shape-rendering="crispEdges">{mono}</svg>' for i, s in enumerate((24, 36, 48)))
             + '<text x="0" y="98" font-family="monospace" font-size="12" fill="#c9c9d6">notification icon @ 24/36/48px</text></g>')
    W, H = 900, 380
    page = (f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" viewBox="0 0 {W} {H}">'
            f'<rect width="{W}" height="{H}" fill="#3a3c4e"/>' + "".join(sheet) + notif + "</svg>")
    qa = os.path.join(d, "..", "qa-v6.svg")
    open(qa, "w").write(page)
    subprocess.run(["rsvg-convert", qa, "-o", os.path.join(d, "..", "qa-v6.png")], check=True)
    for f in sorted(os.listdir(d)):
        print(f"  {f:34} {os.path.getsize(os.path.join(d, f)):>7} bytes")


if __name__ == "__main__":
    main()
