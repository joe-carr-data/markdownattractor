#!/usr/bin/env python3
"""Render the caption strips and the end card of demo video 3 as PNG files (needs Pillow).

    captions.py OUT_DIR BACKDROP.png CAPTION [CAPTION ...]

Writes OUT_DIR/cap0.png … (transparent, the size of the backdrop, one pill of text at the bottom)
and OUT_DIR/end.png (the backdrop, dimmed, with the name, two lines and the install commands).
scripts/demo/record-live.sh overlays them with ffmpeg, whose Homebrew build has no text filter.
"""
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

SANS = ["/System/Library/Fonts/HelveticaNeue.ttc", "/System/Library/Fonts/Helvetica.ttc",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"]
MONO = ["/System/Library/Fonts/SFNSMono.ttf", "/System/Library/Fonts/Menlo.ttc",
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"]
INK = (255, 255, 255, 255)
PANEL = (17, 17, 27, 225)


def font(paths, size, index=0):
    for p in paths:
        if Path(p).exists():
            try:
                return ImageFont.truetype(p, size, index=index)
            except OSError:
                return ImageFont.truetype(p, size)
    return ImageFont.load_default(size)


def pill(draw, cx, cy, text, face, pad_x=26, pad_y=12, fill=PANEL, ink=INK):
    left, top, right, bottom = draw.textbbox((0, 0), text, font=face)
    w, h = right - left, bottom - top
    box = (cx - w / 2 - pad_x, cy - h / 2 - pad_y, cx + w / 2 + pad_x, cy + h / 2 + pad_y)
    draw.rounded_rectangle(box, radius=(h + 2 * pad_y) / 2, fill=fill)
    draw.text((cx - w / 2 - left, cy - h / 2 - top), text, font=face, fill=ink)


def main():
    out, backdrop, captions = Path(sys.argv[1]), Image.open(sys.argv[2]).convert("RGBA"), sys.argv[3:]
    w, h = backdrop.size
    bold = font(SANS, 30, index=1)
    for n, text in enumerate(captions):
        strip = Image.new("RGBA", (w, h), (0, 0, 0, 0))
        pill(ImageDraw.Draw(strip), w / 2, h - 33, text, bold)
        strip.save(out / f"cap{n}.png")

    card = Image.alpha_composite(backdrop, Image.new("RGBA", (w, h), (17, 17, 27, 150)))
    draw = ImageDraw.Draw(card)
    title, line, mono = font(SANS, 92, index=1), font(SANS, 40), font(MONO, 30)
    draw.text((w / 2, 250), "markdownattractor", font=title, fill=INK, anchor="mm")
    draw.text((w / 2, 380), "Claude answers from your markdown, with file, lines and date.",
              font=line, fill=INK, anchor="mm")
    draw.text((w / 2, 436), "An edit is in the answer seconds after the save.",
              font=line, fill=INK, anchor="mm")
    pill(draw, w / 2, 590, "/plugin marketplace add joe-carr-data/markdownattractor", mono,
         fill=(17, 17, 27, 240), ink=(205, 214, 244, 255))
    pill(draw, w / 2, 660, "/plugin install markdownattractor@markdownattractor", mono,
         fill=(17, 17, 27, 240), ink=(205, 214, 244, 255))
    card.convert("RGB").save(out / "end.png")


if __name__ == "__main__":
    main()
