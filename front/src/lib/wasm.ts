import { convert_flp, scan_doc, type FlpDoc, type WasmPreset } from "flp-extract-fxp"

export { default as initWasm } from "flp-extract-fxp"

type PresetFields = Pick<WasmPreset, Exclude<Extract<keyof WasmPreset, string>, "free">>

export type Preset = PresetFields & {
  readonly warnings: string[]
  readonly errors: string[]
}

export type FlpDocHandle = {
  readonly presets: Preset[]
  readonly failedJson: string[]
  readonly serum2Skipped: number
  buildFxp: (index: number) => Uint8Array
  free: () => void
}

function toPreset(p: WasmPreset): Preset {
  return {
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
  }
}

export function scanDoc(data: Uint8Array): FlpDocHandle {
  const doc: FlpDoc = scan_doc(data)
  const presets: Preset[] = doc.presets().map(toPreset)
  const failedJson = safeJsonArray(doc.failed_json)
  const serum2Skipped = doc.serum2_skipped
  return {
    presets,
    failedJson,
    serum2Skipped,
    buildFxp: (index: number) => doc.build_fxp(index),
    free: () => {
      doc.free()
    },
  }
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

export type ConvertDetail = {
  readonly channel: string
  readonly channelName: string
  readonly presetName: string
  readonly payloadLen: number
  readonly notes: string[]
}

export type ConvertedDoc = {
  readonly name: string
  readonly data: Uint8Array
}

export type ConvertOutcome = {
  readonly convertedCount: number
  readonly flp: Uint8Array
  readonly docs: ConvertedDoc[]
  readonly warnings: string[]
  readonly details: ConvertDetail[]
}

export function convert(data: Uint8Array): ConvertOutcome {
  const report = convert_flp(data)
  const indexes = Array.from({ length: report.doc_count }, (_, index) => index)
  const docs: ConvertedDoc[] = indexes.map((index) => ({
    name: report.doc_name_at(index),
    data: report.flp_at(index),
  }))
  const outcome = {
    convertedCount: report.converted_count,
    flp: report.flp(),
    docs,
    warnings: safeJsonArray(report.warnings_json),
    details: safeDetailArray(report.details_json),
  }
  report.free()
  return outcome
}

function str(value: unknown): string {
  return typeof value === "string" ? value : ""
}

function payloadLen(value: unknown): number {
  return typeof value === "number" && Number.isFinite(value) ? value : 0
}

function safeDetailArray(json: string): ConvertDetail[] {
  if (!json) return []
  try {
    const parsed: unknown = JSON.parse(json)
    if (!Array.isArray(parsed)) return []
    const details: ConvertDetail[] = []
    for (const entry of parsed) {
      if (typeof entry !== "object" || entry === null) continue
      if (!("channel" in entry)) continue
      if (!("channelName" in entry)) continue
      if (!("presetName" in entry)) continue
      if (!("payloadLen" in entry)) continue
      if (!("notes" in entry)) continue
      details.push({
        channel: str(entry.channel),
        channelName: str(entry.channelName),
        presetName: str(entry.presetName),
        payloadLen: payloadLen(entry.payloadLen),
        notes: Array.isArray(entry.notes) ? entry.notes.map(String) : [],
      })
    }
    return details
  } catch {
    return []
  }
}
