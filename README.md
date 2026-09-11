# flp-extract-fxp

FL Studio プロジェクトファイル (.flp) 内に埋め込まれた **Serum 1 プリセット** を取り出し、**Serum 2 で読み込める .fxp** として出力する CLI ツールです。

- FLP 内の Serum (VST3) プラグイン状態から Serum 1 プリセットチャンクを復元
- Serum 2 の Serum-1 インポートチェック (静的逆解析 + 動的検証済み、下記参照) を満たす fxp を生成
- 書き出し前にバリデーションし、Serum 2 が拒否するファイルは既定で出力しない

## ビルド

```sh
cargo build --release
```

実行ファイル: `target/release/flp-extract-fxp` (Windows では `.exe`)。依存は clap 4 と flate2 のみ。

## 使い方

### `list` — Serum インスタンスの一覧

```sh
flp-extract-fxp list "path/to/project.flp"
```

Serum 1 インスタンスごとに、チャンネル番号/チャンネル名/プラグイン名/プリセット名/作者/状態サイズ/埋め込みストリーム数/復元元を表示します。Serum 2 インスタンスは件数のみ報告します。

### `extract` — プリセットの抽出

```sh
flp-extract-fxp extract "path/to/project.flp"
flp-extract-fxp extract -o out_dir --overwrite a.flp b.flp
flp-extract-fxp extract --keep_invalid a.flp
```

| フラグ | 意味 |
|---|---|
| `-o`, `--out <DIR>` | 出力ディレクトリ。既定は `<flp名>_serum_fxp` (FLP と同じ場所) |
| `--overwrite` | 既存出力ファイルを上書き (既定はスキップ) |
| `--keep_invalid` | Serum 2 バリデーションに失敗したプリセットも出力する (既定はスキップし、最後に終了コード 1 で報告) |

同一チャンクの重複プリセットは自動的にスキップします。抽出結果にはプリセット名/作者/カテゴリ/バージョンと埋め込みウェーブテーブルのサイズが出力されます。

### `validate` — fxp の検証

```sh
flp-extract-fxp validate preset.fxp
```

Serum 2 の Serum-1 インポータと同じ規則で検査し、`PASS` / `FAIL` とプリセット名を表示します。FAIL の場合は終了コード 1。拡張子が `.fxp` でない場合も FAIL になります (Serum 2 は `.fxp` 名のときしかインポートを提示しないため)。

## 出力ファイル名

`NN_プリセット名.fxp` 形式 (`NN` は出力順の連番、01 始まり)。プリセット名は状態内の名前フィールド (下表オフセット 0x4972)。空の場合はチャンネル名、それも空なら `Serum N` を使用します。同一ベース名は `NN_名_2.fxp` のように接尾番号を付けます。ファイル名に使えない文字 (`/\:*?"<>|`、制御文字) は `_` に置換し、前後の空白とドットを除去して最大 80 文字に切り詰めます。

## 技術概要

### FLP 内の Serum 1 状態 (イベント 213 → チャンク ID 53)

| 構造 | 内容 |
|---|---|
| FLP コンテナ | `FLhd` (u32 LE 長 + ヘッダ) + `FLdt` (u32 LE 長 + イベント列) |
| イベント | 1 バイト ID + データ。0–63: 1 バイト / 64–127: 2 バイト (LE) / 128–191: 4 バイト (LE) / 192 以上: varint 長 + データ |
| イベント 64 (NewChan) | 直後のチャンネル名イベントの所有チャンネル (u16 LE) |
| イベント 203 / 204 | チャンネル名 / FX トラック名 (UTF-8 または UTF-16LE) |
| イベント 213 (PluginParams) | u32 LE バージョン + `[u32 cid][u64 サイズ][データ]` 列。ID 53=状態 / 54=名前 / 55=ファイル名 / 56=ベンダ |
| チャンク 53 (State) | FL の VST3 ラッパー状態: プロローグ `u32 = 1` + `[u32 cid][u64 サイズ]` チャンク列。**cid = 3 がプラグイン本体の保存状態** |

cid = 3 のペイロード (Serum 1 の場合) は fxp の `chunk` 領域そのもので、次の構成です:

```
[zlib ストリーム 0: プリセット状態 172,736 バイト]
[zlib ストリーム 1...: 埋め込みウェーブテーブル / ノイズ / フィルタテーブル]
[u32 LE トレーラ: ストリーム 0 の圧縮後サイズ]
```

Serum 2 の状態 (cid = 3 が `XferJson...` で始まる) は抽出対象外です。

### Serum 1 fxp 形式 (60 バイトヘッダ、マルチバイトはビッグエンディアン)

| オフセット | サイズ | フィールド | 値 |
|---:|---:|---|---|
| 0x00 | 4 | chunkMagic | `CcnK` |
| 0x04 | 4 | byteSize | **ファイル全長** (Steinberg 仕様の fileLen−8 ではなく、Serum の実際の出力に従う) |
| 0x08 | 4 | fxMagic | `FPCh` |
| 0x0C | 4 | version | `1` |
| 0x10 | 4 | fxProgramID | `XfsX` |
| 0x14 | 4 | fxVersion | `1` |
| 0x18 | 4 | numParams | `1` |
| 0x1C | 28 | prgName | プリセット名 (NUL 詰め) |
| 0x38 | 4 | chunkSize | ファイル長 − 60 |
| 0x3C | … | chunk | zlib ストリーム列 + u32 LE トレーラ (上記と同じ) |

復元後の状態 (172,736 バイト) 内のメタデータ:

| オフセット | サイズ | 内容 |
|---:|---:|---|
| 0x4972 | 32 | プリセット名 |
| 0x4994 | 4 | f32 プリセット形式バージョン (例: 0.1631) |
| 0x49A0 | 48 | 作者 |
| 0x49D0 | 48 | カテゴリ |

### 検証 (Serum 2 が実際に受け入れることの確認)

1. **静的逆解析**: `Serum2.vst3` 2.0.23 の fxp インポート経路を逆アセンブルし、全チェック項目を列挙 → [docs/serum2-importer-analysis.md](docs/serum2-importer-analysis.md)
2. **実ファイル較正**: 公開リポジトリから収集した実物 Serum 1 fxp 25 ファイル (2015–2026 年) で形式を検証 → [docs/serum-fxp-format.md](docs/serum-fxp-format.md)
3. **動的検証**: 最小 VST3 ホストで実際の Serum2.vst3 を初期化し、`IComponent::setComponentState` に (a) Serum 2 ネイティブ状態 (受諾、サニティ)、(b) 本ツールが抽出した fxp (**受諾**、33,908 バイトの Serum 2 状態に変換)、(c) 同一プリセットの素の Serum 1 チャンク (受諾、(b) とバイト一致の結果状態) を与え、抽出物が本物の Serum 2 に読み込まれることを確認 → [docs/serum2-dynamic-verification.md](docs/serum2-dynamic-verification.md)

さらに、実 FLP からの抽出物すべてが `validate` で PASS することを確認しています。

## 制限

- **zipped loop package 非対応**: 先頭が `PK` の ZIP 梱包 FLP は読めません。中の .flp を先に展開してください
- **VST2 / VstW はベストエフォート**: VST2 ラッパー (`VstW`) 内の `CcnK` プリセットは探索して復元しますが、全レイアウトは検証していません
- **Serum 2 インスタンスは抽出しない**: 件数の報告のみ行います (Serum 2 は XferJson 状態を使うため対象外)

## テスト

```sh
cargo test
```

ユニットテスト (FLP パーサ / 状態解析 / fxp 構築・検証) に加え、合成 FLP からの `extract` → `validate` を実行する統合テスト (`tests/integration.rs`) と、実フィクスチャ fxp の検証テストを含みます。
