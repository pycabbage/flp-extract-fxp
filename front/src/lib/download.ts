import { zipSync } from "fflate"

import { type ConvertedDoc, type ConvertOutcome, type Preset } from "./wasm"

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

export function convertedFlpFilename(fileName: string): string {
  const base = fileName.replace(/\.[^.]+$/, "") || "project"
  return `${base}-serum2.flp`
}

export function convertedZipFilename(fileName: string): string {
  const base = fileName.replace(/\.[^.]+$/, "") || "project"
  return `${base}-serum2.zip`
}

function uniqueName(base: string, taken: Set<string>, total: number): string {
  const dot = base.lastIndexOf(".")
  const stem = dot > 0 ? base.slice(0, dot) : base
  const ext = dot > 0 ? base.slice(dot) : ""
  const candidates = Array.from({ length: total + 1 }, (_, index) =>
    index === 0 ? base : `${stem}-${index + 1}${ext}`
  )
  return candidates.find((candidate) => !taken.has(candidate)) ?? `${stem}-extra${ext}`
}

export function buildConvertedZip(docs: ConvertedDoc[]): Uint8Array {
  const names = new Map<string, string>()
  const entries: Record<string, Uint8Array> = {}
  for (const doc of docs) {
    const name = names.has(doc.name)
      ? uniqueName(doc.name, new Set(names.values()), docs.length)
      : doc.name
    names.set(doc.name, name)
    entries[name] = doc.data
  }
  return zipSync(entries)
}

export function downloadConverted(fileName: string, outcome: ConvertOutcome): void {
  if (outcome.docs.length > 1) {
    const zip = buildConvertedZip(outcome.docs)
    downloadBlob(zip, convertedZipFilename(fileName))
    return
  }
  downloadBlob(outcome.flp, convertedFlpFilename(fileName))
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

export function buildZip(buildFxp: (index: number) => Uint8Array, presets: Preset[]): Uint8Array {
  const entries: Record<string, Uint8Array> = {}
  for (const preset of presets) {
    const defaultName = presetFilename(preset)
    const name =
      defaultName in entries
        ? `${preset.index}-${sanitizeFilename(
            preset.preset_name || preset.plugin_name || "preset"
          )}-${preset.content_hash.slice(0, 8)}.fxp`
        : defaultName
    entries[name] = buildFxp(preset.index)
  }
  return zipSync(entries)
}
