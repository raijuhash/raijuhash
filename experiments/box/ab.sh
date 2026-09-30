#!/bin/bash
# ab.sh "b0 s1" ROUNDS BIN ARGS...: run ~/VARIANT/BIN ARGS for each variant in
# turn, ROUNDS times, pinned to the last core (CORE overrides).
V=$1; R=$2; shift 2; B=$1; shift
C=${CORE:-$(( $(nproc) - 1 ))}
for r in $(seq "$R"); do for v in $V; do echo "== $v round $r"; taskset -c "$C" ~/"$v"/"$B" "$@"; done; done
