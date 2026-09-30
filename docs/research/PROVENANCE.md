# Licensing and independent implementation

## Project license

RaijuHash is released under CC0 1.0, Apache License 2.0, or Apache License
2.0 with LLVM Exception, as declared in the workspace and crate manifests.
The license texts are included at the repository root and in the crate package.
Dependencies retain their own licenses.

## Implementation procedure

1. Write the mathematical specification, byte layout, contracts, and test
   requirements in new prose, with public research references.
2. Implement from that specification and independently derived constants.
3. Generate test vectors from an independently written scalar specification.
4. Maintain a source ledger containing the URL/revision, what was learned or
   imported, license, and required notices.
5. Inventory runtime, optional, build, and development dependencies separately.
   Public-API benchmark dependencies keep their own applicable terms.
6. Inspect the packaged file list and licenses before publishing. Keep the
   crate self-contained under `crates/raijuhash/` and include its license texts.

## Comparison implementations

The user confirmed that keyed universal hashing and optional MAC must remain.
The non-cryptographic entries below are benchmark references, not replacements
that satisfy that requirement.

| Project | Role | License evidence |
|---|---|---|
| xxHash / XXH3-128 | Established non-cryptographic 128-bit speed baseline | [Core library BSD-2-Clause license](https://github.com/Cyan4973/xxHash/blob/dev/LICENSE); audit bindings and CLI separately. |
| rapidhash | Established 64-bit non-cryptographic comparison; output width differs | [Original implementation MIT license](https://github.com/Nicoshev/rapidhash/blob/master/LICENSE); Rust ports have their own manifests/notices. |
| BLAKE3 | Public cryptographic integrity and keyed hashing option | [Official Rust manifest](https://github.com/BLAKE3-team/BLAKE3/blob/master/Cargo.toml) lists `CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception`. |

These are comparison implementations, not code to relabel as a new invention.
Current upstream pages were checked on 2026-09-27; pin revisions during
implementation because moving branch URLs are not a dependency lock.

## Mathematical sources for the expanded candidate portfolio

The added [candidate catalogue](CANDIDATES.md) cites EHC, integer NH/UMAC,
Multimixer-128, HalftimeHash, and polynomial hashing research. It proposes new
compositions and experiments rather than importing implementations. In
particular, a paper's publication license and the license of its accompanying
software are different records in the source ledger.

Before implementing M32 or the BRW variant, record the exact paper version,
theorems/parameter restrictions used, and any independently derived changes.
Write the scalar specification and test vectors independently. If any existing
implementation is used as a benchmark or imported as code, inventory that
artifact's actual license and notices; the project license does not erase
them. Mark the new composition proofs separately from the published results.
