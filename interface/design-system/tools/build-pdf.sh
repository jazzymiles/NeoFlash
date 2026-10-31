#!/bin/sh
# Render docs/source/build/*.html to docs/pdf/*.pdf with headless Google Chrome.
# Run tools/build-docs.py first.
set -e
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CHROME="${CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
mkdir -p "$ROOT/docs/pdf"
for html in "$ROOT"/docs/source/build/*.html; do
  name="$(basename "$html" .html)"
  "$CHROME" --headless=new --disable-gpu --no-pdf-header-footer --allow-file-access-from-files \
    --print-to-pdf="$ROOT/docs/pdf/$name.pdf" "file://$html" 2>/dev/null
  echo "docs/pdf/$name.pdf  $(du -h "$ROOT/docs/pdf/$name.pdf" | cut -f1)"
done
