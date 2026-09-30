import Mathlib

/-! Fail closed: a successful elaboration is insufficient when a declaration
has admitted or custom axioms among its transitive dependencies. -/
open Lean Elab Command in
elab "audit_axioms " n:ident : command => do
  let name ← liftCoreM <| realizeGlobalConstNoOverloadWithInfo n
  let axioms ← Lean.collectAxioms name
  let allowed := [``propext, ``Classical.choice, ``Quot.sound]
  let rejected := axioms.filter fun ax => !allowed.contains ax
  unless rejected.isEmpty do
    throwError "{n}: unapproved axioms {rejected.toList}"
  logInfo m!"{name}: axiom audit passed"
