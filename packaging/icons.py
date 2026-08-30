#!/usr/bin/env python3
"""Draws the app icon in the colours the editor ships with — VS Code's Dark
Modern — and writes every size the packages need: the PNG ladder and
yara.ico for Windows. Needs Pillow. `make icons` runs it.

The picture is the prompt the editor is about: a blue chevron and the block
cursor beside it, on the ground the panes are painted on."""

import os
import sys

from PIL import Image, ImageDraw, ImageFilter

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "assets", "icon")

# Dark Modern, as crates/yara-core/src/theme.rs has it.
BG_TOP = (0x2A, 0x2A, 0x2B)
BG_BOTTOM = (0x1A, 0x1A, 0x1B)
EDGE = (0x3C, 0x3C, 0x3D)
ACCENT = (0x4F, 0xA6, 0xE0)
ACCENT_DEEP = (0x3F, 0x73, 0x96)
CURSOR = (0xE6, 0xE6, 0xE6)

SIZES = [16, 32, 64, 128, 256, 512, 1024]
# Drawn at 4x and downsampled: rounded corners and stroke ends stay smooth.
SCALE = 4


def rounded_mask(size, radius):
    mask = Image.new("L", (size, size), 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, size - 1, size - 1), radius, fill=255)
    return mask


def draw(size):
    s = size * SCALE
    unit = s / 1024

    # Ground: a vertical gradient inside a squircle-ish square, with a faint
    # edge so the icon still reads on a dark dock.
    gradient = Image.new("RGB", (1, s))
    for y in range(s):
        t = y / (s - 1)
        gradient.putpixel((0, y), tuple(round(a + (b - a) * t) for a, b in zip(BG_TOP, BG_BOTTOM)))
    ground = gradient.resize((s, s))
    radius = int(230 * unit)
    icon = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    icon.paste(ground, (0, 0), rounded_mask(s, radius))
    ImageDraw.Draw(icon).rounded_rectangle(
        (0, 0, s - 1, s - 1), radius, outline=EDGE + (140,), width=int(6 * unit)
    )

    # A soft blue glow behind the prompt, like a phosphor bleed.
    glow = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    ImageDraw.Draw(glow).ellipse(
        (int(220 * unit), int(300 * unit), int(760 * unit), int(760 * unit)),
        fill=ACCENT_DEEP + (90,),
    )
    icon = Image.alpha_composite(icon, glow.filter(ImageFilter.GaussianBlur(int(90 * unit))))

    # The prompt: a chevron with round ends, and the block cursor beside it.
    d = ImageDraw.Draw(icon)
    stroke = int(96 * unit)
    x0, x1 = int(300 * unit), int(520 * unit)
    top, mid, bottom = int(340 * unit), int(512 * unit), int(684 * unit)
    d.line([(x0, top), (x1, mid), (x0, bottom)], fill=ACCENT, width=stroke, joint="curve")
    r = stroke // 2
    for x, y in [(x0, top), (x0, bottom), (x1, mid)]:
        d.ellipse((x - r, y - r, x + r, y + r), fill=ACCENT)
    d.rounded_rectangle(
        (int(600 * unit), top - r, int(720 * unit), bottom + r), int(24 * unit), fill=CURSOR
    )

    return icon.resize((size, size), Image.LANCZOS)


def main():
    os.makedirs(OUT, exist_ok=True)
    images = {size: draw(size) for size in SIZES}
    for size, image in images.items():
        image.save(os.path.join(OUT, f"icon-{size}.png"))
    images[256].save(
        os.path.join(OUT, "yara.ico"),
        sizes=[(16, 16), (32, 32), (64, 64), (128, 128), (256, 256)],
    )
    print(f"wrote {len(images)} icons and yara.ico into {OUT}", file=sys.stderr)


if __name__ == "__main__":
    main()
