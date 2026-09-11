import { zipSync } from "fflate"

import { buildFxp, type Preset } from "./wasm"

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(2)} MB`
}

const RESERVED_FILENAME_CHARS = /[<>:"/\\|?*]/

export function sanitizeFilename(name: string): string {
  const cleaned = Array.from(name)
    .map((char) => (RESERVED_FILENAME_CHARS.test(char) || char.charCodeAt(0) <= 0x1f ? "_" : char))
    .join("")
    .trim()
  return cleaned.length > 0 ? cleaned.slice(0, 120) : "preset"
}

export function presetFilename(preset: Preset): string {
  const base =
    preset.preset_name || preset.plugin_name || preset.channel_name || preset.channel || "preset"
  return `${preset.index}-${sanitizeFilename(base)}.fxp`
}

export function downloadBlob(bytes: Uint8Array, filename: string): void {
  const url = URL.createObjectURL(new Blob([bytes.slice()], { type: "application/octet-stream" }))
  const anchor = document.createElement("a")
  anchor.href = url
  anchor.download = filename
  document.body.appendChild(anchor)
  anchor.click()
  anchor.remove()
  URL.revokeObjectURL(url)
}

export function buildZip(fileData: Uint8Array, presets: Preset[]): Uint8Array {
  const entries: Record<string, Uint8Array> = {}
  for (const preset of presets) {
    const defaultName = presetFilename(preset)
    const name =
      defaultName in entries
        ? `${preset.index}-${sanitizeFilename(
            preset.preset_name || preset.plugin_name || "preset"
          )}-${preset.content_hash.slice(0, 8)}.fxp`
        : defaultName
    entries[name] = buildFxp(fileData, preset.index)
  }
  return zipSync(entries)
}
