#!/bin/sh
# The site at wmpass.pages.dev: the docs, with the web vault under /vault/.
set -e
cd "$(dirname "$0")/.."
npm run web:build
(cd docs && npm run build)
rm -rf docs/dist/vault
cp -R web/dist docs/dist/vault
