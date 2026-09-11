import { scan_flp_report, type WasmPreset } from "flp-extract-fxp"

export { default as initWasm, build_fxp as buildFxp } from "flp-extract-fxp"

type PresetFields = Pick<WasmPreset, Exclude<Extract<keyof WasmPreset, string>, "free">>

export type Preset = PresetFields & {
  readonly warnings: string[]
  readonly errors: string[]
}

export function scan(data: Uint8Array): {
  presets: Preset[]
  failed: string[]
  serum2Skipped: number
} {
  const report = scan_flp_report(data)
  const presets: Preset[] = report.presets().map((p) => ({
    index: p.index,
    channel: p.channel,
    channel_name: p.channel_name,
    plugin_name: p.plugin_name,
    preset_name: p.preset_name,
    author: p.author,
    category: p.category,
    version_f32: p.version_f32,
    state_bytes: p.state_bytes,
    chunk_bytes: p.chunk_bytes,
    source: p.source,
    duplicate: p.duplicate,
    content_hash: p.content_hash,
    has_warnings: p.has_warnings,
    valid: p.valid,
    warnings_json: p.warnings_json,
    errors_json: p.errors_json,
    warnings: safeJsonArray(p.warnings_json),
    errors: safeJsonArray(p.errors_json),
  }))
  const failed = safeJsonArray(report.failed_json)
  const serum2Skipped = report.serum2_skipped
  report.free()
  return { presets, failed, serum2Skipped }
}

function safeJsonArray(json: string): string[] {
  if (!json) return []
  try {
    const parsed: unknown = JSON.parse(json)
    return Array.isArray(parsed) ? parsed.map(String) : []
  } catch {
    return []
  }
}
