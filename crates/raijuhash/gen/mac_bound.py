#!/usr/bin/env python3
"""Evaluate the forging bound of `Mac` (nEHtM over RaijuHash) for concrete
query budgets; produces the table in SPEC.md section 6.2.

The information-theoretic part is the smaller of two published bounds for
nEHtM[H, pi] with a random permutation pi on n = 128 bits and an (n-1)-bit
delta-AXU hash H:

- DNT19: Dutta, Nandi, Talnikar, EUROCRYPT 2019, ePrint 2019/127, Theorem 1.
- CLLL20: Choi, Lee, Lee, Lee, ASIACRYPT 2020, ePrint 2020/1145, Theorem 2
  (Section 4; stated for an (n-1)-bit delta-AXU hash, with the free integer
  parameter `L` chosen here to minimize the bound).

`q` MAC queries, `v` verification queries, `mu` faulty MAC queries (a query
reusing an earlier nonce with a different message). Messages are at most
`max_len` bytes. RaijuHash is eps-AXU with eps = (ceil(L/8192) + 1) / 2^128
(SPEC.md, Theorem 5); keeping the low 127 bits at most doubles it, so
delta = (ceil(L/8192) + 1) / 2^127.

Not included (reported separately): the PRP advantage of AES-128 against
2(q + v) queries under the MAC cipher key, and for `Mac::from_seed` the PRP
advantage against 527 queries under the seed plus the PRP/PRF switch on those
527 distinct counter blocks affecting MAC outputs: 526 hash-key blocks and
one cipher-key block. The implementation also generates an unused avalanche
multiplier V; that unobserved block is omitted from the reduction.
"""

import math

N = 128
SEED_BLOCKS = 8416 // 16 + 1  # used hash key blocks (excluding V) plus the AES key block


def delta(max_len):
    # Keep the ceiling exact even just above a chunk boundary at large lengths.
    chunks = (max_len + 8191) // 8192
    return (chunks + 1) / 2 ** (N - 1)


def dnt19(q, v, mu, d):
    n = N
    return (48 * q**3 / 2 ** (2 * n) + 12 * q**4 * d / 2 ** (2 * n) + 12 * mu**2 * q**2 / 2 ** (2 * n)
            + (q + 2 * v) / 2**n + 4 * q**3 * d / 2**n + (2 * q + v) * mu * d + v * d)


def clll20(q, v, mu, d):
    n = N
    assert q + v <= 2 ** (n - 3)
    mu = max(mu, 1)  # the theorem takes a positive integer; the bound is monotone in mu
    eps = (6 * q * d + q / 2**n + 6 * q**2 * d**2 + q**2 * d / 2**n + 18 * q**2 / 2 ** (2 * n) + 4 * mu * d
           + 24 * mu**2 * d**0.5 / 2**n + 4 * mu**2 * q * d / 2**n + 36 * mu**3 / 2 ** (2 * n)
           + 36 * mu * q**2 * d**1.5 / 2**n + 54 * mu**2 * q**2 * d / 2 ** (2 * n) + 16 * q * v / 2 ** (2 * n))
    fixed = (10 * q**2 * d**0.5 / 2**n + 16 * q**4 / 2 ** (3 * n) + 5 * mu**2 * d + mu**2 / 2**n
             + 3 * mu * q**1.5 * d / 2 ** (n / 2) + 6 * mu**3 * d**0.5 / 2**n + 24 * mu * q**2 / 2 ** (2 * n)
             + 25 * mu**4 / 2 ** (2 * n) + 2 * v / 2**n + eps)

    def with_l(l):
        # 2^n (e mu^2 / (l 2^n))^l, in logs to avoid overflow.
        t = n * math.log(2) + l * (math.log(math.e * mu**2) - math.log(l) - n * math.log(2))
        return (2 * l + 1) * v * d + (math.exp(t) if t < 700 else math.inf)

    return fixed + min(with_l(l) for l in range(1, 200))


def bound(q, v, mu, max_len):
    d = delta(max_len)
    return min(dnt19(q, v, mu, d), clll20(q, v, mu, d))


def lg(x):
    return "≥ 1" if x >= 1 else f"2^{math.log2(x):.1f}"


def main():
    print(f"seed derivation, PRP/PRF switch: {SEED_BLOCKS}*{SEED_BLOCKS - 1}/2^129 = "
          f"{lg(SEED_BLOCKS * (SEED_BLOCKS - 1) / 2**129)}")
    print()
    print("| max message | MAC queries q | verifications v | faulty mu | forging bound |")
    print("|---|---|---|---|---|")
    rows = [
        (2**20, 2**48, 2**48, 0),
        (2**32, 2**48, 2**48, 0),
        (2**40, 2**48, 2**48, 0),
        (2**40, 2**64, 2**64, 0),
        (2**32, 2**64, 2**48, 0),
        (2**32, 2**64, 2**48, 2**16),
        (2**32, 2**64, 2**48, 2**32),
        (2**32, 2**64, 2**48, 2**48),
        (2**40, 2**80, 2**48, 0),
        (2**64 - 1, 2**48, 2**48, 0),
    ]
    for max_len, q, v, mu in rows:
        size = f"2^{math.log2(max_len):.0f} B" if max_len < 2**63 else "2^64 − 1 B"
        mus = "0" if mu == 0 else f"2^{math.log2(mu):.0f}"
        print(f"| {size} | 2^{math.log2(q):.0f} | 2^{math.log2(v):.0f} | {mus} | {lg(bound(q, v, mu, max_len))} |")


if __name__ == "__main__":
    main()
