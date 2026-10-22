#!/usr/bin/env bash
# CI で非 optional な sibling path dep を実 clone する
#
# `alice-zip` は既定 build で compile される (`law` module が `Provenance` /
# `IngestPolicy` / `LawError` を使う) ので、stub ではなく実物が要る
# (判定軸は stub-siblings.sh の冒頭を参照)
#
# path で取るのは、同じ build に入る他の crate (ALICE-LOL 等) が path で
# 引く alice-zip と同じ 1 つの package に解決させるため (registry や git の
# source にすると別 package として 2 つ link される)
#
# 取る commit はここ 1 箇所で固定する 上げる時はこの値だけを変える
#
# ../ALICE-Zip を消してよいのは次の 2 つだけ (手元の checkout は消さない):
#   - GitHub Actions の中 (GITHUB_ACTIONS=true、作業領域は使い捨て)
#   - この script が作った clone (目印の file がある)
# それ以外で ../ALICE-Zip が既にあれば、中身に触れずに ZIP_REV を含むかだけを確かめる
# 含まなければ取得の command を出して exit 1
# 挙動の試験は .github/test-clone-siblings.sh
set -euo pipefail

ZIP_REV=a59ba9c0bd511f036180a6f4092c4b99a286567c
ZIP_URL=https://github.com/ext-sakamoro/ALICE-Zip.git

dir=../ALICE-Zip
sentinel=.clone-siblings-made-this

if [[ -e "$dir" ]]; then
  if [[ "${GITHUB_ACTIONS:-}" == true || -f "$dir/$sentinel" ]]; then
    rm -rf "$dir"
  else
    if git -C "$dir" cat-file -e "${ZIP_REV}^{commit}" 2>/dev/null &&
      git -C "$dir" merge-base --is-ancestor "$ZIP_REV" HEAD 2>/dev/null; then
      echo "using the existing $dir (HEAD $(git -C "$dir" rev-parse --short HEAD) contains $ZIP_REV)"
      exit 0
    fi
    echo "refusing: $dir exists and its HEAD does not contain $ZIP_REV (left untouched)" >&2
    echo "update it with: git -C $dir fetch origin $ZIP_REV && git -C $dir checkout --detach $ZIP_REV" >&2
    exit 1
  fi
fi

git init -q "$dir"
git -C "$dir" remote add origin "$ZIP_URL"
git -C "$dir" fetch -q --depth 1 origin "$ZIP_REV"
git -C "$dir" checkout -q --detach FETCH_HEAD
: > "$dir/$sentinel"
echo "/$sentinel" >> "$dir/.git/info/exclude"
echo "cloned alice-zip at $(git -C "$dir" rev-parse HEAD) into $dir"
