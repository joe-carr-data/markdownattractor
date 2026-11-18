#!/usr/bin/env python3
"""Render the caption strips and the end card of demo video 3 as PNG files (needs Pillow).

    captions.py OUT_DIR BACKDROP.png CAPTION [CAPTION ...]

Writes OUT_DIR/cap0.png … (transparent, the size of the backdrop, one pill of text at the bottom)
OUT_DIR/tech.png (the pipeline and the three ranked lists), OUT_DIR/bench.png (the measured figures) and OUT_DIR/end.png (the backdrop,
dimmed, with the name, three lines and the install commands).
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
    draw.text((w / 2, 436), "A Rust daemon watches the folder and indexes every save within seconds.",
              font=line, fill=INK, anchor="mm")
    draw.text((w / 2, 492), "You never run a reindex.", font=line, fill=INK, anchor="mm")
    pill(draw, w / 2, 620, "/plugin marketplace add joe-carr-data/markdownattractor", mono,
         fill=(17, 17, 27, 240), ink=(205, 214, 244, 255))
    pill(draw, w / 2, 690, "/plugin install markdownattractor@markdownattractor", mono,
         fill=(17, 17, 27, 240), ink=(205, 214, 244, 255))
    card.convert("RGB").save(out / "end.png")
    how_it_works(backdrop).save(out / "tech.png")
    measured(backdrop).save(out / "bench.png")


SUB = (186, 194, 222, 255)
GREEN, MAUVE, BLUE, PEACH = (166, 227, 161, 255), (203, 166, 247, 255), (137, 180, 250, 255), (250, 179, 135, 255)


def box(draw, rect, title, lines, accent, head, body):
    """A dark panel with a coloured top edge, a bold title and a few lines under it."""
    x0, y0, x1, _ = rect
    draw.rounded_rectangle(rect, radius=16, fill=(17, 17, 27, 235))
    draw.rounded_rectangle((x0, y0, x1, y0 + 8), radius=4, fill=accent)
    draw.text((x0 + 22, y0 + 24), title, font=head, fill=accent)
    for n, text in enumerate(lines):
        draw.text((x0 + 22, y0 + 68 + n * 32), text, font=body, fill=SUB if n else INK)


def arrow(draw, start, end, width=5):
    """A straight arrow, to the right or downwards."""
    (x0, y0), (x1, y1) = start, end
    draw.line((x0, y0, x1, y1), fill=INK, width=width)
    if y0 == y1:
        draw.polygon([(x1 + 2, y1), (x1 - 14, y1 - 10), (x1 - 14, y1 + 10)], fill=INK)
    else:
        draw.polygon([(x1, y1 + 2), (x1 - 10, y1 - 14), (x1 + 10, y1 - 14)], fill=INK)


def how_it_works(backdrop):
    """The technical card: the pipeline on one row, the three ranked lists and their fusion below."""
    w, h = backdrop.size
    card = Image.alpha_composite(backdrop, Image.new("RGBA", (w, h), (17, 17, 27, 165)))
    draw = ImageDraw.Draw(card)
    title, head, body, small = font(SANS, 50, index=1), font(SANS, 28, index=1), font(SANS, 23), font(SANS, 24, index=1)
    draw.text((60, 58), "How it works", font=title, fill=INK, anchor="lm")
    draw.text((w - 60, 62), "one Rust binary: watcher, index, MCP server, CLI", font=body, fill=SUB, anchor="rm")

    top, bottom = 110, 290
    box(draw, (60, top, 370, bottom), "Your markdown", ["any folder of .md files", "never modified"], PEACH, head, body)
    box(draw, (440, top, 800, bottom), "Rust daemon",
        ["watches every save", "splits files into sections", "skips unchanged sections"], PEACH, head, body)
    box(draw, (870, top, 1190, bottom), "SQLite index",
        ["one local file", "each section: lines + dates"], PEACH, head, body)
    box(draw, (1260, top, 1540, bottom), "Claude Code",
        ["over MCP", "search, card, open"], PEACH, head, body)
    for x0, x1 in ((370, 440), (800, 870), (1190, 1260)):
        arrow(draw, (x0 + 8, 200), (x1 - 8, 200))

    draw.text((60, 338), "One query, three ranked lists", font=small, fill=INK, anchor="lm")
    top, bottom = 365, 570
    box(draw, (60, top, 530, bottom), "Raw text · BM25",
        ["SQLite FTS5, built at parse time", "searchable 1.7 s after a save (p50)", "works with no API key"],
        GREEN, head, body)
    box(draw, (565, top, 1035, bottom), "Cards · BM25",
        ["a ~200-token summary per section", "written by a small model, in ~8 s", "dates and names need evidence"],
        MAUVE, head, body)
    box(draw, (1070, top, 1540, bottom), "Card vectors",
        ["embeddings computed locally", "bge-small, on your machine", "matches meaning, not only words"],
        BLUE, head, body)
    for x in (295, 800, 1305):
        arrow(draw, (x, bottom + 6), (x, 612))

    draw.rounded_rectangle((60, 620, 1540, 720), radius=16, fill=(17, 17, 27, 235))
    draw.text((w / 2, 652), "Reciprocal rank fusion, with an optional time filter", font=head, fill=INK, anchor="mm")
    draw.text((w / 2, 692), "top 5 hits, each with file · heading path · line range · last changed",
              font=body, fill=SUB, anchor="mm")
    arrow(draw, (w / 2, 726), (w / 2, 762))
    pill(draw, w / 2, 800, "Claude reads the card, then opens only the lines it needs, and cites them",
         small, fill=(17, 17, 27, 240))
    return card.convert("RGB")



def tile(draw, rect, figure, label, lines, accent, big, head, body):
    """A panel with one large figure, what it measures, and its limits in smaller type."""
    x0, y0, x1, _ = rect
    draw.rounded_rectangle(rect, radius=16, fill=(17, 17, 27, 235))
    draw.rounded_rectangle((x0, y0, x1, y0 + 8), radius=4, fill=accent)
    draw.text((x0 + 24, y0 + 78), figure, font=big, fill=accent, anchor="lm")
    draw.text((x0 + 24, y0 + 150), label, font=head, fill=INK, anchor="lm")
    for n, text in enumerate(lines):
        draw.text((x0 + 24, y0 + 190 + n * 30), text, font=body, fill=SUB, anchor="lm")


def measured(backdrop):
    """The benchmark card. Every figure is on docs/benchmarks.md or in the launch drafts' table of
    what passed; change one there and change it here."""
    w, h = backdrop.size
    card = Image.alpha_composite(backdrop, Image.new("RGBA", (w, h), (17, 17, 27, 165)))
    draw = ImageDraw.Draw(card)
    title, big, head, body = font(SANS, 50, index=1), font(SANS, 76, index=1), font(SANS, 25, index=1), font(SANS, 22)
    draw.text((60, 58), "Measured", font=title, fill=INK, anchor="lm")
    draw.text((w - 60, 62), "gates named before the results; only what passed is claimed",
              font=font(SANS, 23), fill=SUB, anchor="rm")

    top, bottom = 110, 400
    tile(draw, (60, top, 412, bottom), "8 s", "install to first search hit",
         ["no API key, nothing installed", "7 to 21 s, one machine"], PEACH, big, head, body)
    tile(draw, (436, top, 788, bottom), "1.7 s", "save to searchable",
         ["median of 20 edits", "summary card ready in 8.4 s"], GREEN, big, head, body)
    tile(draw, (812, top, 1164, bottom), "98 %", "right section in the top 5",
         ["60 golden questions, full index", "88 % with no model at all"], BLUE, big, head, body)
    tile(draw, (1188, top, 1540, bottom), "100 %", "answers fully grounded",
         ["every claim cites opened lines", "golden set; 87.5 % before"], MAUVE, big, head, body)

    top, bottom = 424, 584
    for x0, x1, name, first, second in (
            (60, 788, "Against grep", "Better on 4 of 16 questions, worse on none",
             "same questions, 3 runs each, graded on 0-6"),
            (812, 1540, "Against a knowledge-graph tool (graphify)", "3.18x fewer source tokens read",
             "at answer-quality parity")):
        draw.rounded_rectangle((x0, top, x1, bottom), radius=16, fill=(17, 17, 27, 235))
        draw.text((x0 + 24, top + 36), name, font=head, fill=INK, anchor="lm")
        draw.text((x0 + 24, top + 82), first, font=font(SANS, 30, index=1), fill=GREEN, anchor="lm")
        draw.text((x0 + 24, top + 124), second + " · golden corpus", font=body, fill=SUB, anchor="lm")

    top, bottom, mid = 608, 768, font(SANS, 46, index=1)
    for x0, x1, figure, label, note, accent in (
            (60, 537, "60 ms", "per hybrid search", "mean over the golden questions", PEACH),
            (561, 1039, "96 %+", "of extracted dates and names", "judged supported by two model auditors", MAUVE),
            (1063, 1540, "$0.31", "to index a 32-file docs folder", "222K tokens on Haiku; edits redo one section", GREEN)):
        draw.rounded_rectangle((x0, top, x1, bottom), radius=16, fill=(17, 17, 27, 235))
        draw.text((x0 + 24, top + 50), figure, font=mid, fill=accent, anchor="lm")
        draw.text((x0 + 24, top + 100), label, font=head, fill=INK, anchor="lm")
        draw.text((x0 + 24, top + 134), note, font=body, fill=SUB, anchor="lm")
    pill(draw, w / 2, 826, "method and every number: docs/benchmarks.md", font(SANS, 24, index=1),
         fill=(17, 17, 27, 240))
    return card.convert("RGB")


if __name__ == "__main__":
    main()
