# ALICE-Text

**例外ベーステキスト圧縮**

> 予測ではなく、驚きだけを送信する。

ALICE-Textは、パターン認識とカラムナーエンコーディングを使用してログなどの構造化テキストを圧縮するシステムです。既知のパターン（タイムスタンプ、IP、UUIDなど）を抽出し、型固有のバイナリ形式で保存します。

## 原理

```
┌─────────────────────────────────────────────────────────────┐
│  入力テキスト                                               │
│       ↓                                                     │
│  パターン認識 (Timestamp, IP, UUID, LogLevel, ...)          │
│       ↓                                                     │
│  カラムナーエンコーディング (Struct of Arrays)              │
│       ↓                                                     │
│  型固有の圧縮                                               │
│    - Timestamps: デルタエンコーディング (ミリ秒)            │
│    - IPv4: u32, IPv6: u128                                  │
│    - UUID: u128                                             │
│    - LogLevel: u8                                           │
│       ↓                                                     │
│  Zstd圧縮                                                   │
└─────────────────────────────────────────────────────────────┘
```

## インストール

### Rust CLI

```bash
# ソースからビルド
cargo build --release

# インストール
cargo install --path .
```

### Python (maturin経由)

```bash
# maturinをインストール
pip install maturin

# ビルドとインストール
maturin develop --release
```

## クイックスタート

### CLI使用方法

```bash
# ファイルを圧縮
alice-text compress server.log -o server.atxt --level balanced

# 解凍
alice-text decompress server.atxt -o server.log

# ファイル情報を表示
alice-text info server.atxt

# 圧縮率を推定
alice-text estimate server.log --detailed

# 整合性を検証
alice-text verify server.atxt
```

### 圧縮レベル

| レベル | Zstdレベル | 用途 |
|--------|------------|------|
| `fast` | 3 | 高速圧縮、大きめのファイル |
| `balanced` | 10 | デフォルト、バランス型 |
| `best` | 19 | 最大圧縮、低速 |

### Rust API

```rust
use alice_text::{ALICEText, EncodingMode};

// 圧縮器を作成
let mut alice = ALICEText::new(EncodingMode::Pattern);

// 圧縮
let text = "2024-01-15 10:30:45 INFO User logged in from 192.168.1.100";
let compressed = alice.compress(text).unwrap();

// 解凍
let decompressed = alice.decompress(&compressed).unwrap();
assert_eq!(text, decompressed);

// 統計を確認
if let Some(stats) = alice.last_stats() {
    println!("圧縮率: {:.1}%", stats.compression_ratio() * 100.0);
}
```

## 機能

### パターン認識

自動的に検出・抽出するパターン:

| パターン | 保存形式 | 例 |
|----------|----------|-----|
| Timestamp | デルタエンコード i64 (ms) | `2024-01-15T10:30:45+09:00` |
| IPv4 | u32 | `192.168.1.100` |
| IPv6 | u128 | `2001:db8::1` |
| UUID | u128 | `550e8400-e29b-41d4-a716-446655440000` |
| LogLevel | u8 | `INFO`, `WARN`, `ERROR` |
| Date | u32 (エポック日) | `2024-01-15` |
| Time | u32 (深夜からのms) | `10:30:45` |
| Number | f64 | `42`, `3.14159` |
| Email | String | `user@example.com` |
| URL | String | `https://example.com` |
| Path | String | `/var/log/syslog` |

### デルタエンコーディング

連続したタイムスタンプはデルタエンコーディングの恩恵を受けます:

```
Before: "2024-01-15 10:30:45" (19バイト) × N
After:  base + [0, 1000, 1000, ...] (圧縮後は各数バイト)
```

### タイムゾーンサポート

タイムスタンプのタイムゾーン情報を保持:

- `2024-01-15T10:30:45+09:00` → `+09:00`付きで復元
- `2024-01-15T10:30:45Z` → `Z`付きで復元

## いつ使うべきか

✅ **ALICE-Textを使うべき場面:**
- 圧縮したログデータを完全解凍せずにクエリしたい
- タイムスタンプのタイムゾーン情報の保持が重要（`+09:00`, `Z`）
- 特定フィールド（IP, UUID, LogLevel）へのカラムナーアクセスが必要
- ETL不要の分析パイプラインを構築したい（圧縮データが既に構造化済み）

❌ **ALICE-Textを使わないべき場面:**
- 最大圧縮率だけが目的（gzipやzstdを使用すべき）
- 非構造化テキスト（小説、記事、散文）を圧縮したい
- 圧縮データをクエリ・分析する必要がない
- ファイルサイズだけが重要な指標

**トレードオフ:** ALICE-Text v3は27-43%の圧縮率とクエリ可能なカラムナーストレージを実現します。

## クエリエンジン（v3フォーマット）

ALICE-Text v3は、**完全解凍なしでSQLライクなクエリが可能**なカラムナーストレージを導入しました。

### クエリワークフロー

```
┌─────────────────────────────────────────────────────────────┐
│  1. ヘッダのみ読み込み（メタデータ、カラムディレクトリ）      │
│       ↓                                                     │
│  2. フィルタカラムのみ解凍（例: log_levels）                 │
│       ↓                                                     │
│  3. マッチするインデックスを特定（例: ERRORエントリ）        │
│       ↓                                                     │
│  4. 該当インデックスの選択カラムのみ解凍                     │
│       ↓                                                     │
│  結果: 完全解凍の10倍〜100倍高速                             │
└─────────────────────────────────────────────────────────────┘
```

### CLI使用方法

```bash
# v3フォーマットで圧縮（クエリ可能）
alice-text compress-v3 server.log -o server.atxt --level balanced

# ファイル統計を表示（ヘッダのみ読み込み - 瞬時）
alice-text query server.atxt --stats

# 利用可能なカラム一覧
alice-text query server.atxt --columns

# 特定カラムのみ選択（部分解凍）
alice-text query server.atxt --select timestamps,log_levels,ipv4

# フィルタ: ERRORエントリのみ抽出
alice-text query server.atxt --select timestamps,ipv4 --where "log_levels=ERROR"

# フィルタ: タイムスタンプ範囲クエリ
alice-text query server.atxt --select log_levels,ipv4 --where "timestamps>=2024-01-15 10:30:00"

# JSON形式で出力
alice-text query server.atxt --select log_levels,ipv4 -w "log_levels=ERROR" --format json

# 結果数を制限
alice-text query server.atxt --select log_levels,ipv4 --limit 100
```

### Rust API

```rust
use alice_text::{QueryEngine, Op, compress_v3, CompressionLevel};
use std::io::Cursor;

// v3フォーマットで圧縮
let text = "2024-01-15 10:30:45 INFO User logged in from 192.168.1.100\n...";
let compressed = compress_v3(text, CompressionLevel::Balanced)?;

// クエリ用にオープン
let cursor = Cursor::new(&compressed);
let mut engine = QueryEngine::from_reader(cursor)?;

// 統計取得（ヘッダのみ - O(1)）
let stats = engine.stats();
println!("行数: {}, カラム数: {}", stats.row_count, stats.column_count);

// 特定カラムのみ選択（部分解凍）
let levels = engine.select_column("log_levels")?;

// オペレータでフィルタ
let error_indices = engine.filter_op("log_levels", Op::Eq, "ERROR")?;

// フルクエリ: 1カラムでフィルタ、別カラムを選択
let result = engine.query(
    &["timestamps", "ipv4"],  // SELECT
    "log_levels",              // WHERE カラム
    Op::Eq,                    // 演算子
    "ERROR",                   // 値
)?;

for row in &result.rows {
    println!("{:?}", row.values);
}
```

### クエリパフォーマンス（実測値）

**5.7 MBログファイル（10万行）, Apple M3:**

| 操作 | 時間 | 備考 |
|------|------|------|
| 統計（ヘッダのみ） | **11ms** | O(1) メタデータ読み込み |
| フィルタ: log_levels=ERROR | **16ms** | 型付きu8スキャン, 2万件マッチ |
| フィルタ: timestamps>= | **28ms** | 型付きi64スキャン, 7万件マッチ |
| フィルタ: ipv4= | **7ms** | 型付きu32スキャン |

**カラムナー圧縮（実測値）:**

| カラム | 10万件 | 圧縮後 | 圧縮率 |
|--------|--------|--------|--------|
| timestamps | ~2 MB | **151バイト** | 0.007% |
| log_levels | ~500 KB | 34 KB | 6.8% |
| ipv4 | ~1.5 MB | 249 KB | 16.6% |

## ベンチマーク

テスト環境: Apple M3 (arm64), macOS, Rust 1.84.0

### ランダムログデータ

| 行数 | サイズ | ALICE-Text | gzip -9 | zstd -19 |
|------|--------|------------|---------|----------|
| 1K | 52 KB | 43.0% | 25.6% | 23.6% |
| 10K | 515 KB | 39.3% | 24.0% | 21.6% |
| 100K | 5.0 MB | 39.5% | 23.8% | 20.0% |

### 構造化ログデータ（連続タイムスタンプ）

| 行数 | サイズ | ALICE-Text | gzip -9 | zstd -19 |
|------|--------|------------|---------|----------|
| 100K | 6.5 MB | 34.2% | 12.9% | 10.9% |

*低いほど良い圧縮。圧縮率 = 圧縮後サイズ / 元サイズ*

### 速度 (100K行, 5 MB)

| 操作 | ALICE-Text |
|------|------------|
| 圧縮 | ~340 ms (~15 MB/s) |
| 解凍 | ~350 ms (~14 MB/s) |

### 補足

- ALICE-Textは一般的なログデータで**34-43%の圧縮率**を達成
- 汎用圧縮ツール（gzip, zstd）は生テキストでより高い圧縮率を達成
- ALICE-Textの強みは**パターン認識型カラムナーストレージ**と**型固有エンコーディング**
- 最大圧縮が必要な場合はgzipやzstdを直接使用
- ログコンポーネントへの構造化アクセスが必要な場合にALICE-Textが有用

## ファイルフォーマット

ALICE-Textファイルは `.atxt` 拡張子を使用。

```
┌────────────────────────────────────────────────────────────┐
│ Magic: "ALICETXT" (8バイト)                                │
├────────────────────────────────────────────────────────────┤
│ Version: 2.0 (2バイト)                                     │
├────────────────────────────────────────────────────────────┤
│ Header (24バイト)                                          │
│   - 元の長さ (8バイト)                                     │
│   - 圧縮モード (1バイト)                                   │
│   - パターン数 (4バイト)                                   │
│   - スケルトン長 (4バイト)                                 │
├────────────────────────────────────────────────────────────┤
│ 圧縮ペイロード (Zstd)                                      │
│   - スケルトントークン (バイナリ)                          │
│   - カラムナーデータ (Bincodeシリアライズ)                 │
│     - timestamps (デルタエンコード)                        │
│     - ipv4_addrs (Vec<u32>)                                │
│     - ipv6_addrs (Vec<u128>)                               │
│     - uuids (Vec<u128>)                                    │
│     - log_levels (Vec<u8>)                                 │
│     - ... その他のカラム                                   │
└────────────────────────────────────────────────────────────┘
```

## ビルド設定

最大パフォーマンス向けに最適化:

```toml
# Cargo.toml
[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
panic = "abort"
strip = true
```

mimallocアロケータを使用してメモリ割り当てパフォーマンスを向上。

## 依存関係

**Rust:**
- zstd - 圧縮
- bincode - バイナリシリアライズ
- chrono - タイムスタンプ解析
- regex - パターンマッチング
- mimalloc - 高性能アロケータ
- clap - CLI引数解析

**Python (オプション):**
- maturin - ビルドシステム
- pyo3 - Pythonバインディング

## ライセンス

BSL 1.1 (Business Source License 1.1)

- 非商用・個人・研究用途: 無料
- 商用SaaS利用: 有償ライセンスが必要
- 変更日: 2028-01-31（MIT Licenseに移行）

詳細は [LICENSE](LICENSE) を参照

## 作者

坂本 師也 (Moroya Sakamoto)

## 関連プロジェクト

- [ALICE-Zip](https://github.com/ext-sakamoro/ALICE-Zip) - 手続き的生成圧縮
- [ALICE-DB](https://github.com/ext-sakamoro/ALICE-DB) - モデルベースデータベース
