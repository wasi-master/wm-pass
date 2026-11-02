#!/usr/bin/env python3
"""Regenerates the WM Pass icons from one vector definition.

The mark follows WM Keyboard's lockup: the same white speech-bubble pin and WM
monogram, over a shield with a keyhole where the keyboard's body sits. Authored once
in a 108dp adaptive-icon canvas.

The WM outlines in wm_outline.txt come from WM Keyboard's own icon, so the font is
not needed here and is never shipped.

Requires: Pillow, rsvg-convert, cwebp.

    python3 tools/icon/gen_icon.py --preview /tmp/icon.png
    python3 tools/icon/gen_icon.py --out .
"""

from __future__ import annotations

import argparse
import math
import os
import shutil
import subprocess
import sys
import tempfile

from PIL import Image, ImageDraw

HERE = os.path.dirname(os.path.abspath(__file__))

# ---------------------------------------------------------------- palette
BG_TOP, BG_BOTTOM = "#0F2E2A", "#071A17"
BODY_TOP, BODY_BOTTOM = "#10B981", "#0D9488"
WM_A, WM_B = "#10B981", "#0D9488"
PIN_FILL = "#FFFFFF"
SHADOW = "#02120F"

# ---------------------------------------------------------------- geometry
C = 54.0
SAFE_R = 32.5

SHIELD_W, SHIELD_TOP, SHIELD_SIDE, SHIELD_TIP, SHIELD_R = 58.0, 45.0, 73.0, 99.0, 6.4
SHIELD_L = C - SHIELD_W / 2
SHIELD_RIGHT = C + SHIELD_W / 2

# Identical to WM Keyboard's pin, so the monogram lands in the same place.
PIN_W, PIN_BODY, PIN_TIP, PIN_RX, PIN_TIPW, PIN_TOP = 41.0, 33.0, 9.5, 9.5, 4.9, 17.5
PIN_HALO = 1.6

KEY_CY, KEY_R, KEY_SLOT_W, KEY_SLOT_H = 77.0, 3.6, 2.6, 8.2

DENSITIES = {"mdpi": 1, "hdpi": 1.5, "xhdpi": 2, "xxhdpi": 3, "xxxhdpi": 4}


def f(v: float) -> str:
    return f"{v:.2f}".rstrip("0").rstrip(".")


def shield_path() -> str:
    """Flat top with rounded corners, straight sides, then curves to a point."""
    l, r, t, s, tip, rad = SHIELD_L, SHIELD_RIGHT, SHIELD_TOP, SHIELD_SIDE, SHIELD_TIP, SHIELD_R
    return (
        f"M{f(l + rad)},{f(t)} H{f(r - rad)} A{f(rad)},{f(rad)} 0 0 1 {f(r)},{f(t + rad)} "
        f"V{f(s)} C{f(r)},{f(s + 12)} {f(C + 13)},{f(tip - 6)} {f(C)},{f(tip)} "
        f"C{f(C - 13)},{f(tip - 6)} {f(l)},{f(s + 12)} {f(l)},{f(s)} "
        f"V{f(t + rad)} A{f(rad)},{f(rad)} 0 0 1 {f(l + rad)},{f(t)} Z"
    )


def keyhole_path() -> str:
    """A circle and a tapered slot, as one shape."""
    r, cy = KEY_R, KEY_CY
    half = KEY_SLOT_W / 2
    # Where the slot leaves the circle.
    join = cy + math.sqrt(r * r - half * half)
    bottom = cy + KEY_SLOT_H + 1.4
    return (
        f"M{f(C - half)},{f(join)} A{f(r)},{f(r)} 0 1 1 {f(C + half)},{f(join)} "
        f"L{f(C + half + 0.7)},{f(bottom)} H{f(C - half - 0.7)} Z"
    )


def pin_path(grow: float = 0.0) -> str:
    w, body, tip = PIN_W + 2 * grow, PIN_BODY + 2 * grow, PIN_TIP + grow
    rx, tw, top = PIN_RX + grow, PIN_TIPW + grow * 1.05, PIN_TOP - grow
    l, r = C - w / 2, C + w / 2
    b, ty = top + body, top + body + tip
    return (
        f"M{f(l + rx)},{f(top)} H{f(r - rx)} A{f(rx)},{f(rx)} 0 0 1 {f(r)},{f(top + rx)} "
        f"V{f(b - rx)} A{f(rx)},{f(rx)} 0 0 1 {f(r - rx)},{f(b)} "
        f"H{f(C + tw)} L{f(C)},{f(ty)} L{f(C - tw)},{f(b)} "
        f"H{f(l + rx)} A{f(rx)},{f(rx)} 0 0 1 {f(l)},{f(b - rx)} "
        f"V{f(top + rx)} A{f(rx)},{f(rx)} 0 0 1 {f(l + rx)},{f(top)} Z"
    )


def lockup_scale() -> float:
    """Uniform scale about the centre that keeps every mask's crop clean."""
    points = [(SHIELD_L, PIN_TOP), (SHIELD_RIGHT, PIN_TOP), (SHIELD_L, SHIELD_SIDE), (SHIELD_RIGHT, SHIELD_SIDE), (C, SHIELD_TIP)]
    worst = max(math.hypot(x - C, y - C) for x, y in points)
    return min(1.0, SAFE_R / worst)


def gradient(gid: str, c1: str, c2: str) -> str:
    return (f'<linearGradient id="{gid}" x1="0" y1="0" x2="1" y2="1">'
            f'<stop offset="0" stop-color="{c1}"/><stop offset="1" stop-color="{c2}"/>'
            f'</linearGradient>')


def svg(body: str, defs: str = "") -> str:
    return ('<svg xmlns="http://www.w3.org/2000/svg" width="108" height="108" '
            f'viewBox="0 0 108 108"><defs>{defs}</defs>{body}</svg>')


def background_svg() -> str:
    return svg('<rect width="108" height="108" fill="url(#bg)"/>', gradient("bg", BG_TOP, BG_BOTTOM))


SHADOW_FILTER = ('<filter id="sh" x="-30%" y="-30%" width="160%" height="175%">'
                 f'<feDropShadow dx="0" dy="1.6" stdDeviation="1.7" flood-color="{SHADOW}" '
                 'flood-opacity="0.45"/></filter>')


def foreground_svg(wm: str) -> str:
    s = lockup_scale()
    mask = (f'<mask id="body"><rect width="108" height="108" fill="black"/>'
            f'<path d="{shield_path()}" fill="white"/>'
            f'<path d="{keyhole_path()}" fill="black"/>'
            f'<path d="{pin_path(PIN_HALO)}" fill="black"/></mask>')
    inner = (f'<g filter="url(#sh)">'
             f'<rect width="108" height="108" fill="url(#bodyg)" mask="url(#body)"/>'
             f'<path d="{pin_path()}" fill="{PIN_FILL}"/></g>'
             f'<path d="{wm}" fill="url(#wmg)"/>')
    return svg(f'<g transform="translate({C},{C}) scale({s:.4f}) translate({-C},{-C})">{inner}</g>',
               SHADOW_FILTER + mask + gradient("bodyg", BODY_TOP, BODY_BOTTOM) + gradient("wmg", WM_A, WM_B))


def background_xml() -> str:
    return f"""<?xml version="1.0" encoding="utf-8"?>
<vector xmlns:android="http://schemas.android.com/apk/res/android"
    xmlns:aapt="http://schemas.android.com/aapt"
    android:width="108dp"
    android:height="108dp"
    android:viewportWidth="108"
    android:viewportHeight="108">
    <path android:pathData="M0,0h108v108h-108z">
        <aapt:attr name="android:fillColor">
            <gradient
                android:startX="0"
                android:startY="0"
                android:endX="108"
                android:endY="108"
                android:type="linear">
                <item
                    android:color="#FF{BG_TOP[1:]}"
                    android:offset="0.0" />
                <item
                    android:color="#FF{BG_BOTTOM[1:]}"
                    android:offset="1.0" />
            </gradient>
        </aapt:attr>
    </path>
</vector>
"""


def monochrome_xml(wm: str) -> str:
    s = lockup_scale()
    data = " ".join([shield_path(), keyhole_path(), pin_path(PIN_HALO), pin_path(), wm])
    return f"""<?xml version="1.0" encoding="utf-8"?>
<vector xmlns:android="http://schemas.android.com/apk/res/android"
    android:width="108dp"
    android:height="108dp"
    android:viewportWidth="108"
    android:viewportHeight="108">
    <!-- Themed-icon layer. Even-odd fill knocks the keyhole, pin gap and monogram out. -->
    <group
        android:pivotX="54"
        android:pivotY="54"
        android:scaleX="{s:.4f}"
        android:scaleY="{s:.4f}">
        <path
            android:fillColor="#FFFFFFFF"
            android:fillType="evenOdd"
            android:pathData="{data}" />
    </group>
</vector>
"""


def render(svg_text: str, px: int, tmp: str, name: str) -> str:
    src = os.path.join(tmp, f"{name}.svg")
    dst = os.path.join(tmp, f"{name}-{px}.png")
    with open(src, "w") as fh:
        fh.write(svg_text)
    subprocess.run(["rsvg-convert", "-w", str(px), "-h", str(px), src, "-o", dst], check=True)
    return dst


def to_webp(png: str, dst: str) -> None:
    os.makedirs(os.path.dirname(dst), exist_ok=True)
    subprocess.run(["cwebp", "-lossless", "-exact", "-quiet", png, "-o", dst], check=True)


def legacy(compose: Image.Image, px: int, round_icon: bool) -> Image.Image:
    n = int(compose.width * 72 / 108)
    crop = compose.crop(((compose.width - n) // 2,) * 2 + ((compose.width + n) // 2,) * 2)
    crop = crop.resize((px * 4, px * 4), Image.LANCZOS)
    mask = Image.new("L", crop.size, 0)
    draw = ImageDraw.Draw(mask)
    if round_icon:
        draw.ellipse((0, 0, crop.width - 1, crop.height - 1), fill=255)
    else:
        draw.rounded_rectangle((0, 0, crop.width - 1, crop.height - 1), radius=int(crop.width * 0.12), fill=255)
    out = Image.new("RGBA", crop.size, (0, 0, 0, 0))
    out.paste(crop, (0, 0), mask)
    return out.resize((px, px), Image.LANCZOS)


def composed(bg: str, fg: str, px: int, tmp: str, name: str) -> Image.Image:
    return Image.alpha_composite(
        Image.open(render(bg, px, tmp, f"bg-{name}")).convert("RGBA"),
        Image.open(render(fg, px, tmp, f"fg-{name}")).convert("RGBA"),
    )


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", help="repository root to write assets under")
    ap.add_argument("--preview", help="write a 512px preview PNG here")
    args = ap.parse_args()
    for tool in ("rsvg-convert", "cwebp"):
        if not shutil.which(tool):
            sys.exit(f"missing required tool: {tool}")
    with open(os.path.join(HERE, "wm_outline.txt")) as fh:
        wm = fh.read().strip()
    bg, fg = background_svg(), foreground_svg(wm)

    with tempfile.TemporaryDirectory() as tmp:
        store = composed(bg, fg, 512, tmp, "store")
        if args.preview:
            store.save(args.preview)
        if not args.out:
            return

        res = os.path.join(args.out, "android", "app", "src", "main", "res")
        os.makedirs(os.path.join(res, "drawable"), exist_ok=True)
        with open(os.path.join(res, "drawable", "ic_launcher_background.xml"), "w") as fh:
            fh.write(background_xml())
        with open(os.path.join(res, "drawable", "ic_launcher_monochrome.xml"), "w") as fh:
            fh.write(monochrome_xml(wm))
        for dpi, factor in DENSITIES.items():
            px = int(round(108 * factor))
            to_webp(render(fg, px, tmp, f"fg-{dpi}"), os.path.join(res, f"mipmap-{dpi}", "ic_launcher_fg.webp"))
            legacy_px = int(round(48 * factor))
            hi = composed(bg, fg, legacy_px * 4, tmp, f"legacy-{dpi}")
            for round_icon, name in ((False, "ic_launcher"), (True, "ic_launcher_round")):
                png = os.path.join(tmp, f"{name}-{dpi}.png")
                legacy(hi, legacy_px, round_icon).save(png)
                to_webp(png, os.path.join(res, f"mipmap-{dpi}", f"{name}.webp"))
        store.save(os.path.join(args.out, "android", "app", "src", "main", "ic_launcher-playstore.png"))

        icons = os.path.join(args.out, "assets", "icon")
        os.makedirs(icons, exist_ok=True)
        with open(os.path.join(icons, "wm-pass-foreground.svg"), "w") as fh:
            fh.write(fg)
        full = composed(bg, fg, 1024, tmp, "full")
        # Square icon with rounded corners, for desktop and the extension.
        mask = Image.new("L", full.size, 0)
        ImageDraw.Draw(mask).rounded_rectangle((0, 0, 1023, 1023), radius=184, fill=255)
        rounded = Image.new("RGBA", full.size, (0, 0, 0, 0))
        rounded.paste(full, (0, 0), mask)
        rounded.save(os.path.join(icons, "wm-pass-1024.png"))
        for px in (16, 32, 48, 128, 256, 512):
            rounded.resize((px, px), Image.LANCZOS).save(os.path.join(icons, f"wm-pass-{px}.png"))
    print("icon assets written")


if __name__ == "__main__":
    main()
