import { AlertCircleIcon, DownloadIcon, ExternalLinkIcon, RefreshCwIcon } from "lucide-react"
import { useRef, useState } from "react"
import { toast } from "sonner"

import { EmptyState } from "@/components/empty-state"
import { ResultsCard, type ScanResult } from "@/components/results-card"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import { Toaster } from "@/components/ui/sonner"
import { TooltipProvider } from "@/components/ui/tooltip"
import { UploadCard } from "@/components/upload-card"
import {
  buildZip,
  convertedFlpFilename,
  downloadBlob,
  formatBytes,
  presetFilename,
} from "@/lib/download"
import {
  convert,
  initWasm,
  scanDoc,
  type ConvertOutcome,
  type FlpDocHandle,
  type Preset,
} from "@/lib/wasm"

import { ThemeProvider } from "./components/theme-provider"

export default function App() {
  const [result, setResult] = useState<ScanResult | null>(null)
  const [doc, setDoc] = useState<FlpDocHandle | null>(null)
  const [selected, setSelected] = useState<ReadonlySet<number>>(new Set())
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [fatalError, setFatalError] = useState<string | null>(null)
  const [converting, setConverting] = useState(false)
  const [converted, setConverted] = useState<ConvertOutcome | null>(null)
  const inputRef = useRef<HTMLInputElement>(null)
  const [dragging, setDragging] = useState(false)

  const rows = result?.presets ?? []
  const allSelected = rows.length > 0 && selected.size === rows.length
  const someSelected = selected.size > 0 && selected.size < rows.length

  const handleFile = async (file: File) => {
    setLoading(true)
    setError(null)
    setFatalError(null)
    setSelected(new Set())
    setConverted(null)
    try {
      await initWasm()
      const data = new Uint8Array(await file.arrayBuffer())
      const scanned = scanDoc(data)
      doc?.free()
      setDoc(scanned)
      const unique = scanned.presets.filter((p) => !p.duplicate)
      const duplicates = scanned.presets.length - unique.length
      setResult({
        fileName: file.name,
        fileData: data,
        presets: unique,
        duplicates,
        serum2Skipped: scanned.serum2Skipped,
        failed: scanned.failedJson,
      })
      if (unique.length === 0 && scanned.failedJson.length > 0) {
        toast.error(`No presets extracted from ${file.name}`)
      } else {
        toast.success(
          `Loaded ${unique.length} preset${unique.length === 1 ? "" : "s"}` +
            (duplicates > 0
              ? ` (${duplicates} duplicate${duplicates === 1 ? "" : "s"} ignored)`
              : "")
        )
      }
      for (const message of scanned.failedJson) {
        toast.warning(message)
      }
    } catch (err) {
      const message = err instanceof Error ? err.message : "Failed to scan the file."
      setError(message)
      toast.error(message)
    } finally {
      setLoading(false)
    }
  }

  const onInputChange = (event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0]
    if (file) void handleFile(file)
    event.target.value = ""
  }

  const onDrop = (event: React.DragEvent<HTMLDivElement>) => {
    event.preventDefault()
    setDragging(false)
    const file = event.dataTransfer.files?.[0]
    if (file) void handleFile(file)
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
    if (!doc) return
    try {
      const bytes = doc.buildFxp(preset.index)
      const name = presetFilename(preset)
      downloadBlob(bytes, name)
      toast.success(`Downloaded ${name} (${formatBytes(bytes.length)})`)
    } catch (err) {
      const message = err instanceof Error ? err.message : "Failed to build the .fxp."
      toast.error(message)
    }
  }

  const downloadSelectedZip = () => {
    if (!result || !doc) return
    const chosen = rows.filter((r) => selected.has(r.index))
    if (chosen.length === 0) return
    try {
      const zip = buildZip((index) => doc.buildFxp(index), chosen)
      const base = result.fileName.replace(/\.[^.]+$/, "") || "presets"
      const name = `${base}-selected.zip`
      downloadBlob(zip, name)
      toast.success(`Downloaded ${name} (${chosen.length} presets, ${formatBytes(zip.length)})`)
    } catch (err) {
      const message = err instanceof Error ? err.message : "Failed to build the ZIP."
      toast.error(message)
    }
  }

  const downloadZip = () => {
    if (!result || !doc) return
    if (rows.length === 0) return
    try {
      const zip = buildZip((index) => doc.buildFxp(index), rows)
      const base = result.fileName.replace(/\.[^.]+$/, "") || "presets"
      const name = `${base}-fxp.zip`
      downloadBlob(zip, name)
      toast.success(`Downloaded ${name} (${rows.length} presets, ${formatBytes(zip.length)})`)
    } catch (err) {
      const message = err instanceof Error ? err.message : "Failed to build the ZIP."
      toast.error(message)
    }
  }

  const handleConvert = () => {
    if (!result) return
    setConverting(true)
    try {
      const outcome = convert(result.fileData)
      setConverted(outcome)
      const name = convertedFlpFilename(result.fileName)
      downloadBlob(outcome.flp, name)
      toast.success(
        `Converted ${outcome.convertedCount} Serum 1 instance${outcome.convertedCount === 1 ? "" : "s"}`
      )
      for (const warning of outcome.warnings) {
        toast.warning(warning)
      }
    } catch (err) {
      const message = err instanceof Error ? err.message : "Failed to convert the file."
      toast.error(message)
    } finally {
      setConverting(false)
    }
  }

  const downloadConverted = () => {
    if (!result || !converted) return
    const name = convertedFlpFilename(result.fileName)
    downloadBlob(converted.flp, name)
    toast.success(`Downloaded ${name} (${formatBytes(converted.flp.length)})`)
  }

  return (
    <TooltipProvider>
      <ThemeProvider>
        <div className="mx-auto flex min-h-dvh w-full max-w-5xl flex-col gap-6 px-4 py-8">
          <header className="flex flex-col gap-2">
            <div className="flex items-center justify-between">
              <h1 className="text-2xl font-semibold tracking-tight">FLP Extract FXP</h1>
              <Button variant="ghost" size="sm" asChild>
                <a
                  href="https://github.com/pycabbage/flp-extract-fxp"
                  target="_blank"
                  rel="noreferrer"
                >
                  <ExternalLinkIcon /> GitHub
                </a>
              </Button>
            </div>
            <p className="text-muted-foreground max-w-2xl text-sm">
              Extract Serum presets embedded in FL Studio project (.flp) files and download them as
              Serum2-loadable .fxp files — entirely in your browser.
            </p>
          </header>

          <UploadCard
            loading={loading}
            dragging={dragging}
            onDragging={setDragging}
            onPick={() => inputRef.current?.click()}
            onDrop={onDrop}
          />
          <input
            ref={inputRef}
            type="file"
            accept=".flp"
            className="hidden"
            onChange={onInputChange}
          />

          {error && (
            <Alert variant="destructive">
              <AlertCircleIcon />
              <AlertTitle>Scan failed</AlertTitle>
              <AlertDescription>{error}</AlertDescription>
            </Alert>
          )}

          {fatalError && (
            <AlertDialog
              open
              onOpenChange={(open) => {
                if (!open) setFatalError(null)
              }}
            >
              <AlertDialogContent>
                <AlertDialogHeader>
                  <AlertDialogTitle>Unexpected error</AlertDialogTitle>
                  <AlertDialogDescription>{fatalError}</AlertDialogDescription>
                </AlertDialogHeader>
                <AlertDialogFooter>
                  <AlertDialogAction>OK</AlertDialogAction>
                </AlertDialogFooter>
              </AlertDialogContent>
            </AlertDialog>
          )}

          {result === null && !loading && !error && <EmptyState />}

          {result && (
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
          )}

          {result && (
            <Card>
              <CardContent className="flex flex-wrap items-center gap-2">
                <Button
                  size="sm"
                  disabled={converting || rows.length === 0}
                  onClick={handleConvert}
                >
                  <RefreshCwIcon /> Convert to Serum 2
                </Button>
                {converted && (
                  <>
                    <Button size="sm" variant="secondary" onClick={downloadConverted}>
                      <DownloadIcon /> Download converted .flp
                    </Button>
                    <span className="text-muted-foreground text-sm">
                      Converted {converted.convertedCount} Serum 1 instance
                      {converted.convertedCount === 1 ? "" : "s"} to Serum 2 (warnings:{" "}
                      {converted.warnings.length})
                    </span>
                  </>
                )}
              </CardContent>
            </Card>
          )}

          <footer className="text-muted-foreground mt-auto pt-4 text-center text-xs">
            Runs fully client-side — your .flp file never leaves the browser.
          </footer>
        </div>
        <Toaster richColors position="bottom-right" />
      </ThemeProvider>
    </TooltipProvider>
  )
}
