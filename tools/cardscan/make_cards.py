"""Makes made-up payment card pictures for testing the card readers, with what each one shows.

Usage: python3 -I tools/cardscan/make_cards.py OUT_DIR [COUNT] [FONT_DIR]

Every number is random and passes the Luhn check, so none is a real card. Each picture is
drawn the way cards are printed, flat, embossed or in foil, and then spoiled the way a phone
photo is: tilted, blurred, grainy, glared, compressed. Card fonts make the test worth having:
OCR-A and OCR-B (`brew install --cask font-ocr`) and Kredit (free, from 1001fonts, put in
FONT_DIR) stand in for the Farrington 7B that cards are embossed in; the computer's own sans
fonts stand in for flat print.
`truth.json` in OUT_DIR lists each file's number, expiry (MM/YYYY) and name.
"""
import json
import os
import random
import sys

from PIL import Image, ImageDraw, ImageFilter, ImageFont

FONT_DIRS = ["/System/Library/Fonts", "/System/Library/Fonts/Supplemental", "/Library/Fonts", os.path.expanduser("~/Library/Fonts")]
# Raised digits: Kredit and OCR-A, the nearest free fonts to Farrington 7B.
EMBOSSED_FONTS = ["Kredit.otf", "Kredit Shine.otf", "OCRA.otf"]
# Printed digits: OCR-B and OCR-A, which many cards print in, and the sans faces banks pick.
FLAT_FONTS = ["OCRB.otf", "OCRA.otf", "Kredit Front.otf", "DIN Alternate Bold.ttf", "Helvetica.ttc", "Arial Bold.ttf", "Avenir Next.ttc"]
NAMES = ["JANE Q PUBLIC", "RAHIM UDDIN", "ANA MARIA LOPEZ", "WEI ZHANG", "FATIMA KHAN", "JOHN A SMITH", "NUSRAT JAHAN"]
BANKS = ["NORTHWIND BANK", "CONTOSO", "FABRIKAM", "TAILSPIN", "ADVENTURE WORKS"]
W, H = 856, 540
RNG = random.Random(1077)


def font_path(name):
    for folder in FONT_DIRS:
        path = os.path.join(folder, name)
        if os.path.exists(path):
            return path
    return None


def luhn_number(prefix, length):
    digits = [int(c) for c in prefix]
    while len(digits) < length - 1:
        digits.append(RNG.randint(0, 9))
    total = 0
    for i, d in enumerate(reversed(digits)):
        if i % 2 == 0:
            d *= 2
            if d > 9:
                d -= 9
        total += d
    digits.append((10 - total % 10) % 10)
    return "".join(map(str, digits))


def grouped(number):
    if len(number) == 15:
        return " ".join([number[:4], number[4:10], number[10:]])
    return " ".join(number[i:i + 4] for i in range(0, len(number), 4))


def background(rng):
    kind = rng.choice(["solid", "gradient", "waves", "noise"])
    base = tuple(rng.randint(10, 245) for _ in range(3))
    other = tuple(rng.randint(10, 245) for _ in range(3))
    card = Image.new("RGB", (W, H), base)
    draw = ImageDraw.Draw(card)
    if kind == "gradient":
        for x in range(W):
            t = x / W
            draw.line([(x, 0), (x, H)], fill=tuple(int(base[i] * (1 - t) + other[i] * t) for i in range(3)))
    elif kind == "waves":
        for y in range(-H, H * 2, 18):
            draw.arc([-W // 2, y, W * 1.5, y + H], 0, 180, fill=other, width=3)
    elif kind == "noise":
        speckle = Image.effect_noise((W, H), 40).convert("RGB")
        card = Image.blend(card, speckle, 0.25)
    return card, base


def ink_for(base, rng):
    light = sum(base) / 3 > 128
    return (20, 20, 20) if light and rng.random() < 0.8 else (235, 235, 235)


def text(draw, at, words, font, style, ink):
    x, y = at
    if style == "embossed":
        # Raised digits: a light edge above and left, a dark one below and right, metal between.
        draw.text((x - 2, y - 2), words, font=font, fill=(250, 250, 250))
        draw.text((x + 2, y + 2), words, font=font, fill=(40, 40, 40))
        draw.text((x, y), words, font=font, fill=(185, 185, 190))
    elif style == "foil":
        draw.text((x + 1, y + 1), words, font=font, fill=(90, 70, 20))
        draw.text((x, y), words, font=font, fill=(212, 175, 55))
    else:
        draw.text((x, y), words, font=font, fill=ink)


def card(index, rng):
    prefix, length = rng.choice([("4", 16), ("51", 16), ("55", 16), ("37", 15), ("6011", 16), ("4", 13)])
    number = luhn_number(prefix, length)
    month, year = rng.randint(1, 12), rng.randint(2027, 2034)
    name = rng.choice(NAMES)
    style = rng.choice(["flat", "flat", "embossed", "foil"])
    font_file = font_path(rng.choice(EMBOSSED_FONTS if style != "flat" else FLAT_FONTS)) or font_path("Helvetica.ttc")
    picture, base = background(rng)
    draw = ImageDraw.Draw(picture)
    ink = ink_for(base, rng)
    small = ImageFont.truetype(font_file, 30)
    draw.text((50, 40), rng.choice(BANKS), font=small, fill=ink)
    # The chip.
    draw.rounded_rectangle([70, 170, 175, 245], 12, fill=(200, 170, 90), outline=(150, 120, 50), width=2)
    size = rng.randint(52, 66)
    big = ImageFont.truetype(font_file, size)
    text(draw, (60, 290), grouped(number), big, style, ink)
    label = ImageFont.truetype(font_file, 18)
    draw.text((330, 375), "VALID\nTHRU", font=label, fill=ink)
    text(draw, (400, 370), f"{month:02d}/{year % 100:02d}", ImageFont.truetype(font_file, 36), style, ink)
    text(draw, (60, 440), name, ImageFont.truetype(font_file, 38), style, ink)
    picture = spoil(picture, rng)
    return picture, {"number": number, "expiry": f"{month:02d}/{year}", "name": name, "style": style, "font": os.path.basename(font_file)}


def spoil(picture, rng):
    # A hand never holds a card square to the lens.
    picture = picture.rotate(rng.uniform(-4, 4), resample=Image.BICUBIC, expand=False, fillcolor=(60, 60, 60))
    shear = rng.uniform(-0.04, 0.04)
    picture = picture.transform((W, H), Image.AFFINE, (1, shear, -shear * H / 2, 0, 1, 0), resample=Image.BICUBIC, fillcolor=(60, 60, 60))
    if rng.random() < 0.5:
        glare = Image.new("L", (W, H), 0)
        ImageDraw.Draw(glare).ellipse([rng.randint(0, W // 2), rng.randint(0, H // 2), rng.randint(W // 2, W), rng.randint(H // 2, H)], fill=rng.randint(60, 140))
        glare = glare.filter(ImageFilter.GaussianBlur(60))
        picture = Image.composite(Image.new("RGB", (W, H), (255, 255, 255)), picture, glare)
    picture = picture.filter(ImageFilter.GaussianBlur(rng.uniform(0, 1.6)))
    grain = Image.effect_noise((W, H), rng.uniform(5, 30)).convert("RGB")
    picture = Image.blend(picture, grain, rng.uniform(0.02, 0.12))
    scale = rng.uniform(0.6, 1.0)
    return picture.resize((int(W * scale), int(H * scale)), Image.BILINEAR)


def main():
    out = sys.argv[1]
    count = int(sys.argv[2]) if len(sys.argv) > 2 else 48
    if len(sys.argv) > 3:
        FONT_DIRS.append(sys.argv[3])
    os.makedirs(out, exist_ok=True)
    rng = RNG
    truth = {}
    for i in range(count):
        picture, facts = card(i, rng)
        name = f"card-{i:02d}.png"
        # Through JPEG and back, as a phone keeps photos.
        tmp = os.path.join(out, "tmp.jpg")
        picture.save(tmp, quality=rng.randint(45, 90))
        Image.open(tmp).convert("RGBA").save(os.path.join(out, name))
        os.remove(tmp)
        truth[name] = facts
    with open(os.path.join(out, "truth.json"), "w") as f:
        json.dump(truth, f, indent=1)
    print(f"{count} cards in {out}")


if __name__ == "__main__":
    main()
