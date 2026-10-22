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
set -euo pipefail

ZIP_REV=a59ba9c0bd511f036180a6f4092c4b99a286567c

dir=../ALICE-Zip
rm -rf "$dir"
git init -q "$dir"
git -C "$dir" remote add origin https://github.com/ext-sakamoro/ALICE-Zip.git
git -C "$dir" fetch -q --depth 1 origin "$ZIP_REV"
git -C "$dir" checkout -q --detach FETCH_HEAD
echo "cloned alice-zip at $(git -C "$dir" rev-parse HEAD) into $dir"
