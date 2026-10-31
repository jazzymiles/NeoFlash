#!/bin/sh
# Concatenate src/*.js into a single code.js (Figma plugins load one file).
cd "$(dirname "$0")"
cat src/00-core.js src/10-v1-components.js src/20-v1-screens.js src/30-audit.js src/40-v2-system.js src/50-v2-screens.js src/99-main.js > code.js
node --check code.js && echo "code.js OK ($(wc -l < code.js) lines)"
