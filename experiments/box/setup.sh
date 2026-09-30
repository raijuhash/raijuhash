#!/bin/bash
# setup.sh: on a fresh box, after syncing the working tree to ~/raijuhash and a
# `git archive HEAD` of the baseline to ~/rb, install the helpers and build the
# baseline (~/b0) and the working tree (~/s0).
set -e
cp ~/raijuhash/experiments/box/snap.sh ~/raijuhash/experiments/box/ab.sh ~/
chmod +x ~/snap.sh ~/ab.sh
~/snap.sh b0 ~/rb
~/snap.sh s0
