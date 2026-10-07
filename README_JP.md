# ALICE-Text

[English](README.md)

ログのような構造化テキスト向けの例外ベース圧縮 各行を *骨格* (値を placeholder に置き換えた行) と型別の値の列に分ける 値の列は時刻 (差分符号化)・日付・時刻・IPv4 / IPv6 アドレス・UUID・ログレベル・数値・パス・URL・メールアドレス 列にまとめた payload を Zstd で圧縮する 2 つ目の形式 (v3) は列ごとに別々に格納し、他の列を展開せずに 1 列だけを読んで絞り込める

`law` module は学習した行の骨格を法則として保持し、新しいテキストのうち予測できなかった行の割合を測る

## 何に向かないか

- 圧縮率だけが目的の場合: 下の測定のログでは gzip / zstd を直接使う方が小さい
- 散文などの非構造テキスト: 認識できる値が無いと骨格から分けるものが無い
- 統計的な次 token model: ここでの予測は「この行の骨格は以前に現れた」だけで、それより細かい予測はしない

## 目次

- [インストール](#インストール)
- [例](#例)
- [テキストの法則](#テキストの法則)
- [コマンドライン](#コマンドライン)
- [クエリエンジン (v3 形式)](#クエリエンジン-v3-形式)
- [Feature](#feature)
- [測定値](#測定値)
- [ファイル形式](#ファイル形式)
- [最小対応 Rust バージョン](#最小対応-rust-バージョン)
- [ビルドとテスト](#ビルドとテスト)
- [関連 crate](#関連-crate)
- [ライセンス](#ライセンス)

## インストール

crates.io には公開していないので repository に依存する

```sh
cargo add alice-text --git https://github.com/ext-sakamoro/ALICE-Text
```

Python バインディングは maturin でビルドする (`pyproject.toml` が `python` feature を有効にする)

```sh
pip install maturin
maturin develop --release
```

## 例

```rust
use alice_text::{ALICEText, EncodingMode};

let mut alice = ALICEText::new(EncodingMode::Pattern);

// Compress
let text = "2024-01-15 INFO User logged in from 192.168.1.100";
let compressed = alice.compress(text).unwrap();

// Decompress
let decompressed = alice.decompress(&compressed).unwrap();
assert_eq!(text, decompressed);
```

## テキストの法則

`alice_text::law::TextLaw` はコーパスの骨格を学習して法則として保持し、その残差として **exception rate** (予測できなかった骨格の行数 / 全行数) を持つ 値は常に実テキスト上で数える 学習中は、それより前の行に現れていない骨格の行を exception と数えるので、`p` 個の異なる骨格を持つ `p` 行の周期を `k` 回繰り返したコーパスの exception はちょうど `p` 行、学習時の rate は `1 / k` になる

`TextLaw::ingest` は法則を変えずに新しいテキストを判定する 規則の順序は `alice_zip::law::Verdict` と同じで、`band = max(abs_tolerance, 学習時の rate)`:

| 判定 | 条件 |
|------|------|
| `NoEvidence` | テキストに行が無い |
| `Supports` | テキストの exception rate ≤ band |
| `ParameterUpdate` | コーパスに続けてテキストを学習しても学習時の rate ≤ band (更新した法則を持つ) |
| `ResidualGrew` | exception rate ≤ band × `break_factor` |
| `Breaks` | それ以外 |

`OutOfRange` は無い どの行も骨格に変換でき、未見の骨格はまさに exception rate が数えるものだから

```rust
use alice_text::law::{TextLaw, TextVerdict};
use alice_zip::law::{IngestPolicy, LawError, Provenance};

fn main() -> Result<(), LawError> {
    let corpus: String = (0..10)
        .map(|i| format!("2024-01-15 10:30:{i:02} request {i} accepted\n"))
        .collect();
    let law = TextLaw::learn(&corpus, Provenance::new("service log", "line skeletons"))?;
    assert_eq!(law.learned_rate(), 0.1); // 1 skeleton over 10 lines

    let policy = IngestPolicy { abs_tolerance: 0.01, break_factor: 4.0 };
    let verdict = law.ingest("2024-01-16 08:00:00 request 99 accepted\n", &policy);
    assert!(matches!(verdict, TextVerdict::Supports { rate } if rate == 0.0));
    Ok(())
}
```

`cargo run --example text_law` は 300 行のログを学習し、4 種の入力 (値だけ違う同じログ、2 つ目の周期的なログ、無関係な文、空のテキスト) を判定する

## コマンドライン

```sh
alice-text compress server.log -o server.atxt --level balanced
alice-text decompress server.atxt -o server.log
alice-text info server.atxt
alice-text estimate server.log --detailed
alice-text verify server.atxt

# v3 format (columnar, queryable)
alice-text compress-v3 server.log -o server.atxt --level balanced
alice-text query server.atxt --stats
alice-text query server.atxt --columns
alice-text query server.atxt --select timestamps,ipv4 --where "log_levels=ERROR"
alice-text query server.atxt --select log_levels,ipv4 --where "timestamps>=2024-01-15 10:30:00" --format json
```

| レベル | Zstd レベル |
|--------|-------------|
| `fast` | 3 |
| `balanced` | 10 |
| `best` | 19 |

## クエリエンジン (v3 形式)

v3 形式はヘッダと列の目次を書き、その後に列ごとの圧縮ブロックを並べる `QueryEngine` はヘッダを読み、クエリが名指す列だけを展開する

```rust,ignore
use alice_text::{compress_v3, CompressionLevel, Op, QueryEngine};
use std::io::Cursor;

let compressed = compress_v3(text, CompressionLevel::Balanced)?;
let engine = QueryEngine::from_reader(Cursor::new(&compressed))?;

let stats = engine.stats();                                   // header only
let levels = engine.select_column("log_levels")?;             // one column
let errors = engine.filter_op("log_levels", Op::Eq, "ERROR")?; // row indices
let result = engine.query(&["timestamps", "ipv4"], "log_levels", Op::Eq, "ERROR")?;
```

## Feature

| Feature | 既定 | 内容 |
|---------|------|------|
| `ffi` | no | 圧縮・統計・エントロピー・台詞テーブルの C ABI (`extern "C"`)、C# / C++ のヘッダは `bindings/` |
| `python` | no | PyO3 の module `alice_text` (maturin でビルド) |
| `ml` | no | ALICE-ML への bridge: 学習済み重みによる 3 値の次 token スコア |
| `voice` | no | ALICE-Voice への bridge: 音声符号化のヒント |
| `search` | no | ALICE-Search への bridge: 圧縮テキストの検索 index |
| `font` | no | ALICE-Font への bridge: 文字集合とテキスト整形 |

bridge の 4 feature は隣の checkout (`../ALICE-ML` など) への path 依存

## 測定値

<!-- perf-measured: earlier release, not re-run for this one -->
以前の release で測定 (Rust 1.84.0、macOS、arm64 のノート機) 値はデータと機械に依存する Ratio = 圧縮後 / 元のサイズ、小さいほど良い

| データ | 行数 | サイズ | ALICE-Text | gzip -9 | zstd -19 |
|--------|------|--------|------------|---------|----------|
| ランダムなログ行 | 1K | 52 KB | 43.0% | 25.6% | 23.6% |
| ランダムなログ行 | 10K | 515 KB | 39.3% | 24.0% | 21.6% |
| ランダムなログ行 | 100K | 5.0 MB | 39.5% | 23.8% | 20.0% |
| 連続した時刻 | 100K | 6.5 MB | 34.2% | 12.9% | 10.9% |

5.7 MB (100,000 行) のログに対する v3 クエリ、同じ機械:

| 操作 | 時間 |
|------|------|
| 統計 (ヘッダのみ) | 11 ms |
| 絞り込み `log_levels=ERROR` (20K 件一致) | 16 ms |
| 絞り込み `timestamps>=` (70K 件一致) | 28 ms |
| 絞り込み `ipv4=` | 7 ms |

`.cargo/config.toml` には `target-cpu=native` を置かない (CPU 世代の違う CI runner で rustc が SIGILL で落ちる) ベンチマークでは呼び出しごとに指定する

```sh
RUSTFLAGS="-C target-cpu=native" cargo bench
cargo bench --config 'build.rustflags=["-C","target-cpu=native"]'
```

`.cargo/config.local.toml` は cargo が読まない

## ファイル形式

拡張子は `.atxt` 既定の形式 (v2):

```text
Magic "ALICETXT" (8 bytes)
Version 2.0 (2 bytes)
Header (24 bytes): original length, compression mode, pattern count, skeleton length
Zstd payload: bincode-serialized skeleton tokens and value columns
```

`ALICEText::decompress` は旧形式 v1 (LZMA) も読む

展開は上限を越えて行わない v3 の各列は展開後サイズを記録し、ちょうどその大きさに展開されなければならない 記録の無い stream (v2、台詞テーブル、以前の版が書いた v3) の上限は 256 MiB で、writer もそれを越える stream は書かない

## 最小対応 Rust バージョン

Rust 1.87 (`Cargo.toml` の `rust-version`) CI で既定 feature と `ffi` + `python` の library を検査する 1.86 は依存先 `alice-zip` (その `rust-version` が 1.87) が受け付けない 開発と CI は `rust-toolchain.toml` で固定した toolchain を使う

## ビルドとテスト

bridge の feature は optional な path 依存だが、package の解決に manifest が要る 隣に clone するか、`bash .github/stub-siblings.sh` で空の manifest を作る (`../ALICE-ML/Cargo.toml` などを上書きする)

```sh
cargo test                                           # unit + doc tests
cargo test --features ffi
cargo run --example text_law
cargo clippy --all-targets --features ffi,python -- -D warnings   # pedantic + nursery (Cargo.toml)
cd fuzz && cargo +nightly fuzz run fuzz_decompress   # decoders on untrusted bytes
scripts/preflight.sh            # every CI step with the same arguments
scripts/preflight.sh --quick    # static checks, clippy, docs and `cargo test --lib`
```

## 関連 crate

- [ALICE-Zip](https://github.com/ext-sakamoro/ALICE-Zip): 手続き的圧縮 その `law` module が `TextLaw` の使う `Provenance` / `IngestPolicy` と判定の順序を定める
- [ALICE-DB](https://github.com/ext-sakamoro/ALICE-DB): model ベースのデータベース

## ライセンス

次のどちらかを選んで利用できる

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

商標の条件は [TRADEMARK_NOTICE](TRADEMARK_NOTICE)
