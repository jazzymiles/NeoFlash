#!/bin/sh
# Concatenate src/*.js (name order) into code.js for the Figma development plugin and syntax-check it.
cd "$(dirname "$0")"
cat src/*.js > code.js
node --check code.js && echo "code.js OK ($(wc -l < code.js) lines)"
