# x86 candidate work: progress log

This file is where the x86 work picks up each session. Update it as items
finish. Background and every earlier measurement:
[CANDIDATES.md §10](docs/research/CANDIDATES.md#10-x86-experiments-2026-09-28),
[X86_PORT.md](docs/research/X86_PORT.md).

## Box

- AWS spot instance (AMD EPYC 9R45, Zen 5), user `ubuntu`, created by
  `terraform/`. It can be reclaimed at any time; when that happens the user
  recreates it and supplies the new IP.
- Current (2026-09-29, round 3): `c8a.large` (2 cores, L2 1 MiB per core,
  L3 8 MiB), eu-south-2b, IP `18.100.119.101`; pin with `taskset -c 1`.
  (Round 3 started on a `c8a.xlarge`, `89.60.37.11`, reclaimed.) Earlier rounds used
  `c8a.2xlarge` (8 cores, L3 32 MiB): single-core results up to L2 sizes
  compare, L3-sized ones (1–16 MiB) and `par` with more than 4 threads do not.
- Sync (never copy `proofs/`, `target/`, `terraform/`, `results/`, `.git/`,
  `.artifacts/`):
  `rsync -az --exclude target/ --exclude terraform/ --exclude proofs/ --exclude results/ --exclude .git/ --exclude .artifacts/ -e 'ssh -o StrictHostKeyChecking=accept-new' ./ ubuntu@IP:raijuhash/`
- New box: wait for `cloud-init status --wait`, sync the working tree to
  `~/raijuhash` and the baseline (`git archive HEAD | tar -x -C DIR`, without
  `proofs/`, `results/`, `terraform/`) to `~/rb`, then run
  `~/raijuhash/experiments/box/setup.sh`: it installs `~/snap.sh` and `~/ab.sh`
  and builds `~/b0` (baseline) and `~/s0` (working tree). In zsh, write the
  host out in full in rsync targets (`$H:r...` is a history modifier).
- `~/snap.sh DIR [SRC] [g]` builds SRC (default `~/raijuhash`) natively, or
  generic with `g`, and copies the binaries to `~/DIR`; `~/ab.sh "b0 s1"
  ROUNDS BIN ARGS` alternates variants, pinned to the last core.
  `perf` is installed by `apt-get install linux-tools-$(uname -r)`.
- Hosts differ in clock speed, so compare builds on one host only, alternating
  runs and taking medians.

## Where to start next time

- **Wrap-up for publication (2026-09-29, after round 3).** `results/` was
  removed (research docs now say so and link to the new
  [docs/BENCHMARKS.md](docs/BENCHMARKS.md)). That file compares RaijuHash
  with POLYVAL, GHASH, Poly1305 and UMASH on the M1 and on the
  EPYC 9R45 (harness `benchmarks/src/axu.rs`). README rewritten; crate
  manifest prepared for crates.io (root README as readme, `include` list,
  licenses linked into the crate, MSRV 1.95 tested on both architectures);
  `cargo publish --dry-run -p raijuhash` passes on both. Nothing committed.
- **Review fixes (2026-09-29).** Two undefined-behavior reports were
  confirmed and fixed, outputs and speed unchanged:
  - `Key::from_seed` built a `Key` whose lazy table rows were uninitialized
    `u8`s. Miri reported "constructing invalid value ... at
    `.table.value[1152]`". `table` is now
    `UnsafeCell<MaybeUninit<[u8; TABLE_BYTES]>>`.
  - AVX-512 formed out-of-bounds `ptr::add` addresses for empty-mask tail
    loads (`fin`, `small`) and for the bulk prefetch; these now use
    `wrapping_add`.

  A Miri run of the portable backend (`x86_64-unknown-linux-gnu`) over
  lengths 0–300 and 1–9 KiB, streaming, clone and `hash_parallel` finds
  nothing else. Miri cannot run the SIMD kernels.
- **Second review (2026-09-29).**
  - AVX-512 `finish` computed `BLOCK * n.wrapping_sub(1)` with `n = 0`,
    which overflows. In debug builds the tests panicked at 8192 bytes
    ("attempt to multiply with overflow"). x86 tests had only been run in
    release. It is now `n.saturating_sub(1)`, as in the other backends.
    Debug and release tests pass on x86 (native, generic) and on the M1.
  - SPEC §6.5 and `hash_avalanche` docs: "collisions are those of `H`"
    holds only for `V != 0`. Fixed.
  - Run the tests in debug builds on x86 too.
- **Lint state:** `cargo clippy -p raijuhash --all-targets` gives 6 style
  warnings and no errors, the same on aarch64 and x86. `cargo fmt --check`
  fails: rustfmt was never applied and there is no `rustfmt.toml`. Defaults
  would change about 1,090 lines of the crate; `max_width = 120` with
  `use_small_heuristics = "Max"` would change 238.

- Round 3 (2026-09-29, `c8a.large`): items 34–41, ended at the user's
  request. Adopted: item 36 only (setup, `x86/aes.rs` and `cipher.rs`,
  uncommitted); everything else is reverted. **Decision needed: item 38**,
  the page-crossing cliff of masked loads (up to 50× on messages that end
  just before an untouched or unmapped page) against a guard that costs
  0–3% in its own kernels but more where it changes the caller's inlining;
  the guard is `experiments/patches/page-guard.diff`. New harnesses:
  `wrap` (a non-inlined wrapper; the steadiest for library-only changes),
  `pageend` (the cliff), `setup` with `KEY_LOOP=n` (for `perf`).
- Harness caution (round 3): `quick`'s one-shot column and `frag` move by
  5–38% when a change alters the size of the inlined `Key::hash`/`update`
  (their closures stop being inlined), and `quick`'s streamed column shows
  18–28 ns instead of 3.8–17 ns in some process layouts even for b0 (page-
  aligned data; about 1 run in 5). Check library changes with `wrap`, `lat`
  and `mixlen` before `quick`.

- State at the end of 2026-09-29: items 1–33 below are resolved (only item
  11 is blocked), and every
  adopted change is uncommitted in the working tree (`crates/raijuhash/src/lib.rs`,
  `x86/mod.rs`, `x86/avx512.rs`). Tests pass on the box in all configurations.
  The write-up is in `results/amd-epyc-9r45-round2-2026-09-29.md` and
  CANDIDATES §10.7.
- Tools added this round: `experiments/src/bin/mixlen.rs` (random lengths),
  `setup.rs` (constructor copies), `aesmix.rs`, and IFMA/VAES probes in
  `xport.rs`. `lat`/`mixlen` accept `RJ_BACKEND=sse|avx2`.
- Open, in order of expected value:
  1. Measure on Intel AVX-512 (Sapphire Rapids or later: `c7i`/`c8i`) and an
     AVX2-only CPU. This needs another instance, which is the user's call.
     AVX2's streamed 65–300 B stall (item 6d) should be revisited there.
  2. API decisions: the batch API (X13, now 1.9× hash / 2.7× MAC), a
     `Box<Key>` constructor (saves the remaining 9 KB copy of
     `Box::new(Key::from_seed)`), a persistent worker pool (X14: the
     prototype `pool.rs` reached 150 GB/s at 256 KiB with two threads,
     where `hash_parallel` must now stay serial).
  3. A v2 short/medium branch (NH over GF(2^128), §10.3a) now pays mainly at
     65–255 B (1.2–1.5×); it needs a new spec and a Lean proof.
  4. Smaller ideas, assessed on paper and not built:
     - A branchless 1–64 B form (both paths, then select). Random 1–64 B
       would go from 7.2 to about 4.6 ns (10 clmul issues), but fixed
       1–31 B from 2.7 to about 4.6 and 32–64 B from 3.6 to 4.6: a bad
       trade for fixed-length traffic. Unifying the two forms instead is a
       v2 and would weaken the 1–31 B bound from 1/Q to 2/Q.
     - Eager absorption of whole blocks in `Hasher::update`: streamed
       65 B–1 KiB costs 2–5 ns more than one-shot on AVX-512, but `Core`
       assumes group-aligned positions in every backend's `fin`. Too
       invasive for the gain.
     - The NEON port of the `x^64` single-fold form: not x86, but the same
       algebra.
     - Many-key MAC layout. With 4096 keys, a 16 B tag goes from 5.8 to
       9.0 ns. The AES round keys live in the `Aes` enum, whose software
       variant is the largest, so they cannot be placed beside the hash key's
       hot lines without restructuring `Mac` and `Aes`. Not built.
     - Random 65–256 B costs about 4 ns more than fixed lengths (the
       `small_n` jump table on the block count). A runtime-count loop would
       mispredict at its exit instead, so there is no gain without doing the
       work for 8 blocks every time.

## Already done (2026-09-28; details in CANDIDATES §10)

Hybrid carryless + Multimixer (+7.5% hot, 0–4% from L2, loses beyond L3:
rejected), integer-only Multimixer (0.89×), G64 close (latency only),
NH-over-GF(2^128) short/medium family (1.1–1.85× below 256 B; needs a v2),
integer EHC (analysis: needs distance 5; not better), X1, X2, X3, X6, X7
(one-group entry), X9 (analysis), X10 (profile only), X12, X13 (batch API
3–4×, API decision), X14 (adopted hardware combine; pool prototype), X15, X17
(adopted `avx512::group`), X18 (M1 only; the Zen 5 run was lost).

## Queue for this round (2026-09-29)

Status: `todo`, `running`, `done`, `rejected`, `blocked` (needs the user).

| # | Item | Status | Result |
|---|---|---|---|
| 1 | Baseline on the new host: tests, `quick` and `compare --setup` | done | 4.46 GHz host; matches the first host (64 KiB 129.8 GB/s, setup 102 ns) |
| 2 | X10: remove the 9.3 KB `Key` copy out of the constructors without an API change | done, **adopted** | `expand_into`/`init_into` (out of line, `&mut MaybeUninit<Key>`): LLVM passes the caller's return slot through. `compare --setup` key 102 → 65 ns, MAC 116 → 79 ns, `one setup` 137 → 103 ns. Tests pass. `Box::new(Key::from_seed)` still copies once (155 → 118 ns); a `Box<Key>` constructor would be an API decision |
| 3 | X18 on Zen 5: rerun `pclmul.rs` (lost last session) | rejected | five bit classes 7.88 ns vs current Karatsuba 4.11 ns per 64×64 product (0.52×); portable bulk 4.5 GB/s. Every relevant x86 CPU has PCLMUL anyway |
| 4 | X4/X1: 1–31 B and 32–64 B paths | done, **adopted** | (a) Key gains `sk = (A, B, x^64 A, x^64 B)` (x86 only, cheap shifts at setup): `x A = (x.lo A.lo + x.hi A'.lo) + (x.lo A.hi + x.hi A'.hi) x^64` needs one fold clmul instead of the two-step reduction. SSE/AVX2 `x86::short` use it: store-then-hash latency −13 to −15%, 1–15 B throughput −5 to −10%, 16–31 B equal. (b) AVX-512 `short_msg`: `[X0 \| X1]` as one 256-bit masked load with the length byte merged in, four ymm clmuls plus one fold, no length branches: 1–31 B one-shot 4.5–6.0 → **3.8 ns**, streamed 6.4–8.1 → 4.7 ns; **random lengths 1–31 B 11.65 → 3.80 ns (3.1×)**, 1–64 B 11.7 → 8.1 ns; MAC over random 1–31 B 13.4 → 7.1 ns; store-then-hash latency 10.7–11.7 → 9.7 ns. (c) AVX-512 `medium` (32–64 B): `L T` with one fold (3 clmuls, not 4), XORed in before `h`: 4.46 → 4.01 ns, latency −1%, MAC −2%. Earlier "X1 packed short path" lost latency; this one does not. Tests pass; outputs unchanged |
| 5 | X16/X3: GFNI or shift fold in the new short path | rejected | Shift fold instead of the one fold clmul in `short_msg`: throughput −6% (3.57 → 3.34 ns) but store-then-hash latency +3%, dependent chain +4%, MAC throughput +4% (AES competes for the vector ALU slots the shifts use). GFNI affine ops are byte-local, so the fold would need two of them plus a byte shift: more latency than shifts; not built |
| 6 | X7: AVX2 backend structure (forced on Zen 5 as a proxy) | done, **adopted (a)–(c)** | (a) **Adopted:** branchless AVX2 short path `x86::short_avx2` (dword-masked `vpmaskmovd` load, last `len % 4` bytes by three clamped byte loads, compare-mask placement of tail and length byte, then the ymm math of `short_msg`): random 1–31 B 11.7 → **5.14 ns (2.2×)**, fixed 16–31 B 5.1–5.8 → 4.46 ns; fixed 1–15 B 4.46 (S1: 4.0–4.9); store-then-hash latency 10.4 ns flat (baseline 9.8–11.9, S1 8.1–10.3). A trade against S1 on fixed short lengths, a large win on mixed ones. SSE keeps S1 (no masked loads at all). (b) **Adopted:** AVX2 padded last block in whole 32-byte stores (`padded_block32`): the old 16-byte pieces made every 32-byte load of it fail store forwarding. One-shot 65 B 24.0 → 12.8 ns, 96 B 24.0 → 10.7, 127 B 23.9 → 11.8, 192 B 23.8 → 15.2, 255 B 24.6 → 16.3, 300 B 24.0 → 19.0. (c) **Adopted:** generic `small`/`fin` closes sum the planes with `shifted_sum` and do one `R2` product, instead of one product by `x^b R2` per plane (up to 24 xmm clmuls at 1 KiB); `shifted_sum` folds its ≤ 5 overflow bits with shifts instead of a clmul; one position keeps its direct product; plain index loops (an `enumerate().take().skip()` over vectors was left out of line and cost 4 ns). AVX2 one-shot vs (b): 255–256 B −6%, 512 B −14%, 1 KiB −21%, 2000 B −11%, 4095 B −9%, 65–128 B +3%; SSE: 65 B −31%, 512 B −11%, 1 KiB −15%, 4095 B −5%, 128 B +5%. AVX-512 unchanged. (d) **Rejected:** AVX2 hasher buffer filled with whole 32-byte stores (`copy_group32`) for the streamed stall: AVX2 65 B streamed 23 → 17 ns, but adding the branch to the inlined `update` made AVX-512 streamed 1–64 B 10–15% slower (identical AVX-512 code; caller codegen), and the forced-AVX2 streamed numbers depend on the memcpy glibc picks on this AVX-512 host, so they do not represent an AVX2-only CPU. Streamed AVX2 65–300 B remains 17–29 ns (open; measure on real AVX2 hardware) |
| 7 | X5: quadratic form for 65–128 B | rejected (operation count) | Its X-row linear terms alone need 8 lanes × two 64-bit products = four zmm clmuls, which equals the whole current two-position body; the Y terms, `clmul(X, Y) R2` and the reduction come on top |
| 8 | R3: IFMA (`vpmadd52luq`) port probe next to clmul | rejected | Probe (`xport`): IFMA about 2 per cycle, no conflict with clmul (4 clmul + 8 IFMA = 8.0 cycles). But for 32-bit NH/Multimixer products `madd52lo`+`madd52hi` are two ops, the same as `vpmuludq`+`vpaddq`, and every vector op costs one of Zen 5's four ALU slots, the hybrid's limit. A 52-bit-limb prime-field hash needs about 1 op/B against v1's 0.07 |
| 9 | R6: VAES port probe | done | `vaesenc` zmm 2 per cycle (128 B of AES round per cycle), and **4 clmul + 16 vaesenc = 8.0 cycles: VAES does not compete with the carryless multiply at all**, unlike the adds/ternlogs the Multimixer hybrid needed (8 vaesenc + 8 ternlog = 4.5 cycles: some conflict with the ALU). Leads to item 12 |
| 12 | AES-PRF + carryless hybrid (new; an R6 relative) | rejected | Bare loop `experiments/src/bin/aesmix.rs`, hot, per 1 KiB group of eight v1 chain blocks plus `A` 64-byte `AES-128(M + K)` vectors: A = 0: 31.8 B/cycle, **A = 2: 34.4 (+8%, the best)**, 4: 31.7, 8: 28.3; AES alone 11.8 B/cycle. VAES coexists with clmul alone but competes with the chain's ternlogs/XORs for the vector pipes. +8% bare would shrink after the close and L2 streaming (the Multimixer hybrid went from +13% bare to 0–4% from L2), and it needs a weaker contract. Original idea: part of each chunk through v1's chain, part through `sum_i AES_K(M_i + K_i)` (full 10-round AES, PMAC-like) on the otherwise idle AES units; up to about 32 + 12.8 = 45 B/cycle before overheads. **Contract caveat:** its bound adds the AES PRP advantage for every key (v1: only seeded keys), so it conflicts with §9.1's uniform-key theorem; a user decision. Measure the body first |
| 10 | X11: compact seeded key (API decision; prototype only) | todo | |
| 11 | Intel AVX-512 / AVX2-only CPUs | blocked | needs another instance, which is the user's call |
| 13 | AVX-512 packed close `mix` with `x^64`-prepared keys | done, **adopted** | `Key::pk` now holds `(R, T, R2, 0)` and `x^64 (R, T, R2, 0)` (was the swapped halves): four product clmuls as before, then one fold of `mid`'s top word (a clmul in `small`, shifts in the bulk close) instead of `reduce_mul`'s two serial clmuls or `reduce`'s long shift sequence. `reduce`/`reduce_mul` removed. One-shot 65–128 B 6.0 → 5.4 ns (−10%), 256 B 7.5 → 6.8, 512 B 8.8 → 8.2, 1 KiB 12.3 → 11.8, 64 KiB 129.7 → 131.7 GB/s, 256 KiB 131.0 → 133.0 GB/s (+1.5%, shorter chunk close); store-then-hash latency 65 B–1 KiB −8%; MAC −4 to −6% |
| 15 | Avalanche multiply with `x^64 V` prepared (`Key::vk`, `x86::mul_x64`) | done, **adopted** | `finalize_avalanche` 0 B 7.2 → 6.8 ns, 16 B 9.8 → 9.0, 64 B 11.0 → 10.0, 256 B 17.5 → 16.3, 1 KiB 25.5 → 24.0 (−5 to −9%) |
| 16 | Streamed AVX2 short fix, and `short` inlining | done, **adopted** | `Key::short::<STORED>`: `Hasher::finalize` skips AVX2's masked-load path (it cannot forward from `update`'s stores: streamed 1–16 B had become 12.3 ns, now 6.1–7.1). The generic narrow-load path went out of line (`short_narrow`), so the vector paths inline without its seven register saves, and `Key::hash` is `#[inline]`. AVX-512 1–31 B: throughput 3.57 → **2.68 ns**, dependent latency 3.79 → 2.45 ns, random 1–31 B 3.81 → 2.90 ns, MAC 7.34 → 5.59 ns (−24%), MAC 65–128 B 11.5 → 10.0, 1 KiB 18.9 → 17.7; AVX2 one-shot 1–31 B 4.7 → 3.3 ns. Costs: 65–128 B +3% (5.40 → 5.58), AVX-512 256 B–2 KiB +1–2%, SSE 128 B–4 KiB +2–4% |
| 17 | AVX-512 32–64 B: `L T` from one ymm clmul | done, **adopted** | `L` broadcast times `key.t` = `(T, T swapped)` gives `L T.lo` and `L T.hi` in one 256-bit multiply; the ≤ 7-bit overflow folds with shifts (was three xmm clmuls). 32–64 B 4.02 → **3.57 ns** (−11%), random 32–64 B 4.25 → 3.81; latency and MAC unchanged |
| 19 | AVX2 32–64 B with 256-bit clmuls (`x86::medium_avx2`) | done, **adopted** (AVX-512 checked: neutral within ±2% in `lat`; M1 NEON unchanged) | The affine form with two X word pairs per ymm: 8 ymm + 3 xmm clmuls instead of 19 xmm. AVX2 one-shot 32–64 B 8.7 → **4.9 ns** (−44%). `Hasher::finalize` keeps the 128-bit form (`Key::medium_stored`): LLVM merges the two 16-byte loads into one 32-byte load, which does not forward from `update`'s 16-byte stores (streamed 9.6 → 12.0 ns before the split). Making `medium` const-generic instead (like `short`) changed `Key::hash`'s inlining: AVX-512 65 B 5.4 → 7.2 ns, so two plain functions. AVX-512 in `quick`: 65–128 B 5.4 → 5.6 ns (to check with `lat`) |
| 20 | Branchless short path for the SSE backend on AVX2 CPUs (`x86::short_avx2_x`) | done, **adopted** | The SSE backend is what CPUs with AVX2 but without VPCLMULQDQ run (Haswell to Coffee Lake, Zen 1/2): the same dword-masked loads as `short_avx2` (`short_row_avx2`), then 128-bit `Wide64` products. `Key::avx2` (runtime detection with `std`) selects it; `Hasher::finalize` keeps the narrow loads. Forced SSE on Zen 5: random 1–31 B 11.46 → **5.35 ns** (2.1×), random 1–64 B 14.0 → 11.2; fixed 1–31 B 4.0–5.8 → 4.69 flat (+16% at 1–15 B, −8 to −19% at 16–31 B); store latency 9.1–10.5 → 10.5; dependent chain 10.1 → 11.4. The same trade as item 6a. Cost elsewhere: AVX2 random 1–31 B 3.59 → 3.81 (the extra dispatch arm); AVX-512 unchanged. Tests pass on the box (with and without `target-cpu=native`) and on the M1 |
| 21 | X7: VEX-encoded SSE kernels (the SSE module compiled with `avx,avx2`) for AVX2 CPUs without VPCLMULQDQ | rejected | Generic builds (no `target-cpu=native`), forced SSE on Zen 5: identical at 16 B–64 KiB (for example 1 KiB 45.5/45.5 ns, 64 KiB 2291/2292 ns); the kernels are carryless-multiply bound. The native build's SSE backend is 18% faster in bulk (1946 against 2294 ns at 64 KiB) only because it gets AVX-512's `vpternlogq` and 32 registers, which those CPUs lack |
| 22 | X11 without an API change: key field layout for many active keys | done, **adopted** | Probe [`experiments/src/bin/keys.rs`](experiments/src/bin/keys.rs): one message under `n` keys in shuffled order. At 4096 keys (38 MB, beyond L3) a 16 B hash went 2.89 → 4.03 ns, 48 B 3.78 → 7.18, 128 B 5.6 → 9.7, 1 KiB 12.3 → 24.1. Reordered `Key` so that `sk`, a line with `backend`/`avx2`/`s`/`t`, and `s2` are adjacent: 1–31 B touches 2 lines instead of 3, 32–64 B 4 instead of 5. 4096 keys: 16 B 4.03 → **3.45 ns**, 48 B 7.18 → **5.59**; 1024 keys: 48 B 4.2 → 4.03, 128 B 7.0 → 6.45. Single-key times unchanged; MAC with many keys unchanged (±2–3%) |
| 23 | Setup: a seeded key's 72 row blocks and 7 field blocks in one AES batch (`x86::aes::eager_wide`, `Aes::ctr2`) | done, **adopted** | Before, two `ctr_wide` calls: 18 vectors, then a 4-block vector plus 3 blocks through the 128-bit loop, each waiting out the ten-round latency. One batch of 20 vectors with a masked store for the last 3 blocks: `Key::from_seed` 67 → **60 ns**, `Mac::from_seed` 81 → **75 ns**. Frozen vectors pass. (`one setup` also shows a harness `memcpy`: its `black_box` passes the 9 KB key by value) |
| 24 | Key schedule with `pshufb` + `aesenclast` instead of `aeskeygenassist` | rejected | Same round keys, setup 60/75 ns either way on Zen 5 |
| 25 | Only two eager rows (for `s2_prepare`), the first group's rows generated by the first 65 B–1 KiB message | rejected (trade-off) | Setup 60 → 49 ns, MAC 75 → 61; fresh key + hash of 0–64 B 64–71 → 53–58 ns (−17%), but fresh key + hash of 65 B–1 KiB 76–84 → 87–99 ns (+13–18%) even with a single 14-vector batch for the lazy rows: the lazy path rebuilds the AES schedule and takes the lock. Storing the 176-byte schedule in every key would be needed to make it a win. Probe: fresh-key section of `setup.rs` |
| 26 | MAC cipher-key block in the eager batch's spare lane | rejected | Key setup 60 → 64 ns, MAC 75 → 77: the 20-vector batch already holds 31 of 32 zmm registers, and the extra lane's live values make it spill |
| 27 | X6: fragmented streaming, measured | done | Probe [`experiments/src/bin/frag.rs`](experiments/src/bin/frag.rs) (`PIECE=n`, `ONLY_PIECES`, `ITERS` for `perf`). AVX-512, 64 KiB: one `update` 505 ns; in 1 KiB pieces 1030–1170 ns, in 128 B pieces 2.1 µs, in 16 B pieces 7.3 µs (about 1.8 ns per `update` call at 1–16 B). 1 KiB pieces are instruction-bound: `perf` shows 3.9× the instructions of one update (294 per KiB against 75), 80% in `avx512::groups`, whose per-call fold (planes, `shifted_sum`, lane fold) and state load/store are as large as the eight blocks' work. No spills |
| 28 | AVX-512 streaming state kept per lane (`h0`, `h1` as four-lane vectors in words 0..8 of `Core`) | done, **adopted** | Skips the lane fold per `groups` call (the close folds anyway). 4 KiB in 1000 B pieces 111–123 → 100 ns, 64 KiB in 1 KiB pieces −3%, 127 B pieces −3 to −9%; whole-message and one-shot times unchanged |
| 29 | Absorb a group-completing `update` directly unless it is the message's first group | done, **adopted** (decided in `update_long`) | Buffering (and copying) every exact group only helps a message's first group. The rule inside the inlined `update` gave 64 KiB in 1 KiB pieces 1163 → 1050 ns but made 16 B pieces 7.3 → 9.0 µs (codegen of the inlined path); moved into `update_long` (`update` now buffers only `total < GROUP`): 1 KiB pieces 1050–1063 ns and 16 B pieces 7.3 µs, i.e. both. AVX2 neutral (1 KiB −3.5%, 16–128 B +2–3%) |
| 30 | X14 without an API change: `hash_parallel` thread use | done, **adopted** | The calling thread now takes tasks too (one spawn fewer), and at most one thread per 2 MiB of input is used ("up to `threads`" allows it; `PAR_MIN_BYTES`). GB/s for 2 / 4 / 8 threads requested, before → after: 1 MiB 55/35/21 → 124 (serial), 2 MiB 91/64/39 → 115 (serial), 4 MiB 123/135/78 → 147/148/152, 8 MiB 145/188/125 → 168/206/202, 16 MiB 157/238/215 → 167/244/271; 64 MiB memory-bound (66–82) either way. 1 MiB per thread was worse at 1–2 MiB, 4 MiB worse at 8 MiB. `parallel_matches_serial` gained 4 MiB and 6 MiB inputs so that 2 and 3 threads really run |
| 31 | `hash_parallel` task size 128 → 256 KiB (`TASK_CHUNKS` 32) | done, **adopted** | Two alternating runs each: 4 MiB 142–147 → 149–162 GB/s, 8 MiB with two threads 167–169 → 177–184, 16 MiB 252–270 → 265–278, 64 MiB +2–4%. 512 KiB tasks lost at 4 MiB (131–143). Final: 4 MiB 155, 8 MiB 174/212/212, 16 MiB 174/259/275 GB/s for 2/4/8 threads |
| 32 | X15: software prefetch for DRAM-sized inputs (beyond 32 MiB) | rejected (re-confirmed) | 4 KiB ahead: 64 MiB 54.4–54.8 → 47.3–48.4 GB/s, 256 MiB 47 → 42; 16 KiB ahead worse (45.6, 37.8), and worse at 16 MiB too. The hardware prefetcher alone is best from memory, as found before |
| 33 | `length_only` (a message ending at a chunk boundary) with `Wide64` | done, **adopted** | `L T` with one fold: 8 KiB one-shot 70.1 → 69.6 ns, 16 KiB 131.1 → 130.5; others unchanged |
| 18 | Out-of-line 32-byte-store buffer copy for AVX2 (`copy_long` in the long branch of the hasher's copy) | rejected | AVX2 streamed 65 B 23.4 → 17.0 ns, but streamed 64 B–2 KiB 4–19% slower on AVX2 and SSE 64 B +14% (the extra call level, and the copy loop against `memcpy`); AVX-512 unchanged. Reverted |
| 14 | Same for the SSE/AVX2 closes (`Wide64`) | done, **adopted** | `outer_step`, `finish`, `small`, `fin` use `Wide64` (products with `pk`'s `x^64` keys, one fold): store-then-hash latency 65–256 B −3 to −11% (AVX2 65 B 19.3 → 17.9 ns, SSE 128 B 15.9 → 14.1), neutral above; throughput within ±4% |
| 34 | Round 3. AVX-512 1–31 B with two 512-bit multiplies (3 multiply issues instead of 5) | rejected (trade-off) | Only word 0 of each lane is multiplied, so `[X0 \| X1 \| X0 >> 64 \| X1 >> 64]` against `sk` as one zmm gives all eight products in two `vpclmulqdq` zmm, plus one zmm fold. Variants (b0 = current): s1 `vpermq` to build the vector, s2 a second masked load at `msg - 24` instead, s3/s4 the same with the four lanes reduced by three `vextracti32x4` in one level. 1–31 B throughput 2.67 → 2.23 (s1, s2) / 2.45 (s3, s4) ns; random 1–31 B 2.89 → 2.46 (s1) / 2.69–2.78; **store-then-hash latency 9.73 → 11.38 / 10.65 / 10.22 / 10.96**, mixlen dependent chain 9.54 → 11.26 / 10.98 / 10.66 / 10.47; MAC throughput 5.57 → 5.62–5.85. Four lanes need one more reduction level than two, so every form pays latency for throughput, like X1 and item 5. s1 is the option if throughput alone counted (−16%) |
| 35 | Streamed 1 B–1 KiB: unmasked loads from the hasher's buffer (mask applied in a register), so that they forward from `copy_group`'s whole-vector stores | rejected | Prototype for all three stored paths (s5): only 32–64 B moved (streamed 4.9 → 4.7, 5.4 → 5.1 ns); 1–31 B and 65 B–1 KiB unchanged, so masked-load forwarding is not where the streamed overhead goes (it is instruction count in `hasher`/`update`/`finalize`). A sound 32–64 B-only version (s7: `buf_len > 0` implies `copy_group` initialized the first 64 bytes) gave one-`update` 32–48 B 4.9 → 4.2 ns, 64 B 5.4 → 5.0 (`quick`), `frag` one update −10%; but the same message in 16 B pieces got **+10%** on AVX-512 at 48–64 B (16.4–18.5 → 18.0–20.4 ns: a plain load assembled from four stores fails forwarding at a higher cost than the masked load), and forced AVX2/SSE streamed 64 B +0.7–0.9 ns. Out of line (s8) lost the gain. Telling the cases apart needs a flag stored by every `update` |
| 36 | Setup: the field blocks' AES ahead of the rows' | done, **adopted** (s12) | `perf` of `Key::from_seed` (b0): 291 cycles and 842 instructions per key; the largest stall (29% of samples) is the first use of the field elements, which came out of the same round-interleaved 20-vector batch as the 72 row blocks, i.e. after all 200 `vaesenc`. `eager_wide` now runs the two field vectors' ten rounds first in program order, then the 18 row vectors: oldest-first issue lets the field chain run at its latency. `setup` harness, two alternating runs: `Key::from_seed` 65.3 → **62.7 ns**, `Mac::from_seed` 76.2–76.5 → **72.5–72.7**, `Box::new(Key::from_seed)` 114.3 → 108.2, fresh key + hash of 16 B 68.5 → 65.3, 65 B 76.4 → 72.5, 1 KiB 83.6 → 80.9, fresh MAC + tag −4 to −5%. Rejected on the way: two AES calls (fields, their field setup, then rows: 59 → 71–73 ns; masked stores of the field blocks broke forwarding into the scalar loads, and even unmasked the second call cost 135 instructions); the field setup as a closure between the two parts inside one VAES function (s13: 66–67 ns) |
| 37 | AVX-512 65–256 B without the block-count jump (`small12`: both blocks by masked loads, the third position masked to zero for one block) | rejected (trade-off) | Same outputs (a zero second block's position with the first is `small::<1>`'s end position). Random 65–256 B 10.4 → **7.65 ns** (−26%), random 1–256 B −20%, 65–1024 B −10%; but fixed 65–128 B 5.55 → **7.25** (+31%: one more position, two zmm multiplies), 129–256 B +7%, dependent chain +1–3 ns |
| 38 | **Page-crossing masked loads (new finding)** and a guard against them | **decision for the user**; patch saved | Probe [`pageend.rs`](experiments/src/bin/pageend.rs): a message ending shortly before a page that is inaccessible **or mapped but never touched** makes the masked load of its last partial vector take a microcode assist on Zen 5: `Key::hash` 2.7–12 ns → **120–145 ns**, streamed 6–19 → 135–160 ns, on every call (masked-off bytes never fault the page in). No penalty when the next page is present. It hits AVX-512 at 1 B–any length (the last partial vector: `short_msg`, `medium`, `group`, `tail`, `finish`, and `copy_group` in `update`) and AVX2/SSE only at 1–31 B one-shot (`vpmaskmovd`). It happens in practice: `lat`'s 65-byte `Vec` at the top of a fresh heap measured 143 ns per hash on this host (17.8 on the previous one). Guard (s33, [`experiments/patches/page-guard.diff`](experiments/patches/page-guard.diff), applies to the round-3 tree): `x86::crosses_page` (last message byte and last byte read in different pages), a two-level test for the group/tail/finish kernels (`near_page_end`, then the precise one in a cold function), the short path falling through to `short_narrow`, cold copies otherwise, and a `Key::x86_avx512` flag tested before the `match` on `backend` (LLVM orders that `match` by value and merged an explicit `backend` test into it; the flag alone gains nothing, but it absorbs the check on 1–64 B). Cliff cases then take 4–35 ns. Costs against b0, `wrap` (a non-inlined wrapper, the steadiest harness): 1–64 B **0**, 65–128 B +3%, 256 B +1.5%, 1–4 KiB 0–1%; random positions 32–64 B +3%, 65–256 B **+8%**, 1–1024 B +3% (1–2% of random positions really do cross and take a copy although the next page is present here); `lat` 65 B +2%, MAC 65 B +9%; `frag` 4 KiB in 16 B pieces **+21%** and `quick` one-shot 65 B–1 KiB **+30–38%**: any guard variant made the inlined `Key::hash`/`update` large enough that the harness closure stopped being inlined into its timing loop (checked by symbol; `#[inline(never)]` on `medium`/`group`, no extra call site in `short`: still not inlined), which user code could hit too. A copy of only the last block (≤ 128 B) instead of the whole group would cut the random-position cost |
| 39 | Prefetch window from the detected L3 size (this `c8a.large` reports 8 MiB, the window assumes 32 MiB) | rejected (policy confirmed) | Prefetch off (s34) against the current window, `quick`, two runs: 1 MiB equal, 2 MiB −1%, 4 MiB −3%, 8 MiB **−4%**, 16 MiB −2%, 32 MiB −2%, 64 MiB equal (no prefetch in either). The window still helps at 8–32 MiB: the VM sees 8 MiB but uses more of the CCD's 32 MiB (16 MiB still runs at 104 GB/s) |
| 40 | Backend dispatch order: AVX-512 behind two compares (LLVM tests `match` arms in discriminant order, SSE = 3 first) | rejected (mixed) | `wrap`, generic build (what most users compile) against native: 1–31 B 3.11 vs 2.67 ns, 65 B 5.80 vs 5.29, 32–64 B equal. (a) A `Key::x86_avx512` flag tested first in `short` and `dispatch!` (s26/g26): generic AVX-512 1–31 B 3.11 → **2.89**, 65 B −4%, 1 KiB −2%, MAC −6%; native AVX-512 unchanged; forced AVX2 (native build) **+3–6.5%** (16 B 3.55 → 3.78, 256 B +3.7%). (b) `X86Avx512` declared before the other x86 variants (s35): generic random 1–31 B and 32–64 B −5–6%, fixed lengths unchanged; forced SSE +2–4% at 256 B–4 KiB (now tested last). Each moves cost between backends; the AVX2/SSE figures are proxies on Zen 5 |
| 41 | `Mac::from_seed`: one-block `ctr` fast path for the cipher key (the xmm loop encrypts eight), and the cipher's schedule before `Key::expand` | rejected, reverted | `setup`: one-block path alone 72.5 → 72.1–72.2 ns (noise); with `cipher` computed first, 72.5 → **88.6–89.9 ns** (the `Mac` literal no longer receives the key in place: the 9.5 KB copy of X10 returns). Building the `Mac` in place (a `MaybeUninit<Mac>`, `cipher` first, then `expand_into` on the `key` field) was the next step, not built. Stopped here at the user's request |

## Log

- 2026-09-29: new box up, synced, workspace builds.
- 2026-09-29: X10 copy removed (item 2). Probe: `experiments/src/bin/setup.rs`.
- 2026-09-29: new harness `experiments/src/bin/mixlen.rs` (random lengths, 2^18-call sequence; a 4096-call sequence was memorized by Zen 5's predictor and hid all mispredictions). `lat` and `mixlen` take `RJ_BACKEND=sse|avx2`.
- 2026-09-29: short-path rework adopted (item 4). On the box, baseline binaries of this round are in `~/b0` (tree `~/rb` = working tree without the short-path changes), variants in `~/s1`–`~/s5` (s5 = current).
- 2026-09-29: AVX2/SSE items 6(a)–(c) adopted, 6(d) rejected. Current build on the box: `~/s16`. Baseline for the whole round: `~/b0`.
- 2026-09-29: items 13–16 adopted; current build `~/s22`. Rejected on the way: 12 (AES hybrid).
- 2026-09-29: item 17 adopted; item 18 (out-of-line AVX2 buffer copy) rejected. Final build `~/s25`; final comparison against `~/b0` written up in `results/amd-epyc-9r45-round2-2026-09-29.md` and CANDIDATES §10.7.
- 2026-09-29: item 19 adopted (addendum in the results file). Current build `~/s28`. Full test matrix passes on the box and on the M1.
- 2026-09-29: full x86 benchmark run of the current tree (every mode, backend, generic and native build) with this repository's own harnesses only. Raw data in `results/amd-epyc-9r45-full-2026-09-29/` (with `runall.sh`); tables in `docs/research/BENCHMARKS.md`.
- 2026-09-29: round 3 ended; wrap-up for publication (see "Where to start next time").
