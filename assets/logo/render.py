#!/usr/bin/env python3
"""Pixel-art logo renderer: text grids -> crisp SVG -> PNG preview sheet.

Usage: python3 -I render.py <out_dir>
Writes <name>.svg per variant, plus preview.svg/.png showing each variant
large on one tile and at icon sizes on several candidate backgrounds.
"""
import os
import subprocess
import sys

PALETTE = {
    "K": "#0e0d14",  # outline
    "B": "#1f1c2b",  # black fur
    "H": "#3a3550",  # fur highlight / inner ear
    "M": "#55506e",  # muzzle (lighter so nose + smile read)
    "W": "#f4f1ea",  # white: forehead stripe, eye shine
    "E": "#9a5b2e",  # brown eye
    "N": "#08070c",  # nose
    "n": "#8a85a6",  # nose glint
    "R": "#6b1a30",  # mouth interior
    "T": "#eb6f92",  # tongue (rose-pine love)
    "t": "#c24d70",  # tongue crease
}

BACKGROUNDS = [
    ("#f6c177", "gold"),
    ("#c4a7e7", "iris"),
    ("#9ccfd8", "foam"),
    ("#faf4ed", "dawn"),
    ("#191724", "base"),
]

VARIANTS = {
    "happy-v5": [
        ".....K.............K.....",
        ".....KK...........KK.....",
        "....KBHK.........KHBK....",
        "....KBHHK.......KHHBK....",
        "...KBBHHBK.....KBHHBBK...",
        "...KBBHHHBK...KBHHHBBK...",
        "...KBBHHHBBKKKBBHHHBBK...",
        "..KBBBHHBBBBWBBBBHHBBBK..",
        "..KBBBBBBBBBWBBBBBBBBBK..",
        ".KBBBBBBBBBBWBBBBBBBBBBK.",
        ".KBBBBBBWEBBBBBWEBBBBBBK.",
        ".KBBBBBBEEBBBBBEEBBBBBBK.",
        ".KBBBBBBBMnNNNNMBBBBBBBK.",
        ".KBBBBBBMMMNNNMMMBBBBBBK.",
        ".KBBBBBKMMMMKMMMMKBBBBBK.",
        "..KBBBBBRRRRRRRRRBBBBBK..",
        "..KBBBBBBRTTTTTRBBBBBBK..",
        "...KBBBBBRTTtTTRBBBBBK...",
        "....KBBBBKTTtTTKBBBBK....",
        ".....KKKKKTTtTTKKKKK.....",
        ".........KTTtTTK.........",
        ".........KTTtTTK.........",
        "..........KTTTK..........",
        "...........KKK...........",
        ".........................",
    ],
    "happy-v4": [
        "....K.............K....",
        "...KBK...........KBK...",
        "...KBHK.........KHBK...",
        "..KBHHK.........KHHBK..",
        "..KBHHBK.......KBHHBK..",
        "..KBHHBBK.....KBBHHBK..",
        "..KBBHBBBKKKKKBBBHBBK..",
        "..KBBBBBBBBWBBBBBBBBK..",
        ".KBBBBBBBBBWBBBBBBBBBK.",
        ".KBBBBBBBBBWBBBBBBBBBK.",
        ".KBBBBBWEBBBBBWEBBBBBK.",
        ".KBBBBBEEBBBBBEEBBBBBK.",
        ".KBBBBBBBMnNNMBBBBBBBK.",
        ".KBBBBBBMMMNMMMBBBBBBK.",
        ".KBBBBKMMMMKMMMMKBBBBK.",
        "..KBBBBRRRRRRRRRBBBBK..",
        "..KBBBBBRTTTTTRBBBBBK..",
        "...KBBBBRTTtTTRBBBBK...",
        "....KKKKKTTtTTKKKKK....",
        "........KTTtTTK........",
        "........KTTtTTK........",
        ".........KTTTK.........",
        "..........KKK..........",
    ],
}


def grid_svg(grid, bg=None, pad=0):
    h = len(grid)
    w = max(len(r) for r in grid)
    for i, row in enumerate(grid):
        assert len(row) == w, f"row {i} has width {len(row)}, want {w}"
        bad = set(row) - set(PALETTE) - {"."}
        assert not bad, f"row {i} has unknown colors {bad}"
    vw, vh = w + 2 * pad, h + 2 * pad
    parts = [
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {vw} {vh}" '
        f'width="{vw}" height="{vh}" shape-rendering="crispEdges">'
    ]
    if bg:
        parts.append(f'<rect width="{vw}" height="{vh}" fill="{bg}"/>')
    for y, row in enumerate(grid):
        x = 0
        while x < w:
            ch = row[x]
            if ch == ".":
                x += 1
                continue
            run = 1
            while x + run < w and row[x + run] == ch:
                run += 1
            parts.append(f'<rect x="{x + pad}" y="{y + pad}" width="{run}" height="1" fill="{PALETTE[ch]}"/>')
            x += run
    parts.append("</svg>")
    return "\n".join(parts)


def nested(grid, x, y, size, bg):
    svg = grid_svg(grid, bg=bg, pad=2)
    vb = svg.split('viewBox="')[1].split('"')[0]
    body = svg.split(">", 1)[1].rsplit("</svg>", 1)[0]
    return (
        f'<svg x="{x}" y="{y}" width="{size}" height="{size}" viewBox="{vb}" '
        f'shape-rendering="crispEdges">{body}</svg>'
    )


def preview_sheet(variants):
    gap, big, sizes = 20, 230, [96, 48]
    out = []
    y = gap
    width = 0
    for name, grid in variants.items():
        out.append(nested(grid, gap, y, big, BACKGROUNDS[0][0]))
        x = gap + big + gap
        for bg, label in BACKGROUNDS:
            out.append(nested(grid, x, y, sizes[0], bg))
            out.append(nested(grid, x + (sizes[0] - sizes[1]) // 2, y + sizes[0] + 12, sizes[1], bg))
            out.append(
                f'<text x="{x}" y="{y + sizes[0] + sizes[1] + 30}" font-family="monospace" '
                f'font-size="12" fill="#908caa">{label}</text>'
            )
            x += sizes[0] + gap
        out.append(
            f'<text x="{gap}" y="{y + big + 18}" font-family="monospace" font-size="14" fill="#908caa">{name}</text>'
        )
        width = max(width, x)
        y += big + gap + 28
    head = (
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{y}" viewBox="0 0 {width} {y}">'
        f'<rect width="{width}" height="{y}" fill="#26233a"/>'
    )
    return head + "\n".join(out) + "</svg>"


def main():
    out = sys.argv[1]
    os.makedirs(out, exist_ok=True)
    for name, grid in VARIANTS.items():
        with open(os.path.join(out, f"{name}.svg"), "w") as f:
            f.write(grid_svg(grid))
    sheet = os.path.join(out, "preview.svg")
    with open(sheet, "w") as f:
        f.write(preview_sheet(VARIANTS))
    subprocess.run(["rsvg-convert", sheet, "-o", os.path.join(out, "preview.png")], check=True)
    print("wrote", os.path.join(out, "preview.png"))


if __name__ == "__main__":
    main()
