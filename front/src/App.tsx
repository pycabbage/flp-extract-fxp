import { AlertCircleIcon, ExternalLinkIcon } from "lucide-react"
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
import { Toaster } from "@/components/ui/sonner"
import { TooltipProvider } from "@/components/ui/tooltip"
import { UploadCard } from "@/components/upload-card"
import { buildZip, downloadBlob, formatBytes, presetFilename } from "@/lib/download"
import { buildFxp, initWasm, scan, type Preset } from "@/lib/wasm"

import { ThemeProvider } from "./components/theme-provider"

export default function App() {
  const [result, setResult] = useState<ScanResult | null>(null)
  const [selected, setSelected] = useState<ReadonlySet<number>>(new Set())
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [fatalError, setFatalError] = useState<string | null>(null)
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
    try {
      await initWasm()
      const data = new Uint8Array(await file.arrayBuffer())
      const report = scan(data)
      const unique = report.presets.filter((p) => !p.duplicate)
      const duplicates = report.presets.length - unique.length
      setResult({
        fileName: file.name,
        fileData: data,
        presets: unique,
        duplicates,
        serum2Skipped: report.serum2Skipped,
        failed: report.failed,
      })
      if (unique.length === 0 && report.failed.length > 0) {
        toast.error(`No presets extracted from ${file.name}`)
      } else {
        toast.success(
          `Loaded ${unique.length} preset${unique.length === 1 ? "" : "s"}` +
            (duplicates > 0
              ? ` (${duplicates} duplicate${duplicates === 1 ? "" : "s"} ignored)`
              : "")
        )
      }
      for (const message of report.failed) {
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
    if (!result) return
    try {
      const bytes = buildFxp(result.fileData, preset.index)
      const name = presetFilename(preset)
      downloadBlob(bytes, name)
      toast.success(`Downloaded ${name} (${formatBytes(bytes.length)})`)
    } catch (err) {
      const message = err instanceof Error ? err.message : "Failed to build the .fxp."
      toast.error(message)
    }
  }

  const downloadSelectedZip = () => {
    if (!result) return
    const chosen = rows.filter((r) => selected.has(r.index))
    if (chosen.length === 0) return
    try {
      const zip = buildZip(result.fileData, chosen)
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
    if (!result) return
    if (rows.length === 0) return
    try {
      const zip = buildZip(result.fileData, rows)
      const base = result.fileName.replace(/\.[^.]+$/, "") || "presets"
      const name = `${base}-fxp.zip`
      downloadBlob(zip, name)
      toast.success(`Downloaded ${name} (${rows.length} presets, ${formatBytes(zip.length)})`)
    } catch (err) {
      const message = err instanceof Error ? err.message : "Failed to build the ZIP."
      toast.error(message)
    }
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

          <footer className="text-muted-foreground mt-auto pt-4 text-center text-xs">
            Runs fully client-side — your .flp file never leaves the browser.
          </footer>
        </div>
        <Toaster richColors position="bottom-right" />
      </ThemeProvider>
    </TooltipProvider>
  )
}
