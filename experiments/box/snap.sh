#!/bin/bash
# snap.sh DIR [SRC] [g]: build SRC (default ~/raijuhash) with -C target-cpu=native
# (or generic, runtime detection, with a third argument "g") and copy its
# binaries to ~/DIR, for A/B runs with ab.sh.
set -e
source ~/.cargo/env
SRC=${2:-$HOME/raijuhash}
cd "$SRC"
if [ "$3" = g ]; then
  CARGO_TARGET_DIR=~/tg cargo build --release --workspace --bins 2>&1 | grep -E "^(error|warning)" -A5 || true; T=~/tg
else
  CARGO_TARGET_DIR="$SRC/target" RUSTFLAGS="-C target-cpu=native" cargo build --release --workspace --bins 2>&1 | grep -E "^(error|warning)" -A5 || true; T="$SRC/target"
fi
mkdir -p ~/"$1"
for f in "$T"/release/*; do [ -f "$f" ] && [ -x "$f" ] && cp "$f" ~/"$1"/; done
ls ~/"$1" | wc -l
