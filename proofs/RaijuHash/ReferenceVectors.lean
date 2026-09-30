import RaijuHash.Reference

/-!
# Frozen vectors for the transcription of `reference.rs`

`crates/raijuhash/tests/vectors.rs` pins `reference::hash` and every Rust
backend to these outputs (key bytes `i * 13 + 5`, message bytes
`i * 31 + 7`, both reduced to `u8`). Here the same inputs are run through
the Lean transcription. Up to 1024 bytes the kernel evaluates them
(`decide +kernel`, no additional axioms); the four longer messages, which
cover a full 64-block chunk and several chunks, are checked by compiled
evaluation (`#guard`), because list indexing makes kernel evaluation slow.
`check_vectors.py` checks that both files list the same numbers.
-/

namespace RaijuHash.Reference.Vectors

/-- `Params::from_bytes` of the key bytes `(i * 13 + 5) as u8`. -/
def patternKey : Params := Params.fromBytes fun i => (i * 13 + 5) % 256

/-- The message bytes `(i * 31 + 7) as u8`. -/
def patternMsg (n : ℕ) : List ℕ := (List.range n).map fun i => (i * 31 + 7) % 256

theorem pattern_0 :
    hash patternKey (patternMsg 0) = 0x584b3e3124170afdf0e3d6c9bcafa295 := by
  decide +kernel

theorem pattern_1 :
    hash patternKey (patternMsg 1) = 0x000fc20c256f04ee23135fe801794877 := by
  decide +kernel

theorem pattern_15 :
    hash patternKey (patternMsg 15) = 0xebe17fc2bb4337e85308a1d43247f806 := by
  decide +kernel

theorem pattern_16 :
    hash patternKey (patternMsg 16) = 0x580381ccfee1c42a09f636f9f6db0930 := by
  decide +kernel

theorem pattern_31 :
    hash patternKey (patternMsg 31) = 0x7b4d70fca6edb0581a539e1a0a5be81c := by
  decide +kernel

theorem pattern_32 :
    hash patternKey (patternMsg 32) = 0xb94d7ba165e7bc46267fd5e0b6e012eb := by
  decide +kernel

theorem pattern_127 :
    hash patternKey (patternMsg 127) = 0x6d1aaf8c35ae00242f969fb2c9ac2a4c := by
  decide +kernel

theorem pattern_128 :
    hash patternKey (patternMsg 128) = 0xf5413437937f6e3440c3fddd38b731e1 := by
  decide +kernel

theorem pattern_129 :
    hash patternKey (patternMsg 129) = 0x134d99c0c09f516c013b1f061675b86b := by
  decide +kernel

theorem pattern_1000 :
    hash patternKey (patternMsg 1000) = 0xc3ee0a9c543d9978dd14c2160b740767 := by
  decide +kernel

theorem pattern_1024 :
    hash patternKey (patternMsg 1024) = 0xe6821818e4b3f51319f4e6d806acf113 := by
  decide +kernel

#guard hash patternKey (patternMsg 8191) == 0x89deeac678cfd9391b1b54d045ba6fdf
#guard hash patternKey (patternMsg 8192) == 0x9e6b50b1cf5b185edef367c5708e142c
#guard hash patternKey (patternMsg 8193) == 0xc95a64f89b205ecd069afcd0c347f38d
#guard hash patternKey (patternMsg 20000) == 0x1cd9404a351f6d9d5c92c467dd0a3ac3

end RaijuHash.Reference.Vectors
