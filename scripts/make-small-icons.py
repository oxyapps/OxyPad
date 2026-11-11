#!/usr/bin/env python3
"""Draws the small OxyPad icons and rebuilds oxypad-icons/oxypad.ico.

Scaled down, the artwork in oxypad.svg turns to mush below 48 pixels: its
outline and text lines are thinner than a pixel. The 16, 20, 24 and 32 pixel
icons are therefore drawn here on the pixel grid, with one-pixel outlines and
whole-pixel text lines, as icon sets usually do for their small sizes.

Run it after changing the artwork (needs Pillow):

    python3 scripts/make-small-icons.py

  Output: oxypad-icons/png/oxypad-{16,20,24,32}.png
          oxypad-icons/oxypad.ico (those, plus png/oxypad-{48,64,128,256}.png)
"""

import struct
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFilter

ICONS = Path(__file__).resolve().parent.parent / "oxypad-icons"

INK = (0x14, 0x16, 0x1B)
PAPER = (0xF3, 0xEE, 0xE3)
FOLD = (0xC9, 0xC2, 0xB3)
ACCENT = (0xD9, 0x56, 0x1F)

# Each shape is drawn this many times larger and averaged down, so only
# curves and diagonals are antialiased; edges on the pixel grid stay sharp.
SCALE = 8

# In pixels, right and bottom edges exclusive.
#   page:    the sheet's box, corner radius and the size of the folded corner
#   lines:   text lines as (top, left, right, opacity), `thickness` pixels tall
#   chevron: the prompt's ">", as boxes of whole pixels at the smallest sizes
#            and as three points and a stroke width above them
#   block:   the prompt's cursor
LAYOUTS = {
    16: dict(
        page=(2, 1, 14, 15), radius=1.5, fold=5, thickness=1,
        lines=[(5, 4, 9, 0.3), (7, 4, 12, 0.5), (9, 4, 10, 0.8)],
        chevron=[(4, 11, 6, 12), (5, 12, 7, 13), (4, 13, 6, 14)],
        block=(8, 11, 10, 14),
    ),
    20: dict(
        page=(3, 1, 17, 19), radius=2, fold=6, thickness=1,
        lines=[(6, 5, 11, 0.3), (9, 5, 15, 0.5), (12, 5, 13, 0.8)],
        chevron=[(5, 14, 7, 15), (6, 15, 8, 16), (5, 16, 7, 17)],
        block=(10, 14, 12, 17),
    ),
    24: dict(
        page=(3, 1, 21, 23), radius=2, fold=7, thickness=2,
        lines=[(6, 6, 13, 0.3), (10, 6, 18, 0.5), (14, 6, 15, 0.8)],
        chevron=[(6.8, 17.8), (9, 19.5), (6.8, 21.2)], stroke=1.7,
        block=(12, 17, 15, 22),
    ),
    32: dict(
        page=(4, 1, 28, 31), radius=2.5, fold=9, thickness=2,
        lines=[(11, 8, 18, 0.3), (15, 8, 24, 0.5), (19, 8, 21, 0.8)],
        chevron=[(8.6, 23.8), (11.4, 26), (8.6, 28.2)], stroke=2.2,
        block=(14, 23, 17, 29),
    ),
}
# Larger sizes are the artwork itself.
ARTWORK_SIZES = (48, 64, 128, 256)


def scaled(values):
    return [round(value * SCALE) for value in values]


def draw(size):
    layout = LAYOUTS[size]
    canvas = size * SCALE
    left, top, right, bottom = layout["page"]
    fold = layout["fold"]

    def box(values):
        """`values` on the canvas, for Pillow, whose boxes include their far edges."""
        x0, y0, x1, y1 = scaled(values)
        return (x0, y0, x1 - 1, y1 - 1)

    def mask():
        return Image.new("L", (canvas, canvas), 0)

    # The sheet: a rounded box without its top right corner.
    sheet = mask()
    pen = ImageDraw.Draw(sheet)
    pen.rounded_rectangle(box((left, top, right, bottom)), radius=layout["radius"] * SCALE, fill=255)
    pen.polygon(scaled((right - fold, top - 1, right + 1, top - 1, right + 1, top + fold, right - fold, top)), fill=0)
    pen.polygon(scaled((right - fold, top, right + 1, top, right + 1, top + fold + 1, right, top + fold)), fill=0)
    # One pixel of outline: the sheet, less itself shrunk by a pixel.
    inside = sheet.filter(ImageFilter.MinFilter(2 * SCALE + 1))
    # The folded corner covers the cut; its two straight edges are outlined too.
    corner = mask()
    ImageDraw.Draw(corner).polygon(scaled((right - fold, top, right - fold, top + fold, right, top + fold)), fill=255)
    edges = mask()
    pen = ImageDraw.Draw(edges)
    pen.rectangle(box((right - fold, top, right - fold + 1, top + fold)), fill=255)
    pen.rectangle(box((right - fold, top + fold - 1, right, top + fold)), fill=255)

    image = Image.new("RGBA", (canvas, canvas), (0, 0, 0, 0))
    image.paste(INK + (255,), mask=sheet)
    image.paste(PAPER + (255,), mask=inside)
    image.paste(FOLD + (255,), mask=ImageChops.multiply(corner, inside))
    image.paste(INK + (255,), mask=ImageChops.multiply(edges, corner))

    pen = ImageDraw.Draw(image)
    for line_top, line_left, line_right, opacity in layout["lines"]:
        # Mixed with the paper rather than translucent, so the icon is opaque.
        color = tuple(round(ink * opacity + paper * (1 - opacity)) for ink, paper in zip(INK, PAPER))
        pen.rectangle(box((line_left, line_top, line_right, line_top + layout["thickness"])), fill=color)

    if "stroke" in layout:
        width = round(layout["stroke"] * SCALE)
        points = [tuple(scaled(point)) for point in layout["chevron"]]
        pen.line(points, fill=ACCENT, width=width, joint="curve")
        for x, y in points:
            pen.ellipse((x - width / 2, y - width / 2, x + width / 2, y + width / 2), fill=ACCENT)
    else:
        for pixels in layout["chevron"]:
            pen.rectangle(box(pixels), fill=ACCENT)
    pen.rectangle(box(layout["block"]), fill=ACCENT)

    return image.resize((size, size), Image.BOX)


def write_ico(path, pngs):
    """An .ico whose images are the PNG files `pngs`, by size."""
    header = struct.pack("<HHH", 0, 1, len(pngs))
    offset = len(header) + 16 * len(pngs)
    entries, data = b"", b""
    for size, png in sorted(pngs.items()):
        side = 0 if size >= 256 else size
        entries += struct.pack("<BBBBHHII", side, side, 0, 0, 1, 32, len(png), offset + len(data))
        data += png
    path.write_bytes(header + entries + data)


def main():
    pngs = {}
    for size in LAYOUTS:
        path = ICONS / "png" / f"oxypad-{size}.png"
        draw(size).save(path, optimize=True)
        pngs[size] = path.read_bytes()
        print(f"drew {path.relative_to(ICONS.parent)}")
    for size in ARTWORK_SIZES:
        pngs[size] = (ICONS / "png" / f"oxypad-{size}.png").read_bytes()
    write_ico(ICONS / "oxypad.ico", pngs)
    print(f"wrote oxypad-icons/oxypad.ico ({', '.join(str(size) for size in sorted(pngs))})")


if __name__ == "__main__":
    main()
