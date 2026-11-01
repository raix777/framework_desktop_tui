#!/usr/bin/env python3
"""Render the captured TUI demo frames (from `CAPTURE_DEMO_DIR`) to PNG images.

Each frame file (`frame_NNNN.txt`) holds a `cols rows` header followed by one
line per row; each cell is `<symbol>\\x1e<fg r,g,b>\\x1e<bg r,g,b>` with cells
separated by `\\x1f`. Every cell is drawn on a monospace grid: the cell's
background color as a filled block, then the cell's character in the foreground
color. The output is a set of truecolor PNGs; assemble an animated GIF from
them with ffmpeg (see the README).
"""

import argparse
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

FIELD = "\x1e"
CELL = "\x1f"
DEFAULT_FONT = "/usr/share/fonts/TTF/MesloLGSNerdFontMono-Regular.ttf"


def parse_frame(text: str):
    """Parse a frame file into (cols, rows, cells); each cell is
    (symbol, fg_rgb, bg_rgb)."""
    lines = text.split("\n")
    cols, rows = map(int, lines[0].split())
    cells = []
    for line in lines[1 : 1 + rows]:
        row = line.split(CELL)
        if row and row[-1] == "":
            row = row[:-1]
        for field in row:
            symbol, fg, bg = field.split(FIELD)
            fr, fgv, fb = (int(v) for v in fg.split(","))
            br, bgv, bb = (int(v) for v in bg.split(","))
            cells.append((symbol, (fr, fgv, fb), (br, bgv, bb)))
    if len(cells) != cols * rows:
        raise SystemExit(f"expected {cols * rows} cells, got {len(cells)}")
    return cols, rows, cells


def render(cols, rows, cells, cell_w, cell_h, font) -> Image.Image:
    """Draw one frame: a black canvas, then each cell's background block and
    character."""
    img = Image.new("RGB", (cols * cell_w, rows * cell_h), (0, 0, 0))
    draw = ImageDraw.Draw(img)
    for y in range(rows):
        for x in range(cols):
            symbol, fg, bg = cells[y * cols + x]
            x0, y0 = x * cell_w, y * cell_h
            if bg != (0, 0, 0):
                draw.rectangle([x0, y0, x0 + cell_w - 1, y0 + cell_h - 1], fill=bg)
            if symbol.strip():
                draw.text(
                    (x0 + cell_w / 2, y0 + cell_h / 2),
                    symbol,
                    font=font,
                    fill=fg,
                    anchor="mm",
                )
    return img


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("frames_dir", type=Path, help="dir with frame_NNNN.txt files")
    ap.add_argument("--out", type=Path, required=True, help="output dir for PNGs")
    ap.add_argument("--cell-w", type=int, default=10, help="pixel width of a cell")
    ap.add_argument("--cell-h", type=int, default=20, help="pixel height of a cell")
    ap.add_argument("--font-size", type=int, default=15)
    ap.add_argument("--font", default=DEFAULT_FONT)
    args = ap.parse_args()

    font = ImageFont.truetype(args.font, args.font_size)
    frame_files = sorted(args.frames_dir.glob("frame_*.txt"))
    if not frame_files:
        sys.exit(f"no frame files in {args.frames_dir}")

    args.out.mkdir(parents=True, exist_ok=True)
    for i, path in enumerate(frame_files):
        cols, rows, cells = parse_frame(path.read_text(encoding="utf-8"))
        img = render(cols, rows, cells, args.cell_w, args.cell_h, font)
        img.save(args.out / f"frame_{i:04d}.png")
    print(f"rendered {len(frame_files)} frames to {args.out}")


if __name__ == "__main__":
    main()
