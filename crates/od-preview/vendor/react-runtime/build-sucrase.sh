#!/usr/bin/env bash
# Rebuilds sucrase.min.js (global `Sucrase`, only `transform`) with every
# bundled package's license text in a leading /*! … */ comment, so the
# exported HTML carries the notices. Run from an empty folder:
#   npm install sucrase@3.35.0 esbuild@0.24.0
#   bash <this dir>/build-sucrase.sh <this dir>
set -euo pipefail
dir="$1"
{
  echo '/*!'
  echo ' * Sucrase 3.35.0 bundled for Nét (React component previews and exports), with:'
  echo ' * lines-and-columns 1.2.4 (MIT), ts-interface-checker 0.1.13 (Apache-2.0),'
  echo ' * @jridgewell/gen-mapping 0.3.13, @jridgewell/resolve-uri 3.1.2,'
  echo ' * @jridgewell/sourcemap-codec 1.6.0, @jridgewell/trace-mapping 0.3.31 (MIT).'
  for f in sucrase lines-and-columns ts-interface-checker @jridgewell-gen-mapping \
           @jridgewell-resolve-uri @jridgewell-sourcemap-codec @jridgewell-trace-mapping; do
    echo ' *'
    echo " * ---- $f ----"
    sed -e 's#\*/#* /#g' -e 's/^/ * /' -e 's/ *$//' "$dir/licenses/$f.LICENSE"
  done
  echo ' */'
} > banner.js
echo "export { transform } from 'sucrase';" > entry.js
npx esbuild entry.js --bundle --minify --format=iife --global-name=Sucrase \
  --platform=browser --legal-comments=inline --banner:js="$(cat banner.js)" \
  --outfile="$dir/sucrase.min.js"
