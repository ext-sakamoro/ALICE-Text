#!/usr/bin/env bash
# CI で optional な sibling path dep の manifest だけを用意する
#
# `Cargo.toml` の 4 つの path dep (alice-ml / alice-voice / alice-search /
# alice-font) は CI に存在しないが、**cargo は optional dep でも path の実在を
# 要求する** ので、これが無いと `cargo check` / `test` / `clippy` が
# manifest 解決の段階で落ちる (2026-09-29 実測: optional / 非 optional の
# どちらでも `cargo check` は落ちる 一方 `cargo fmt -- --check` は落ちない
# ので、fmt しか無かった頃の CI は path dep 不在でも green のままだった)
#
# ⚠️ ここで **stub が正しい** 理由は「4 つとも `optional = true` かつ
# `default = []`」だから 既定 feature の build では manifest が読めれば足り、
# stub の中身は 1 行も compile されない
#
# ⚠️ 対の例を挙げておく: ALICE-Edge-Firewall / ALICE-Queue / ALICE-Risk の
# `alice-blockchain` は `signature::{KeyPair, PublicKey, Signature}` を
# **無条件に**使い、`tampered_*_breaks_verify` 系の test が実 Ed25519 の
# 署名検証に依存する そこで stub を置くと「暗号的に無効な test double」で
# CI が緑になるので、read-only deploy key で実 clone している
#
# **判定軸は「その dep が既定 build で compile されるか」** であって、
# private かどうかでも暗号かどうかでもない
#
# `--all-features` まで CI で見たくなったら実 crate が要る そのとき
# `alice-font` だけ PRIVATE なので deploy key が 1 本必要になる
# (alice-ml / alice-voice / alice-search は PUBLIC)
set -euo pipefail

for entry in \
  "ALICE-ML:alice-ml" \
  "ALICE-Voice:alice-voice" \
  "ALICE-Search:alice-search" \
  "ALICE-Font:alice-font"; do
  dir="${entry%%:*}"
  pkg="${entry##*:}"
  mkdir -p "../$dir/src"
  cat > "../$dir/Cargo.toml" <<TOML
[package]
name = "$pkg"
version = "0.1.0"
edition = "2021"
license = "MIT OR Apache-2.0"

[lib]
path = "src/lib.rs"

[features]
default = []
std = []
TOML
  : > "../$dir/src/lib.rs"
  echo "stubbed $pkg at ../$dir"
done
