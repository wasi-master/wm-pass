"""Play Store phone slides for WM Pass, in the style of WM Keyboard's (play/pipeline there).

Each slide is a headline, a line under it, a row of feature chips and a framed screen from
android/app/build/play-shots/, which PlayShots.kt draws with fake data:

    cd android && ./gradlew :app:testDebugUnitTest --tests 'com.wasimaster.wmpass.PlayShots'
    python3 play/slides.py

Writes 1200x2400 PNGs (1:2, the tallest shape Play takes) to
fastlane/metadata/android/en-US/images/phoneScreenshots/.
"""

from __future__ import annotations

from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = Path(__file__).resolve().parents[1]
SHOTS = ROOT / "android/app/build/play-shots"
FONTS = ROOT / "android/vendor/wm-suite/theme/src/main/res/font"
LOGO = ROOT / "assets/icon/wm-pass-256.png"
OUT = ROOT / "fastlane/metadata/android/en-US/images/phoneScreenshots"

SS = 2  # drawn at twice the size, then scaled down, so edges stay clean
W, H = 1200, 2400

GROUND = (6, 19, 17)          # #061311, the app's dark ground
EMERALD = (16, 185, 129)      # #10B981
TEAL = (13, 148, 136)         # #0D9488
MINT = (94, 234, 212)         # #5EEAD4
AMBER = (251, 191, 36)        # #FBBF24
WHITE = (255, 255, 255)
BODY = (170, 196, 188)
CHIP_TEXT = (222, 236, 231)

_fonts: dict[tuple, ImageFont.FreeTypeFont] = {}


def font(name: str, size: int) -> ImageFont.FreeTypeFont:
    key = (name, size)
    if key not in _fonts:
        _fonts[key] = ImageFont.truetype(str(FONTS / f"{name}.ttf"), size * SS)
    return _fonts[key]


def width(s: str, f) -> int:
    d = ImageDraw.Draw(Image.new("RGB", (8, 8)))
    b = d.textbbox((0, 0), s, font=f)
    return (b[2] - b[0]) // SS


def blob(img, cx, cy, r, color, alpha=80, blur=220):
    layer = Image.new("RGBA", img.size, (0, 0, 0, 0))
    ImageDraw.Draw(layer).ellipse([(cx - r) * SS, (cy - r) * SS, (cx + r) * SS, (cy + r) * SS], fill=color + (alpha,))
    layer = layer.filter(ImageFilter.GaussianBlur(blur * SS))
    img.paste(Image.alpha_composite(img.convert("RGBA"), layer).convert("RGB"))


def vignette(img, strength=110):
    w, h = img.size
    mask = Image.new("L", (w, h), 0)
    ImageDraw.Draw(mask).ellipse([-w * 0.25, -h * 0.18, w * 1.25, h * 1.12], fill=strength)
    mask = mask.filter(ImageFilter.GaussianBlur(min(w, h) // 6))
    img.paste(Image.composite(img, Image.new("RGB", (w, h)), mask.point(lambda p: 255 - (strength - p))))


def grain(img, opacity=6):
    w, h = img.size
    noise = Image.effect_noise((w // 2, h // 2), 22).resize((w, h))
    img.paste(Image.blend(img, Image.merge("RGB", (noise, noise, noise)), opacity / 255))


def gradient(size, c1, c2):
    g = Image.new("RGB", (256, 1))
    for x in range(256):
        t = x / 255
        g.putpixel((x, 0), tuple(int(c1[i] + (c2[i] - c1[i]) * t) for i in range(3)))
    return g.resize(size)


def text(img, xy, s, f, fill=WHITE, anchor="la"):
    ImageDraw.Draw(img).text((xy[0] * SS, xy[1] * SS), s, font=f, fill=fill, anchor=anchor)


def gradient_text(img, xy, s, f, c1=MINT, c2=EMERALD, glow=6):
    mask = Image.new("L", img.size, 0)
    ImageDraw.Draw(mask).text((xy[0] * SS, xy[1] * SS), s, font=f, fill=255)
    box = mask.getbbox()
    grad = Image.new("RGB", img.size)
    if box:
        grad.paste(gradient((box[2] - box[0], 1), c1, c2).resize((box[2] - box[0], img.size[1])), (box[0], 0))
    if glow:
        layer = Image.new("RGBA", img.size, (0, 0, 0, 0))
        layer.paste(grad.convert("RGBA"), (0, 0), mask.filter(ImageFilter.GaussianBlur(glow * SS)))
        img.paste(Image.alpha_composite(img.convert("RGBA"), layer).convert("RGB"))
    img.paste(grad, (0, 0), mask)


def headline_line(img, y, parts, size=84):
    """One centred line; parts is a list of (text, accented)."""
    f = font("manrope_bold", size)
    x = W // 2 - sum(width(s, f) for s, _ in parts) // 2
    for s, accent in parts:
        if accent:
            gradient_text(img, (x, y), s, f)
        else:
            text(img, (x, y), s, f)
        x += width(s, f)


def brand_row(img, y=92, size=60):
    logo = Image.open(LOGO).convert("RGBA").resize((size * SS, size * SS), Image.LANCZOS)
    f = font("inter_semibold", 34)
    total = size + 20 + width("WM Pass", f)
    x0 = W // 2 - total // 2
    img.paste(logo, (x0 * SS, y * SS), logo)
    text(img, (x0 + size + 20, y + size // 2), "WM Pass", f, fill=(226, 238, 234), anchor="lm")


def chip_row(img, y, labels):
    f = font("inter_medium", 29)
    pad_x, pad_y, gap, dot_w = 26, 15, 16, 18
    d = ImageDraw.Draw(img, "RGBA")
    widths = [width(s, f) + pad_x * 2 + (dot_w if a else 0) for s, a in labels]
    x = W // 2 - (sum(widths) + gap * (len(labels) - 1)) // 2
    h = 34 + pad_y * 2
    for (s, accent), w in zip(labels, widths):
        outline = accent + (120,) if accent else (255, 255, 255, 30)
        d.rounded_rectangle([x * SS, y * SS, (x + w) * SS, (y + h) * SS], (h // 2) * SS,
                            fill=(255, 255, 255, 14), outline=outline, width=SS)
        tx = x + pad_x
        if accent:
            cy = y + h / 2
            d.ellipse([(tx - 2) * SS, (cy - 4) * SS, (tx + 7) * SS, (cy + 5) * SS], fill=accent + (255,))
            tx += dot_w
        d.text((tx * SS, (y + h / 2) * SS), s, font=f, fill=accent or CHIP_TEXT, anchor="lm")
        x += w + gap


def phone(img, shot, y, w=860):
    """The screen in a slim dark bezel, centred, with a soft green glow under it."""
    ratio = shot.height / shot.width
    h = int(w * ratio)
    x = (W - w) // 2
    bezel, radius = 9, 56
    ob = (x - bezel, y - bezel, x + w + bezel, y + h + bezel)

    glow = Image.new("RGBA", img.size, (0, 0, 0, 0))
    gd = ImageDraw.Draw(glow)
    mid = (ob[1] + ob[3]) / 2
    gd.rounded_rectangle([ob[0] * SS, ob[1] * SS, ob[2] * SS, mid * SS], (radius + bezel) * SS, fill=EMERALD + (60,))
    gd.rounded_rectangle([ob[0] * SS, mid * SS, ob[2] * SS, ob[3] * SS], (radius + bezel) * SS, fill=TEAL + (60,))
    glow = glow.filter(ImageFilter.GaussianBlur(70 * SS))
    img.paste(Image.alpha_composite(img.convert("RGBA"), glow).convert("RGB"))

    shade = Image.new("RGBA", img.size, (0, 0, 0, 0))
    ImageDraw.Draw(shade).rounded_rectangle([ob[0] * SS, (ob[1] + 26) * SS, ob[2] * SS, (ob[3] + 26) * SS],
                                            (radius + bezel) * SS, fill=(0, 0, 0, 180))
    shade = shade.filter(ImageFilter.GaussianBlur(40 * SS))
    img.paste(Image.alpha_composite(img.convert("RGBA"), shade).convert("RGB"))

    d = ImageDraw.Draw(img, "RGBA")
    d.rounded_rectangle([v * SS for v in ob], (radius + bezel) * SS, fill=(18, 28, 26, 255),
                        outline=(255, 255, 255, 28), width=SS)
    screen = shot.convert("RGB").resize((w * SS, h * SS), Image.LANCZOS)
    mask = Image.new("L", screen.size, 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, screen.width, screen.height], radius * SS, fill=255)
    img.paste(screen, (x * SS, y * SS), mask)
    return ob


def slide(name, lines, sub, chips, shot, blobs=((1000, 260, EMERALD), (120, 1100, TEAL), (1050, 1900, MINT))):
    img = Image.new("RGB", (W * SS, H * SS), GROUND)
    for cx, cy, c in blobs:
        blob(img, cx, cy, 420, c, 55)
    brand_row(img)
    y = 210
    for parts in lines:
        headline_line(img, y, parts)
        y += 108
    text(img, (W // 2, y + 20), sub, font("inter_regular", 35), fill=BODY, anchor="ma")
    chip_row(img, y + 104, chips)
    ob = phone(img, Image.open(SHOTS / f"{shot}.png"), y + 230)
    text(img, (W // 2, ob[3] + 78), "No account. No ads. Open source.", font("inter_medium", 32), fill=BODY, anchor="ma")
    vignette(img)
    grain(img)
    out = img.resize((W, H), Image.LANCZOS)
    OUT.mkdir(parents=True, exist_ok=True)
    out.save(OUT / f"{name}.png", optimize=True)


SLIDES = [
    ("1", [[("Your passwords,", False)], [("on ", False), ("your own devices", True)]],
     "No account to make. Works with the network off.",
     [("Autofill", EMERALD), ("Passkeys", None), ("Open source", MINT)], "vault"),
    ("2", [[("Every login,", False)], [("one tap", True), (" away", False)]],
     "Copy it, open the app, or go straight to the site's own page.",
     [("Change-password links", EMERALD), ("Fills in apps", None)], "entry"),
    ("3", [[("Knows the ", False), ("real app", True)], [("for every site", False)]],
     "Logins match an app by its signing certificate, not its name.",
     [("Websites and apps", EMERALD), ("Goes with this login", None)], "editor"),
    ("4", [[("Your ", False), ("authenticator", True)], [("is built in", False)]],
     "Every one-time code on one screen, counting down.",
     [("TOTP codes", EMERALD), ("Google Authenticator import", None)], "codes"),
    ("5", [[("Strong passwords,", False)], [("made ", False), ("to fit", True)]],
     "Random, words or pronounceable, with the strength shown.",
     [("Time to crack", EMERALD), ("Words or random", None), ("Save as login", None)], "generator"),
    ("6", [[("Find the ", False), ("weak spots", True)], [("first", False)]],
     "Reused, easy to guess and missing second factors, worst first.",
     [("Vault health", AMBER), ("Reused passwords", None), ("Missing 2FA", None)], "health"),
    ("7", [[("Make it ", False), ("yours", True)]],
     "Themes, layouts and styles, all in Settings.",
     [("Glass", MINT), ("Light and dark", None), ("Your own themes", None)], "glass"),
    ("8", [[("Private ", False), ("by default", True)]],
     "Every network feature starts switched off.",
     [("Duress vault", AMBER), ("Hidden vaults", None), ("Hardware keys", None)], "locked"),
]

if __name__ == "__main__":
    for name, lines, sub, chips, shot in SLIDES:
        slide(name, lines, sub, chips, shot)
        print("wrote", OUT / f"{name}.png")
