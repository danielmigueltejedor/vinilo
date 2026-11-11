#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
# SPDX-License-Identifier: GPL-3.0-or-later
"""Draw the Vinilo installer background: dmg-background.png and dmg-background@2x.png.

    python3 packaging/macos/render_dmg_background.py packaging/macos <Sora[wght].ttf>

The Finder window shows a 660x432 point area with the icons at (160,190) and
(500,190). The picture is 2000x1300 points on purpose: Finder never scales it,
so a window opened wider keeps showing the window colour instead of a white
band. The look is the Nodalia one: a flat island on the window colour, Sora
title, no gradients or glows. Colours are the dark tokens of nodalia-gtk.
create_dmg.sh joins the two PNGs into the multi-resolution TIFF Finder reads.
"""
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

WINDOW = (0x0B, 0x0C, 0x10)  # color.surface.window
PANEL = (0x15, 0x17, 0x1E)  # color.surface.panel
TEXT = (0xEE, 0xF0, 0xF4)  # color.text.primary
TERTIARY = (0x8A, 0x90, 0xA3)  # color.text.tertiary
ACCENT = (0x2C, 0xC5, 0xD3)  # color.accent.text
CANVAS = (2000, 1300)
AREA = (660, 440)  # what the window shows, drawn with room to spare
SUPER = 4  # supersampling for the area that has shapes and text


def font(path: str, size: float, weight: int) -> ImageFont.FreeTypeFont:
    f = ImageFont.truetype(path, round(size))
    try:
        f.set_variation_by_axes([weight])
    except (OSError, AttributeError):
        pass
    return f


def draw_area(scale: int, sora: str) -> Image.Image:
    k = scale * SUPER
    img = Image.new("RGB", (AREA[0] * k, AREA[1] * k), WINDOW)
    d = ImageDraw.Draw(img)

    def pt(x: float) -> int:
        return round(x * k)

    # The island the icons sit on: the same 16 pt radius as every Nodalia panel.
    d.rounded_rectangle([pt(20), pt(20), pt(640), pt(412)], radius=pt(16), fill=PANEL)

    title = font(sora, 34 * k, 800)
    d.text((pt(330), pt(58)), "Vinilo", font=title, fill=TEXT, anchor="mm")
    hint = font(sora, 12.5 * k, 500)
    d.text((pt(330), pt(96)), "Arrastra Vinilo a Aplicaciones  ·  Drag Vinilo to Applications",
           font=hint, fill=TERTIARY, anchor="mm")

    # Finder draws icon names in its own colour: a light plate keeps them legible.
    for x, width in ((160, 86), (500, 128)):
        d.rounded_rectangle([pt(x - width / 2), pt(262), pt(x + width / 2), pt(284)],
                            radius=pt(11), fill=TEXT)

    # The one action here is a drag: the arrow takes the accent.
    w = pt(3)
    y = pt(190)
    d.line([(pt(262), y), (pt(380), y)], fill=ACCENT, width=w)
    d.line([(pt(366), pt(177)), (pt(381), y), (pt(366), pt(203))], fill=ACCENT, width=w, joint="curve")
    for cx, cy in ((262, 190), (366, 177), (366, 203), (381, 190)):
        d.ellipse([pt(cx) - w // 2, pt(cy) - w // 2, pt(cx) + w // 2, pt(cy) + w // 2], fill=ACCENT)
    return img.resize((AREA[0] * scale, AREA[1] * scale), Image.LANCZOS)


def render(scale: int, out: Path, sora: str) -> None:
    canvas = Image.new("RGB", (CANVAS[0] * scale, CANVAS[1] * scale), WINDOW)
    canvas.paste(draw_area(scale, sora), (0, 0))
    # 144 dpi marks the @2x picture so tiffutil -cathidpicheck pairs the two.
    canvas.save(out, dpi=(72 * scale, 72 * scale), optimize=True)


def main() -> None:
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    out, sora = Path(sys.argv[1]), sys.argv[2]
    render(1, out / "dmg-background.png", sora)
    render(2, out / "dmg-background@2x.png", sora)


main()
