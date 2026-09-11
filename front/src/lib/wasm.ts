import init, { build_fxp, scan_flp_report } from "flp-extract-fxp"

export type WasmPreset = {
  readonly index: number
  readonly channel: string
  readonly channel_name: string
  readonly plugin_name: string
  readonly preset_name: string
  readonly author: string
  readonly category: string
  readonly version_f32: number
  readonly state_bytes: number
  readonly chunk_bytes: number
  readonly source: string
  readonly duplicate: boolean
  readonly content_hash: string
  readonly has_warnings: boolean
  readonly valid: boolean
  readonly warnings_json: string
  readonly errors_json: string
}

export type Preset = WasmPreset & {
  readonly warnings: string[]
  readonly errors: string[]
}

let ready: Promise<void> | null = null

export function initWasm(): Promise<void> {
  ready ??= init().then(() => undefined)
  return ready
}

export function scan(data: Uint8Array): {
  presets: Preset[]
  failed: string[]
  serum2Skipped: number
} {
  const report = scan_flp_report(data)
  // Field getters live on the prototype, so a plain object spread would
  // produce undefined values - map every field explicitly.
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

export function buildFxp(data: Uint8Array, index: number): Uint8Array {
  return build_fxp(data, index)
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
