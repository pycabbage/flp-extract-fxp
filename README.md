# flp-extract-fxp

FL Studio プロジェクトファイル (.flp) 内に埋め込まれた **Serum プリセット** を取り出して **Serum2 で読み込める .fxp** として出力し、さらに **FLP 内の Serum インスタンスを Serum2 インスタンスへ一括変換**できる CLI ツールです。

- FLP 内の Serum (VST3) プラグイン状態から Serum プリセットチャンクを復元
- Serum2 の Serum インポートチェック (静的逆解析、下記参照) を満たす fxp を生成
- 書き出し前にバリデーションし、Serum2 が拒否するファイルは既定で出力しない
- `convert`: FLP 内の Serum インスタンスを変換済み Serum2 インスタンスに書き換えた FLP を生成 (下記参照)

## ビルド

```sh
cargo build --release
```

実行ファイル: `target/release/flp-extract-fxp` (Windows では `.exe`)。依存は clap 4、flate2、md-5。

## 使い方

### `list` — Serum インスタンスの一覧

```sh
flp-extract-fxp list "path/to/project.flp"
```

Serum インスタンスごとに、チャンネル番号/チャンネル名/プラグイン名/プリセット名/作者/状態サイズ/埋め込みストリーム数/復元元を表示します。Serum2 インスタンスは件数のみ報告します。

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
| `--keep_invalid` | Serum2 バリデーションに失敗したプリセットも出力する (既定はスキップし、最後に終了コード 1 で報告) |

同一チャンクの重複プリセットは自動的にスキップします。抽出結果にはプリセット名/作者/カテゴリ/バージョンと埋め込みウェーブテーブルのサイズが出力されます。

### `validate` — fxp の検証

```sh
flp-extract-fxp validate preset.fxp
```

Serum2 の Serum インポータと同じ規則で検査し、`PASS` / `FAIL` とプリセット名を表示します。FAIL の場合は終了コード 1。拡張子が `.fxp` でない場合も FAIL になります (Serum2 は `.fxp` 名のときしかインポートを提示しないため)。

### `convert` — FLP 内の Serum を Serum2 へ一括変換

```sh
flp-extract-fxp convert "path/to/project.flp"
flp-extract-fxp convert --out converted.flp a.flp
flp-extract-fxp convert --dry-run a.flp
```

| フラグ | 意味 |
|---|---|
| `-o`, `--out <FILE>` | 出力 FLP パス。既定は `<入力名>_serum2.flp` (FLP と同じ場所)。単一入力時のみ指定可 |
| `--dry-run` | 変換計画だけ表示して書き込まない |

FLP 内の Serum (シンセ) インスタンスごとにプリセット状態を Serum2 形式へ完全変換し、プラグインスロットを Serum2 に書き換えます。変換後の FLP を FL Studio で開くと Serum2 が既に読み込まれた状態になり、手作業のプラグイン差し替え + fxp インポートが不要になります。ウェーブテーブルデータは変換後の状態に埋め込まれるため、追加ファイルは不要です。

再生には実際の Serum2 (VST3) のインストールが必要です。Web UI にも同じ変換があり、「Convert to Serum2」ボタンでブラウザ内で変換し `<名前>-serum2.flp` としてダウンロードできます。パイプラインと検証方法の詳細は [docs/flp-conversion.md](docs/flp-conversion.md) を参照してください。

## 出力ファイル名

`NN_プリセット名.fxp` 形式 (`NN` は出力順の連番、01 始まり)。プリセット名は状態内の名前フィールド (下表オフセット 0x4972)。空の場合はチャンネル名、それも空なら `Serum N` を使用します。同一ベース名は `NN_名_2.fxp` のように接尾番号を付けます。ファイル名に使えない文字 (`/\:*?"<>|`、制御文字) は `_` に置換し、前後の空白とドットを除去して最大 80 文字に切り詰めます。

## 技術概要

### FLP 内の Serum 状態 (イベント 213 → チャンク ID 53)

| 構造 | 内容 |
|---|---|
| FLP コンテナ | `FLhd` (u32 LE 長 + ヘッダ) + `FLdt` (u32 LE 長 + イベント列) |
| イベント | 1 バイト ID + データ。0–63: 1 バイト / 64–127: 2 バイト (LE) / 128–191: 4 バイト (LE) / 192 以上: varint 長 + データ |
| イベント 64 (NewChan) | 直後のチャンネル名イベントの所有チャンネル (u16 LE) |
| イベント 203 / 204 | チャンネル名 / FX トラック名 (UTF-8 または UTF-16LE) |
| イベント 213 (PluginParams) | u32 LE バージョン + `[u32 cid][u64 サイズ][データ]` 列。ID 53=状態 / 54=名前 / 55=ファイル名 / 56=ベンダ |
| チャンク 53 (State) | FL の VST3 ラッパー状態: プロローグ `u32 = 1` + `[u32 cid][u64 サイズ]` チャンク列。**cid = 3 がプラグイン本体の保存状態** |

cid = 3 のペイロード (Serum の場合) は fxp の `chunk` 領域そのもので、次の構成です:

```
[zlib ストリーム 0: プリセット状態 172,736 バイト]
[zlib ストリーム 1...: 埋め込みウェーブテーブル / ノイズ / フィルタテーブル]
[u32 LE トレーラ: ストリーム 0 の圧縮後サイズ]
```

Serum2 の状態 (cid = 3 が `XferJson...` で始まる) は抽出対象外です。

### Serum fxp 形式 (60 バイトヘッダ、マルチバイトはビッグエンディアン)

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

### 検証 (Serum2 が実際に受け入れることの確認)

1. **静的逆解析**: `Serum2.vst3` 2.0.23 の fxp インポート経路を逆アセンブルし、全チェック項目を列挙 → [docs/serum2-importer-analysis.md](docs/serum2-importer-analysis.md)
2. **実ファイル較正**: 公開リポジトリから収集した実物 Serum fxp 25 ファイル (2015–2026 年) で形式を検証 → [docs/serum-fxp-format.md](docs/serum-fxp-format.md)
3. **動的検証**: 最小 VST3 ホストで実際の Serum2.vst3 を初期化して状態を検査。`IComponent::setComponentState` は Serum2 ネイティブ状態を受諾するが (サニティ)、Serum の fxp/チャンクを与えると **kResultFalse で拒否し、状態は無変化** です (初回実験の「受諾」は誤判定 — 同一コンポーネントに先行して読み込んだネイティブ状態の再シリアライズだった。訂正の全文は [docs/serum2-dynamic-verification.md](docs/serum2-dynamic-verification.md) 冒頭)。Serum → Serum2 の実変換経路は内部関数 `s1state_load` (RVA 0x4DABC0) で、これをハーネスから直接呼び出して得た 5 プリセット分の変換結果はすべて実機の `setState` で受諾され、ロード後の状態が再インポート結果とバイト一致しました → [docs/s1-to-s2-mapping.md](docs/s1-to-s2-mapping.md)。`convert` 機能の検証 (変換結果と実インポータ出力のバイト一致 + 実機受諾) は [docs/flp-conversion.md](docs/flp-conversion.md) を参照してください。

さらに、実 FLP からの抽出物すべてが `validate` で PASS することを確認しています。

## 制限

- **zipped loop package 非対応**: 先頭が `PK` の ZIP 梱包 FLP は読めません。中の .flp を先に展開してください
- **VST2 / VstW はベストエフォート**: VST2 ラッパー (`VstW`) 内の `CcnK` プリセットは探索して復元しますが、全レイアウトは検証していません
- **Serum2 インスタンスは抽出しない**: 件数の報告のみ行います (Serum2 は XferJson 状態を使うため対象外)
- **`convert` の制限**: 新形式の Serum プリセット (172,736 バイト状態) のみ変換。旧形式 (2015 年頃) のプリセットは変換せず報告、Serum FX インスタンスは対象外。詳細は [docs/flp-conversion.md](docs/flp-conversion.md)

## テスト

```sh
cargo test
```

ユニットテスト (FLP パーサ / 状態解析 / fxp 構築・検証) に加え、合成 FLP からの `extract` → `validate` を実行する統合テスト (`tests/integration.rs`) と、実フィクスチャ fxp の検証テストを含みます。

`convert` は 5 プリセット分の golden 変換状態 (`tests/fixtures/golden_s2/`、実インポータが生成したもの) とのバイト一致テストと、実プロジェクト (`tests/fixtures/serina1.flp`) を変換した FLP の再スキャン / 差分テストで検証します。
