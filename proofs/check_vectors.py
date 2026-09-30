#!/usr/bin/env python3
"""Check that RaijuHash/ReferenceVectors.lean pins the same outputs as the
Rust test file crates/raijuhash/tests/vectors.rs (PATTERN_KEY), so the
kernel-checked Lean vectors and the Rust backends agree on one list."""
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parent
rust = (ROOT.parent / "crates/raijuhash/tests/vectors.rs").read_text()
block = rust[rust.index("const PATTERN_KEY"):]
block = block[: block.index("];")]
expected = {int(n): int(h, 16) for n, h in re.findall(r"\((\d+),\s*0x([0-9a-f]+)\)", block)}

lean = (ROOT / "RaijuHash/ReferenceVectors.lean").read_text()
found = {}
for n, h in re.findall(r"hash patternKey \(patternMsg (\d+)\) = 0x([0-9a-f]+)", lean):
    found[int(n)] = int(h, 16)
for n, h in re.findall(r"#guard hash patternKey \(patternMsg (\d+)\) == 0x([0-9a-f]+)", lean):
    found[int(n)] = int(h, 16)

if not expected:
    raise SystemExit("no PATTERN_KEY vectors found in tests/vectors.rs")
if found != expected:
    missing = sorted(set(expected) - set(found))
    extra = sorted(set(found) - set(expected))
    wrong = sorted(n for n in set(found) & set(expected) if found[n] != expected[n])
    raise SystemExit(f"vector mismatch: missing {missing}, extra {extra}, different {wrong}")
print(f"{len(found)} vectors match tests/vectors.rs")
