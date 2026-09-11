# 0001: `WasmPreset` のフィールドを明示的にマッピングする

## Status

Accepted

## Context

`front/src/lib/wasm.ts` の `scan()` は、wasm-bindgen が生成した `WasmPreset`
クラスのインスタンス配列 (`report.presets()`) を、`ScanReport.free()` 呼び出し後も
安全に保持できるプレーンオブジェクト (`Preset[]`) に変換する必要がある。

`WasmPreset` の各フィールドは `readonly` な**プロトタイプ上のゲッター**として
生成されるため (`front/pkg/flp_extract_fxp.d.ts` 参照)、`{ ...p }` のような
オブジェクトスプレッドや `Object.assign({}, p)` は `p` 自身の own property を
列挙するだけで、プロトタイプ上のゲッターを呼び出さない。そのため、スプレッドで
生成したオブジェクトは全フィールドが `undefined` になる。

## Decision

`scan()` 内で `WasmPreset` の各フィールドを一つずつ明示的に読み出し、新しい
プレーンオブジェクトを組み立てる (`front/src/lib/wasm.ts` の `presets.map` 参照)。
併せて、フィールドの型はライブラリが生成した `WasmPreset` 型から
`Pick<WasmPreset, ...>` で導出し、型を手書きで複製しない
(`front/AGENTS.md` の「ライブラリが提供する型を自前で書き写さないこと」に準拠)。

## Consequences

- `front/pkg/` (wasm-bindgen の生成物) 側でフィールドの型が変わった場合、
  `Pick<WasmPreset, ...>` 経由で型エラーとして検出できる。
- フィールド名が変わった場合は `presets.map` 内の該当行がコンパイルエラーになる
  ため、追従漏れが起きにくい。
- オブジェクトスプレッドに戻すと全フィールドが `undefined` になる不具合を
  再導入するため、行わないこと。
