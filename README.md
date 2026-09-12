# flp-extract-fxp

FL Studio プロジェクトファイル (.flp) 内に埋め込まれた **Serum プリセット** を取り出して **Serum2 で読み込める .fxp** として出力し、さらに **FLP 内の Serum インスタンスを Serum2 インスタンスへ一括変換**できる CLI ツールです。

- FLP 内の Serum (VST3) プラグイン状態から Serum プリセットチャンクを復元
- Serum2 の Serum インポートチェック (静的逆解析、下記参照) を満たす fxp を生成
- 書き出し前にバリデーションし、Serum2 が拒否するファイルは既定で出力しない
- `convert`: FLP 内の Serum インスタンスを変換済み Serum2 インスタンスに書き換えた FLP を生成 (下記参照)
- `convert-fxp`: 単体の Serum .fxp プリセットを Serum2 の .SerumPreset ファイルへ変換 (実験的、下記参照)
- `patch`: fxp (または FLP 内の全 Serum インスタンス) のプリセット名 / 作者 / カテゴリを書き換え (下記参照)
