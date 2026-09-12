import { DownloadIcon, RefreshCwIcon } from "lucide-react"
import { useState } from "react"
import { toast } from "sonner"

import { ResultsCard, type ScanResult } from "@/components/results-card"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import {
  baseName,
  buildZip,
  downloadBlob,
  downloadConverted,
  formatBytes,
  presetFilename,
} from "@/lib/download"
import { convert, type ConvertOutcome, type FlpDocHandle, type Preset } from "@/lib/wasm"

export type ProjectEntry = {
  readonly id: string
  readonly result: ScanResult
  readonly doc: FlpDocHandle
}

export function ProjectSection(props: { project: ProjectEntry }) {
  const { project } = props
  const result = project.result
  const rows = result.presets
  const [selected, setSelected] = useState<ReadonlySet<number>>(new Set())
  const [converting, setConverting] = useState(false)
  const [converted, setConverted] = useState<ConvertOutcome | null>(null)

  const allSelected = rows.length > 0 && selected.size === rows.length
  const someSelected = selected.size > 0 && selected.size < rows.length

  const toMessage = (err: unknown, fallback: string) => {
    if (err instanceof Error) return err.message
    const text = String(err)
    return text.length > 0 ? text : fallback
  }

  const toggleRow = (index: number, checked: boolean) => {
    setSelected((prev) => {
      const next = new Set(prev)
      if (checked) next.add(index)
      else next.delete(index)
      return next
    })
  }

  const toggleAll = () => {
    setSelected((prev) =>
      prev.size === rows.length ? new Set() : new Set(rows.map((r) => r.index))
    )
  }

  const downloadOne = (preset: Preset) => {
    try {
      const bytes = project.doc.buildFxp(preset.index)
      const name = presetFilename(preset)
      downloadBlob(bytes, name)
      toast.success(`Downloaded ${name} (${formatBytes(bytes.length)})`)
    } catch (err) {
      toast.error(toMessage(err, "Failed to build the .fxp."))
    }
  }

  const downloadSelectedZip = () => {
    const chosen = rows.filter((r) => selected.has(r.index))
    if (chosen.length === 0) return
    try {
      const zip = buildZip((index) => project.doc.buildFxp(index), chosen)
      const name = `${baseName(result.fileName, "presets")}-selected.zip`
      downloadBlob(zip, name)
      toast.success(`Downloaded ${name} (${chosen.length} presets, ${formatBytes(zip.length)})`)
    } catch (err) {
      toast.error(toMessage(err, "Failed to build the ZIP."))
    }
  }

  const downloadZip = () => {
    if (rows.length === 0) return
    try {
      const zip = buildZip((index) => project.doc.buildFxp(index), rows)
      const name = `${baseName(result.fileName, "presets")}-fxp.zip`
      downloadBlob(zip, name)
      toast.success(`Downloaded ${name} (${rows.length} presets, ${formatBytes(zip.length)})`)
    } catch (err) {
      toast.error(toMessage(err, "Failed to build the ZIP."))
    }
  }

  const handleConvert = () => {
    setConverting(true)
    try {
      const outcome = convert(result.fileData)
      setConverted(outcome)
      downloadConverted(result.fileName, outcome)
      toast.success(
        `Converted ${outcome.convertedCount} Serum instance${outcome.convertedCount === 1 ? "" : "s"}`
      )
      for (const warning of outcome.warnings) {
        toast.warning(warning)
      }
    } catch (err) {
      toast.error(toMessage(err, "Failed to convert the file."))
    } finally {
      setConverting(false)
    }
  }

  const downloadConvertedAgain = () => {
    if (!converted) return
    downloadConverted(result.fileName, converted)
  }

  return (
    <>
      <ResultsCard
        result={result}
        selected={selected}
        allSelected={allSelected}
        someSelected={someSelected}
        onToggleAll={toggleAll}
        onToggleRow={toggleRow}
        onDownloadOne={downloadOne}
        onDownloadSelectedZip={downloadSelectedZip}
        onDownloadZip={downloadZip}
      />
      <Card>
        <CardContent className="flex flex-wrap items-center gap-2">
          <Button size="sm" disabled={converting || rows.length === 0} onClick={handleConvert}>
            <RefreshCwIcon /> Convert to Serum2
          </Button>
          {converted && (
            <>
              <Button size="sm" variant="secondary" onClick={downloadConvertedAgain}>
                <DownloadIcon /> Download converted
              </Button>
              <span className="text-muted-foreground text-sm">
                Converted {converted.convertedCount} Serum instance
                {converted.convertedCount === 1 ? "" : "s"} to Serum2 (warnings:{" "}
                {converted.warnings.length})
              </span>
            </>
          )}
        </CardContent>
      </Card>
    </>
  )
}
