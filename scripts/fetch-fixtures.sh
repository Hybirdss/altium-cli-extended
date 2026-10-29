#!/usr/bin/env bash
# Fetch the exact real projects used by CI. Missing fixtures fail CLI tests in CI.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
mkdir -p "$root/.fixtures/samples"
cd "$root/.fixtures/samples"
while read -r repo revision; do
    destination=$(basename "$repo")
    if [ ! -d "$destination/.git" ]; then
        mkdir -p "$destination"
        git -C "$destination" init -q
        git -C "$destination" remote add origin "https://github.com/$repo.git"
    fi
    current=$(git -C "$destination" rev-parse HEAD 2>/dev/null || true)
    if [ "$current" != "$revision" ]; then
        if [ -n "$(git -C "$destination" status --porcelain)" ]; then
            echo "refusing to replace modified fixture $destination" >&2
            exit 1
        fi
        git -C "$destination" fetch -q --depth 1 origin "$revision"
        git -C "$destination" checkout -q --detach "$revision"
    fi
    if [ -n "$(git -C "$destination" status --porcelain)" ]; then
        echo "fixture $destination has local changes; restore it before testing" >&2
        exit 1
    fi
done <<'REVISIONS'
ryansturmer/BeagleBoneCape 0daa2990ccbcb3f6558a75cf207395a9c3578585
SalimTerryLi/AirPi_PCB 3fe1413e8a689d118068385795227a1618b7a3b2
SalimTerryLi/PilotPi_PCB a51b739a550d7434c7dcb8ca670c368434281dbd
mrKuzmich/quasar_arm_pcb c54dd1784a7f0589a786b4dd6ea7e737f1a4872f
REVISIONS
echo "pinned fixtures in $root/.fixtures/samples"
