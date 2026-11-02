#!/usr/bin/env python3
"""Set `version` and `sha256` in the cask: stamp.py <version> <sha256 of the dmg>."""

import pathlib
import re
import sys

version, digest = sys.argv[1:3]
if not re.fullmatch(r"[0-9a-f]{64}", digest):
    sys.exit(f"not a sha256: {digest!r}")
path = pathlib.Path(__file__).parent / "Casks/wm-pass.rb"
text = path.read_text()
text = re.sub(r'^(  version ")[^"]+(")', rf"\g<1>{version}\g<2>", text, count=1, flags=re.M)
text = re.sub(r'^(  sha256 ")[0-9a-f]{64}(")', rf"\g<1>{digest}\g<2>", text, count=1, flags=re.M)
path.write_text(text)
