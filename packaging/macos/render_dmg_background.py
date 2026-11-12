#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
# SPDX-License-Identifier: GPL-3.0-or-later
"""Render the Vinilo installer background: dmg-background.png and dmg-background@2x.png.

    python3 packaging/macos/render_dmg_background.py packaging/macos

dmg-background.svg is drawn by tools/dmg-background/draw_background.py in nodalia-design, the
same layout every Nodalia app's DMG uses: two islands on the window colour, the record on the
left, the icons on the white one. Its text is outlined, so resvg needs no fonts. The Finder
window shows 660x400 points with the icons at (326,205) and (574,205); the picture is padded to
2000x1300 points with the window colour on purpose, since Finder never scales it and a window
opened wider would otherwise show a white band. create_dmg.sh joins the two PNGs into the
multi-resolution TIFF Finder reads.
"""
import subprocess
import sys
import tempfile
from pathlib import Path

from PIL import Image

WINDOW = (0xE4, 0xE6, 0xEB)  # color.surface.window, light
AREA = (660, 400)
CANVAS = (2000, 1300)


def render(svg: Path, scale: int, out: Path) -> None:
    with tempfile.TemporaryDirectory() as tmp:
        area = Path(tmp) / "area.png"
        subprocess.run(["resvg", "--skip-system-fonts", "-w", str(AREA[0] * scale), str(svg), str(area)],
                       check=True)
        canvas = Image.new("RGB", (CANVAS[0] * scale, CANVAS[1] * scale), WINDOW)
        with Image.open(area) as im:
            canvas.paste(im.convert("RGB"), (0, 0))
    # 144 dpi marks the @2x picture so tiffutil -cathidpicheck pairs the two.
    canvas.save(out, dpi=(72 * scale, 72 * scale), optimize=True)


def main() -> None:
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    out = Path(sys.argv[1])
    svg = out / "dmg-background.svg"
    render(svg, 1, out / "dmg-background.png")
    render(svg, 2, out / "dmg-background@2x.png")


main()
