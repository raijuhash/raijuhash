#!/usr/bin/env python3
"""Negative integration checks for the axiom gate; no admitted test is imported.

Run after `lake build RaijuHash.AxiomAudit`. Temporary Lean modules live outside
this project, so none can contaminate its proof environment or build targets.
"""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent
CASES = {
    "valid": ("theorem sample : True := True.intro", True),
    "direct_admission": ("theorem sample : False := by sorry", False),
    "transitive_admission": (
        "theorem unfinished : False := by sorry\n"
        "theorem sample : False := unfinished", False
    ),
    "custom_axiom": ("axiom unsupported : False\ntheorem sample : False := unsupported", False),
    "native_decision": ("theorem sample : 1 + 1 = 2 := by native_decide", False),
}

for name, (declarations, should_pass) in CASES.items():
    with tempfile.TemporaryDirectory(prefix="raijuhash-axioms-") as directory:
        source = Path(directory) / "AuditProbe.lean"
        source.write_text(
            "import RaijuHash.AxiomAudit\n" + declarations + "\naudit_axioms sample\n"
        )
        result = subprocess.run(
            ["lake", "env", "lean", str(source)], cwd=ROOT,
            text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
        )
        if (result.returncode == 0) != should_pass:
            raise SystemExit(f"{name}: unexpected status {result.returncode}\n{result.stdout}")
        if not should_pass and "unapproved axioms" not in result.stdout:
            raise SystemExit(f"{name}: failed for an unrelated reason\n{result.stdout}")
        print(f"{name}: {'accepted' if should_pass else 'rejected'}", flush=True)
