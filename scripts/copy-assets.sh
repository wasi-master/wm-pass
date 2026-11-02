#!/bin/sh
# What the web shells serve from data/: the site marks, drawn only on the device.
set -e
cd "$(dirname "$0")/.."
mkdir -p ui/public/brands
cp -R data/brands/. ui/public/brands/
rm -f ui/public/brands/ABOUT-BRANDS.md
