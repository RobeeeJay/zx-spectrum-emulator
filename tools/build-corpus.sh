#!/usr/bin/env bash
# Build a training corpus from the annotated game disassemblies, and score
# AutoDoc's rules against it.
#
# The disassemblies are other people's work and are not in this repository, so
# this fetches them, assembles each one back into the binary it describes, and
# reads the routines out of that with the emulator's own feature extractor.
#
#   tools/build-corpus.sh /tmp/corpus
#
# Needs SkoolKit for the assembling step: pip install skoolkit. It is a
# development tool, not a dependency of the emulator.
set -euo pipefail

work="${1:-/tmp/zxrs-corpus}"
repo="$(cd "$(dirname "$0")/.." && pwd)"
base="https://raw.githubusercontent.com/mrcook/zx-spectrum-games/master"
mkdir -p "$work"
cd "$work"

command -v skool2bin.py >/dev/null || {
    echo "skool2bin.py not found: pip install skoolkit" >&2
    exit 1
}
cargo build --release --manifest-path "$repo/Cargo.toml" --bin corpus

# Every .skool file in the collection, whatever it is called: the games do not
# agree on a naming scheme.
curl -sS "https://api.github.com/repos/mrcook/zx-spectrum-games/git/trees/master?recursive=1" |
    python3 -c "
import json,sys
for e in json.load(sys.stdin)['tree']:
    if e['path'].endswith('.skool'): print(e['path'])" > skools.txt

first=1
: > games.csv
while read -r path; do
    name="$(echo "$path" | tr '/' '_' | sed 's/\.skool$//')"
    curl -sSL -o "$name.skool" "$base/$(python3 -c "
import urllib.parse,sys; print(urllib.parse.quote(sys.argv[1]))" "$path")"
    [ -s "$name.skool" ] || continue

    python3 "$repo/tools/skool-symbols.py" "$name.skool" > "$name.symbols.txt" || continue
    skool2bin.py "$name.skool" "$name.bin" 2>/dev/null || continue
    [ -s "$name.bin" ] || continue

    # Where the binary starts: skool2bin says so, and the lowest address in the
    # symbol file is the fallback.
    org="$(skool2bin.py "$name.skool" /dev/null 2>&1 | grep -oE '\(0x[0-9A-Fa-f]+' | head -1 | tr -d '(0x')"
    [ -n "$org" ] || org="$(grep -oE '^[0-9A-F]{4} ' "$name.symbols.txt" | sort | head -1 | tr -d ' ')"

    "$repo/target/release/corpus" "$name.bin" "$name.symbols.txt" --org "$org" > one.csv
    if [ "$first" = 1 ]; then cp one.csv games.csv; first=0; else tail -n +2 one.csv >> games.csv; fi
    printf '%-40s $%s\n' "$name" "$org"
done < skools.txt

echo
python3 "$repo/tools/score-autodoc.py" games.csv
