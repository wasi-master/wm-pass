#!/usr/bin/env python3
"""Generates the icon styles that come from packs other than Material Symbols.

Each pack's glyphs are redrawn on Material's grid (960 units, y from -960 to 0), so the phone and
the desktop draw them through the same code as the Material styles. Which glyph stands for which
name is in packs.py. A pack drawn with lines keeps them as lines, and the style says how wide.
Run from the repository root after gen_symbols.py; downloaded packages are cached in target/packs.
"""
import io, pathlib, re, shutil, sys, tarfile, urllib.request

sys.path.insert(0, str(pathlib.Path(__file__).parent))
from names import phone_glyphs, web_glyphs  # noqa: E402
from packs import SOURCES, STYLES, TABLES  # noqa: E402

ICONS = pathlib.Path("android/app/src/main/java/com/wasimaster/wmpass/ui/icons")
WEB = pathlib.Path("ui/src/icons")
LICENCES = pathlib.Path("android/app/src/main/assets/licenses")
CACHE = pathlib.Path("target/packs")
CACHE.mkdir(parents=True, exist_ok=True)


def unpacked(pack: str) -> pathlib.Path:
    where = CACHE / pack
    if not where.exists():
        with urllib.request.urlopen(SOURCES[pack][2], timeout=60) as response:
            data = response.read()
        with tarfile.open(fileobj=io.BytesIO(data)) as archive:
            archive.extractall(where, filter="data")
    return where / "package"


def attributes(tag: str) -> dict:
    return dict(re.findall(r'([\w-]+)="([^"]*)"', tag))


def number(value: str) -> float:
    return float(value) if value else 0.0


def shapes_as_paths(svg: str) -> list:
    """Every drawing element of an icon as a path, on the icon's own grid."""
    parts = []
    for kind, tag in re.findall(r"<(path|circle|ellipse|rect|line|polyline|polygon)\b([^>]*)>", svg):
        a = attributes(tag)
        if kind == "path":
            parts.append(a["d"])
        elif kind in ("circle", "ellipse"):
            cx, cy = number(a.get("cx")), number(a.get("cy"))
            rx = number(a.get("r") or a.get("rx"))
            ry = number(a.get("r") or a.get("ry"))
            parts.append(f"M{cx - rx} {cy}a{rx} {ry} 0 1 0 {2 * rx} 0a{rx} {ry} 0 1 0 {-2 * rx} 0z")
        elif kind == "rect":
            x, y, w, h = (number(a.get(k)) for k in ("x", "y", "width", "height"))
            rx = number(a.get("rx") or a.get("ry"))
            ry = number(a.get("ry") or a.get("rx"))
            rx, ry = min(rx, w / 2), min(ry, h / 2)
            if rx == 0 or ry == 0:
                parts.append(f"M{x} {y}h{w}v{h}h{-w}z")
            else:
                parts.append(
                    f"M{x + rx} {y}h{w - 2 * rx}a{rx} {ry} 0 0 1 {rx} {ry}v{h - 2 * ry}a{rx} {ry} 0 0 1 {-rx} {ry}"
                    f"h{-(w - 2 * rx)}a{rx} {ry} 0 0 1 {-rx} {-ry}v{-(h - 2 * ry)}a{rx} {ry} 0 0 1 {rx} {-ry}z"
                )
        elif kind == "line":
            parts.append(f"M{a['x1']} {a['y1']}L{a['x2']} {a['y2']}")
        else:
            points = re.findall(r"[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?", a["points"])
            pairs = [f"{points[i]} {points[i + 1]}" for i in range(0, len(points) - 1, 2)]
            parts.append("M" + "L".join(pairs) + ("z" if kind == "polygon" else ""))
    return parts


NUMBER = re.compile(r"[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?")
ARGUMENTS = {"M": 2, "L": 2, "H": 1, "V": 1, "C": 6, "S": 4, "Q": 4, "T": 2, "A": 7, "Z": 0}


def commands(d: str):
    """(command, arguments) for a path, arc flags read one digit each as SVG allows."""
    i, end, command = 0, len(d), None

    def gap():
        nonlocal i
        while i < end and d[i] in " ,\t\r\n":
            i += 1

    def read():
        nonlocal i
        gap()
        match = NUMBER.match(d, i)
        if not match:
            raise ValueError(f"no number at {i} in {d[:60]}")
        i = match.end()
        return float(match.group())

    def flag():
        nonlocal i
        gap()
        value = d[i]
        i += 1
        return float(value)

    while True:
        gap()
        if i >= end:
            return
        if d[i].isalpha():
            command = d[i]
            i += 1
            if command in "Zz":
                yield command, []
                continue
        elif command is None or command in "Zz":
            raise ValueError(f"numbers without a command in {d[:60]}")
        if command in "Aa":
            yield command, [read(), read(), read(), flag(), flag(), read(), read()]
        else:
            yield command, [read() for _ in range(ARGUMENTS[command.upper()])]
        # Pairs after a move are lines.
        if command == "M":
            command = "L"
        elif command == "m":
            command = "l"


def short(value: float) -> str:
    text = f"{value:.1f}".rstrip("0").rstrip(".")
    return "0" if text in ("-0", "") else text


def onto_material_grid(d: str, side: float) -> str:
    """The path scaled from a `side`-unit square to Material's, and lifted to its y from -960 to 0."""
    k = 960 / side
    out = []
    for index, (command, args) in enumerate(commands(d)):
        # A path's first move has nothing to be relative to, so SVG reads it as absolute.
        upper, absolute = command.upper(), command.isupper() or index == 0
        if index == 0:
            command = upper
        if upper == "Z":
            out.append(command)
            continue
        if upper == "A":
            rx, ry, turn, large, sweep, x, y = args
            values = [rx * k, ry * k, turn, large, sweep, x * k, y * k - 960 if absolute else y * k]
        elif upper == "H":
            values = [args[0] * k]
        elif upper == "V":
            values = [args[0] * k - 960 if absolute else args[0] * k]
        else:
            values = [v * k - (960 if absolute and n % 2 else 0) for n, v in enumerate(args)]
        text = " ".join(short(v) for v in values).replace(" -", "-")
        out.append(command + text)
    return "".join(out)


def glyph_path(pack: str, root: pathlib.Path, symbol: str, weight) -> str:
    name = TABLES[pack].get(symbol, symbol.replace("_", "-"))
    if pack == "phosphor":
        file = root / "assets" / weight / (name + ("" if weight == "regular" else f"-{weight}") + ".svg")
    else:
        file = root / "icons" / f"{name}.svg"
    return "".join(onto_material_grid(d, SOURCES[pack][3]) for d in shapes_as_paths(file.read_text()))


roots = {pack: unpacked(pack) for pack in SOURCES}
LICENCES.mkdir(parents=True, exist_ok=True)
for pack, root in roots.items():
    shutil.copyfile(root / "LICENSE", LICENCES / f"{pack}-{SOURCES[pack][1].lower()}.txt")

phone, web = phone_glyphs(), web_glyphs()
for style, (code, pack, plain, solid, _) in STYLES.items():
    title, licence = SOURCES[pack][0], SOURCES[pack][1]
    header = f"Generated by tools/icon/gen_packs.py from {title} ({licence}). Do not edit."
    paths = [glyph_path(pack, roots[pack], symbol, solid if filled else plain) for _, symbol, filled in phone]
    rows = "\n".join(f'        {i} -> "{path}"' for i, path in enumerate(paths))
    (ICONS / f"IconPaths{code}.kt").write_text(
        f"// {header}\n"
        '@file:Suppress("MaxLineLength")\n\n'
        "package com.wasimaster.wmpass.ui.icons\n\n"
        f"internal object IconPaths{code} {{\n"
        "    fun path(index: Int): String = when (index) {\n"
        f"{rows}\n"
        '        else -> ""\n'
        "    }\n}\n"
    )
    paths = [glyph_path(pack, roots[pack], symbol, solid if filled else plain) for _, symbol, filled in web]
    rows = "\n".join(f'  {key}: "{path}",' for (key, _, _), path in zip(web, paths))
    (WEB / f"{style}.ts").write_text(f"// {header}\nexport default {{\n{rows}\n}} as const;\n")
    print(style, sum(len(p) for p in paths) // 1024, "KB")
