#!/usr/bin/env bash
# Command-line interface tests. Run from the repository root after `cargo build --release`:
#   bash tests/cli.sh [path/to/crate]
# Each check prints PASS or FAIL; the script exits non-zero if any check failed.

set -u
P="${1:-target/release/crate}"
[ -x "$P" ] || P="$P.exe"
OUT=target/tests/cli
rm -rf "$OUT" && mkdir -p "$OUT"
FAILED=0

check() { # check <name> <command...>: passes if the command succeeds
    local name="$1"; shift
    if "$@" > "$OUT/last.log" 2>&1; then echo "PASS $name"; else echo "FAIL $name"; sed 's/^/    /' "$OUT/last.log" | head -5; FAILED=1; fi
}
json_ok() { grep -q '"ok":true' "$1"; }

# new: import, bake, export, save
check "new" bash -c "'$P' new assets/crate.glb -o $OUT/Crate.crate --tile-resolution 1024 --bake all --export $OUT/export --json > $OUT/new.json"
check "new reports ok" json_ok "$OUT/new.json"
check "new exported 6 files" bash -c "[ \$(ls $OUT/export | wc -l) -eq 6 ]"
check "new saved the project" test -f "$OUT/Crate.crate/project.json"

# run: commands on an existing project, saved
check "run -c" bash -c "'$P' run --project $OUT/Crate.crate --save -c 'add-fill; rename \"Base coat\"; set base_color=#2A4C7D' --json > $OUT/run.json"
check "run added the layer" grep -q '"name":"Base coat"' "$OUT/run.json"

# run from stdin
check "run from stdin" bash -c "printf 'open $OUT/Crate.crate\ninfo\n' | '$P' run - --json | grep -q '\"ok\":true'"

# variables
printf 'add-fill\nrename "Paint ${project}"\nset base_color=${color}\n' > "$OUT/template.txt"
cp -r "$OUT/Crate.crate" "$OUT/Copy_A.crate"
cp -r "$OUT/Crate.crate" "$OUT/Copy_B.crate"
check "batch" bash -c "'$P' batch $OUT/template.txt '$OUT/Copy_*.crate' --set color=#C03020 --save --json > $OUT/batch.json"
check "batch ran both projects" bash -c "grep -o '\"ok\":true' $OUT/batch.json | wc -l | grep -q 3"
check "batch filled \${project}" bash -c "'$P' info $OUT/Copy_B.crate --json | grep -q 'Paint Copy_B'"
check "missing variable fails" bash -c "! '$P' run --project $OUT/Crate.crate -c 'set base_color=\${nope}' > /dev/null 2>&1"

# export
check "export" bash -c "'$P' export $OUT/Crate.crate -o $OUT/export2 --preset 'Unreal Engine' --format png16 --json > $OUT/export.json && grep -q 'crate_ORM.1001.png' $OUT/export.json"

# reimport (same mesh: paint and layers must survive)
check "reimport" bash -c "'$P' reimport $OUT/Crate.crate assets/crate.glb --json > $OUT/reimport.json && grep -q '\"name\":\"Base coat\"' $OUT/reimport.json"

# watch: stops after one reimport triggered by rewriting the mesh
cp assets/crate.glb "$OUT/watched.glb"
( "$P" watch "$OUT/Crate.crate" --mesh "$OUT/watched.glb" --count 1 --interval 200 --json > "$OUT/watch.jsonl" 2>&1 ) &
WATCH=$!
sleep 6
cp assets/crate.glb "$OUT/watched.tmp" && touch "$OUT/watched.tmp" && mv -f "$OUT/watched.tmp" "$OUT/watched.glb"
for _ in $(seq 1 60); do kill -0 $WATCH 2>/dev/null || break; sleep 1; done
kill $WATCH 2>/dev/null
check "watch reimported" grep -q '"event":"reimported"' "$OUT/watch.jsonl"

# exit codes
"$P" info "$OUT/Missing.crate" > /dev/null 2>&1; code=$?
check "missing project exits 1" test "$code" -eq 1
"$P" export > /dev/null 2>&1; code=$?
check "high-poly bake" bash -c "'$P' new tests/data/hp_low.obj -o $OUT/High.crate --tile-resolution 1024 --high-poly tests/data/hp_high.obj --bake normal,height --json | grep -q '\"Normal\":\"Up to date\"'"
check "bad high poly fails" bash -c "! '$P' new tests/data/hp_low.obj -o $OUT/Bad.crate --high-poly README.md --bake normal > /dev/null 2>&1"
check "usage error exits 2" test "$code" -eq 2

exit $FAILED
