#!/usr/bin/env python3
"""Generate MoeDict app icons (PNG / ICO / ICNS) without external tooling.

Requires Pillow.  Run from the repository root:
    python3 scripts/gen_icons.py
"""
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "src-tauri" / "icons"
FONT = "/usr/share/fonts/truetype/dejavu/DejaVuSerif-Bold.ttf"

S = 2048
INK_TOP = (31, 42, 74)
INK_BOTTOM = (11, 15, 27)
PAGE = (247, 242, 231)
PAGE_EDGE = (231, 223, 206)
GOLD = (216, 164, 74)
CORAL = (224, 96, 75)
TEXT_INK = (34, 45, 76)


def gradient(size: int) -> Image.Image:
    img = Image.new("RGBA", (size, size))
    draw = ImageDraw.Draw(img)
    for y in range(size):
        t = y / max(size - 1, 1)
        eased = t * t * (3 - 2 * t)
        color = tuple(
            int(INK_TOP[i] + (INK_BOTTOM[i] - INK_TOP[i]) * eased) for i in range(3)
        )
        draw.line([(0, y), (size, y)], fill=color + (255,))
    return img


def rounded_mask(size: int, margin: int, radius: int) -> Image.Image:
    mask = Image.new("L", (size, size), 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        [margin, margin, size - margin, size - margin], radius=radius, fill=255
    )
    return mask


def build() -> Image.Image:
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    margin = int(S * 0.055)
    radius = int(S * 0.225)

    bg = gradient(S)
    mask = rounded_mask(S, margin, radius)
    img.paste(bg, (0, 0), mask)

    # A soft sheen across the upper-left corner.
    sheen = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    sd = ImageDraw.Draw(sheen)
    sd.polygon(
        [(margin, margin), (S * 0.62, margin), (margin, S * 0.62)],
        fill=(255, 255, 255, 16),
    )
    sheen = sheen.filter(ImageFilter.GaussianBlur(S * 0.02))
    img.alpha_composite(Image.composite(sheen, Image.new("RGBA", (S, S)), mask))

    # Book shadow.
    shadow = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    ImageDraw.Draw(shadow).rounded_rectangle(
        [S * 0.235, S * 0.175, S * 0.785, S * 0.855],
        radius=S * 0.035,
        fill=(0, 0, 0, 120),
    )
    shadow = shadow.filter(ImageFilter.GaussianBlur(S * 0.025))
    img.alpha_composite(shadow)

    d = ImageDraw.Draw(img)
    page = [S * 0.22, S * 0.16, S * 0.78, S * 0.84]
    d.rounded_rectangle(page, radius=S * 0.028, fill=PAGE)
    # Page edge on the right/bottom.
    d.rounded_rectangle(
        [page[0] + S * 0.006, page[1] + S * 0.010, page[2] + S * 0.004, page[3] + S * 0.006],
        radius=S * 0.028,
        outline=PAGE_EDGE,
        width=int(S * 0.004),
    )
    # Spine.
    d.rounded_rectangle(
        [S * 0.22, S * 0.16, S * 0.295, S * 0.84],
        radius=S * 0.026,
        fill=GOLD,
    )
    d.line(
        [(S * 0.303, S * 0.20), (S * 0.303, S * 0.80)],
        fill=(196, 143, 58, 180),
        width=int(S * 0.006),
    )

    # Headword.
    font = ImageFont.truetype(FONT, int(S * 0.36))
    d.text((S * 0.565, S * 0.395), "M", font=font, fill=TEXT_INK, anchor="mm")
    # A short gold rule and two "definition" lines.
    d.rounded_rectangle(
        [S * 0.40, S * 0.545, S * 0.60, S * 0.562], radius=S * 0.01, fill=GOLD
    )
    for i, width in enumerate((0.33, 0.27, 0.31, 0.22)):
        y = 0.615 + i * 0.055
        d.rounded_rectangle(
            [S * 0.395, S * y, S * (0.395 + width), S * (y + 0.020)],
            radius=S * 0.010,
            fill=(120, 126, 142, 210),
        )

    # Magnifier over the page, bottom-right.
    cx, cy, r = S * 0.735, S * 0.725, S * 0.105
    mshadow = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    ImageDraw.Draw(mshadow).ellipse(
        [cx - r, cy - r, cx + r, cy + r], fill=(0, 0, 0, 90)
    )
    mshadow = mshadow.filter(ImageFilter.GaussianBlur(S * 0.012))
    img.alpha_composite(mshadow)
    d = ImageDraw.Draw(img)
    d.ellipse([cx - r, cy - r, cx + r, cy + r], fill=PAGE, outline=CORAL, width=int(S * 0.030))
    d.line(
        [cx + r * 0.72, cy + r * 0.72, cx + r * 1.55, cy + r * 1.55],
        fill=CORAL,
        width=int(S * 0.038),
    )
    d.ellipse(
        [cx - r * 0.55, cy - r * 0.55, cx - r * 0.05, cy - r * 0.05],
        fill=(255, 255, 255, 150),
    )

    return img


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    icon = build()
    icon_1024 = icon.resize((1024, 1024), Image.LANCZOS)
    icon_1024.save(OUT / "icon.png")
    for name, size in {
        "32x32.png": 32,
        "128x128.png": 128,
        "128x128@2x.png": 256,
        "icon-512.png": 512,
        "icon-1024.png": 1024,
    }.items():
        icon.resize((size, size), Image.LANCZOS).save(OUT / name)
    icon_1024.save(
        OUT / "icon.ico",
        sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
    )
    icon_1024.save(OUT / "icon.icns", format="ICNS")
    print(f"icons written to {OUT}")


if __name__ == "__main__":
    main()
