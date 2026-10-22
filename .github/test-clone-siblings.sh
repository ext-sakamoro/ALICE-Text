#!/usr/bin/env bash
# .github/clone-siblings.sh の挙動の試験 (一時 dir の中だけで動く、network で ALICE-Zip を取る)
#
# 1. 手元 (GITHUB_ACTIONS 無し) で無関係な ../ALICE-Zip がある → exit 1、中身はそのまま
# 2. 手元で ZIP_REV を含む checkout (目印なし) がある → exit 0、中身はそのまま
# 3. 手元で何も無い → clone して目印を置く
# 4. 手元で script が作った clone (目印あり) がある → 作り直す
# 5. Actions (GITHUB_ACTIONS=true) で無関係な ../ALICE-Zip がある → 作り直す
set -euo pipefail

script="$(cd "$(dirname "$0")" && pwd)/clone-siblings.sh"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/Text"
zip="$work/ALICE-Zip"
fail=0
checked=0

run() { (cd "$work/Text" && "$@" bash "$script") >/dev/null 2>&1; }
expect() {
  checked=$((checked + 1))
  if [[ "$1" != "$2" ]]; then echo "FAIL: $3 (got $1, want $2)"; fail=1; else echo "ok: $3"; fi
}

# 1
mkdir -p "$zip" && echo keep > "$zip/work.txt"
rc=0; run env -u GITHUB_ACTIONS || rc=$?
expect "$rc" 1 "local + unrelated dir: refused"
expect "$(cat "$zip/work.txt" 2>/dev/null || echo gone)" keep "local + unrelated dir: left untouched"
rm -rf "$zip"

# 3
rc=0; run env -u GITHUB_ACTIONS || rc=$?
expect "$rc" 0 "local + no dir: cloned"
expect "$(test -f "$zip/.clone-siblings-made-this" && echo yes || echo no)" yes "local + no dir: marker placed"

# 4
echo extra > "$zip/extra.txt"
rc=0; run env -u GITHUB_ACTIONS || rc=$?
expect "$rc" 0 "local + own clone: recreated"
expect "$(test -f "$zip/extra.txt" && echo kept || echo replaced)" replaced "local + own clone: replaced"

# 2 (an own clone with the marker removed stands for a developer's checkout that contains ZIP_REV)
rm "$zip/.clone-siblings-made-this"
echo mine > "$zip/mine.txt"
rc=0; run env -u GITHUB_ACTIONS || rc=$?
expect "$rc" 0 "local + checkout containing ZIP_REV: accepted"
expect "$(cat "$zip/mine.txt" 2>/dev/null || echo gone)" mine "local + checkout containing ZIP_REV: left untouched"
rm -rf "$zip"

# 5
mkdir -p "$zip" && echo junk > "$zip/junk.txt"
rc=0; run env GITHUB_ACTIONS=true || rc=$?
expect "$rc" 0 "Actions + unrelated dir: recreated"
expect "$(test -f "$zip/junk.txt" && echo kept || echo replaced)" replaced "Actions + unrelated dir: replaced"

if [[ "$checked" -eq 0 ]]; then echo "FAIL: nothing checked"; exit 1; fi
echo "checked $checked"
exit "$fail"
