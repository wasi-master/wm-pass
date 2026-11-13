#!/usr/bin/env python3
"""Write the AUR package wm-pass-bin for a release.

    packaging/aur/stamp.py --version 0.1.0 --assets assets [--out target/aur]

Each architecture installs the release's .deb. One whose .deb is not in --assets is left out. To
publish, copy the folder into a clone of ssh://aur@aur.archlinux.org/wm-pass-bin.git and push.
"""

import argparse
import hashlib
from pathlib import Path

REPO = "https://github.com/wasi-master/wm-pass"
ARCHES = [("x86_64", "amd64"), ("aarch64", "arm64")]
DEPENDS = ["gtk3", "webkit2gtk-4.1", "libayatana-appindicator", "hicolor-icon-theme"]
OPTDEPENDS = ["org.freedesktop.secrets: quick unlock and sealed destinations"]

args = argparse.ArgumentParser(description=__doc__)
args.add_argument("--version", required=True)
args.add_argument("--assets", type=Path, required=True)
args.add_argument("--out", type=Path, default=Path("target/aur"))
args = args.parse_args()

v = args.version
lines = [
    "pkgname=wm-pass-bin",
    f"pkgver={v}",
    "pkgrel=1",
    "pkgdesc='Password manager that keeps its vault on your own devices'",
    f"url='{REPO}'",
    "license=('MIT')",
    "provides=('wm-pass')",
    "conflicts=('wm-pass')",
    "depends=(" + " ".join(f"'{d}'" for d in DEPENDS) + ")",
    "optdepends=(" + " ".join(f"'{d}'" for d in OPTDEPENDS) + ")",
]
arches = []
for arch, deb in ARCHES:
    name = f"WM.Pass_{v}_{deb}.deb"
    file = args.assets / name
    if not file.exists():
        continue
    arches.append(arch)
    lines.append(f"source_{arch}=('{REPO}/releases/download/desktop-v{v}/{name}')")
    lines.append(f"sha256sums_{arch}=('{hashlib.sha256(file.read_bytes()).hexdigest()}')")
lines.insert(4, "arch=(" + " ".join(f"'{a}'" for a in arches) + ")")
lines += ["", "package() {", '  bsdtar -xf data.tar.* -C "$pkgdir"', "}", ""]
out = args.out / "wm-pass-bin"
out.mkdir(parents=True, exist_ok=True)
(out / "PKGBUILD").write_text("\n".join(lines))
