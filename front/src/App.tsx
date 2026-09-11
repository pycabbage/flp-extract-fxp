import {
  AlertCircleIcon,
  DownloadIcon,
  ExternalLinkIcon,
  FileAudioIcon,
  PackageOpenIcon,
  TriangleAlertIcon,
  UploadIcon,
} from "lucide-react"
import { useCallback, useMemo, useRef, useState } from "react"
import { toast } from "sonner"

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
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Checkbox } from "@/components/ui/checkbox"
import { Progress } from "@/components/ui/progress"
import { Toaster } from "@/components/ui/sonner"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip"
import { buildZip, downloadBlob, formatBytes, presetFilename } from "@/lib/download"
import { buildFxp, initWasm, scan, type Preset } from "@/lib/wasm"

type ScanResult = {
  fileName: string
  fileData: Uint8Array
  presets: Preset[]
  duplicates: number
  serum2Skipped: number
  failed: string[]
}

export default function App() {
  const [result, setResult] = useState<ScanResult | null>(null)
  const [selected, setSelected] = useState<ReadonlySet<number>>(new Set())
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [fatalError, setFatalError] = useState<string | null>(null)
  const inputRef = useRef<HTMLInputElement>(null)
  const [dragging, setDragging] = useState(false)

  const rows = useMemo(() => result?.presets ?? [], [result])
  const allSelected = rows.length > 0 && selected.size === rows.length
  const someSelected = selected.size > 0 && selected.size < rows.length

  const handleFile = useCallback(async (file: File) => {
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
  }, [])

  const onInputChange = useCallback(
    (event: React.ChangeEvent<HTMLInputElement>) => {
      const file = event.target.files?.[0]
      if (file) void handleFile(file)
      event.target.value = ""
    },
    [handleFile]
  )

  const onDrop = useCallback(
    (event: React.DragEvent<HTMLDivElement>) => {
      event.preventDefault()
      setDragging(false)
      const file = event.dataTransfer.files?.[0]
      if (file) void handleFile(file)
    },
    [handleFile]
  )

  const toggleRow = useCallback((index: number, checked: boolean) => {
    setSelected((prev) => {
      const next = new Set(prev)
      if (checked) next.add(index)
      else next.delete(index)
      return next
    })
  }, [])

  const toggleAll = useCallback(() => {
    setSelected((prev) =>
      prev.size === rows.length ? new Set() : new Set(rows.map((r) => r.index))
    )
  }, [rows])

  const downloadOne = useCallback(
    (preset: Preset) => {
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
    },
    [result]
  )

  const downloadSelectedZip = useCallback(() => {
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
  }, [result, rows, selected])

  const downloadZip = useCallback(() => {
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
  }, [result, rows])

  return (
    <TooltipProvider>
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
    </TooltipProvider>
  )
}

function UploadCard(props: {
  loading: boolean
  dragging: boolean
  onDragging: (dragging: boolean) => void
  onPick: () => void
  onDrop: (event: React.DragEvent<HTMLDivElement>) => void
}) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>Load a project file</CardTitle>
        <CardDescription>
          Drop an .flp file below or pick one from disk. The scan looks for Serum plugin instances
          and extracts their preset state.
        </CardDescription>
      </CardHeader>
      <CardContent>
        <div
          role="button"
          tabIndex={0}
          aria-label="Upload .flp file"
          onClick={() => {
            if (!props.loading) props.onPick()
          }}
          onKeyDown={(event) => {
            if (!props.loading && (event.key === "Enter" || event.key === " ")) {
              event.preventDefault()
              props.onPick()
            }
          }}
          onDragOver={(event) => {
            event.preventDefault()
            props.onDragging(true)
          }}
          onDragLeave={() => props.onDragging(false)}
          onDrop={props.onDrop}
          className={`flex min-h-44 cursor-pointer flex-col items-center justify-center gap-3 rounded-lg border-2 border-dashed p-6 text-center transition-colors outline-none focus-visible:ring-2 focus-visible:ring-ring ${
            props.dragging
              ? "border-primary bg-primary/5"
              : "border-muted-foreground/25 hover:border-primary/50 hover:bg-muted/40"
          }`}
        >
          {props.loading ? (
            <div className="flex w-full max-w-sm flex-col items-center gap-3">
              <UploadIcon className="text-muted-foreground size-8 animate-pulse" />
              <p className="text-muted-foreground text-sm font-medium">Scanning file…</p>
              <Progress value={null} className="w-full" />
            </div>
          ) : (
            <>
              <FileAudioIcon className="text-muted-foreground size-8" />
              <div className="space-y-1">
                <p className="text-sm font-medium">Drag &amp; drop your .flp file here</p>
                <p className="text-muted-foreground text-xs">
                  or click to browse — files are processed locally
                </p>
              </div>
              <Button
                type="button"
                onClick={(event) => {
                  event.stopPropagation()
                  props.onPick()
                }}
              >
                <UploadIcon /> Select .flp file
              </Button>
            </>
          )}
        </div>
      </CardContent>
    </Card>
  )
}

function EmptyState() {
  return (
    <Alert>
      <PackageOpenIcon />
      <AlertTitle>Nothing scanned yet</AlertTitle>
      <AlertDescription>
        Load an .flp file above to see the Serum presets it contains.
      </AlertDescription>
    </Alert>
  )
}

function ResultsCard(props: {
  result: ScanResult
  selected: ReadonlySet<number>
  allSelected: boolean
  someSelected: boolean
  onToggleAll: () => void
  onToggleRow: (index: number, checked: boolean) => void
  onDownloadOne: (preset: Preset) => void
  onDownloadSelectedZip: () => void
  onDownloadZip: () => void
}) {
  const { result } = props
  return (
    <Card>
      <CardHeader>
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div>
            <CardTitle>Presets in {result.fileName}</CardTitle>
            <CardDescription>
              {result.presets.length} unique preset
              {result.presets.length === 1 ? "" : "s"} found
            </CardDescription>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            {result.duplicates > 0 && (
              <Badge variant="secondary">
                {result.duplicates} duplicate
                {result.duplicates === 1 ? "" : "s"} ignored
              </Badge>
            )}
            {result.serum2Skipped > 0 && (
              <Badge variant="outline">{result.serum2Skipped} Serum2 skipped</Badge>
            )}
          </div>
        </div>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        {result.failed.length > 0 && (
          <Alert variant="destructive">
            <TriangleAlertIcon />
            <AlertTitle>
              {result.failed.length} instance
              {result.failed.length === 1 ? "" : "s"} failed to convert
            </AlertTitle>
            <AlertDescription>
              <ul className="list-disc pl-4">
                {result.failed.map((message, i) => (
                  <li key={i}>{message}</li>
                ))}
              </ul>
            </AlertDescription>
          </Alert>
        )}

        <div className="flex flex-wrap items-center gap-2">
          <Button size="sm" disabled={result.presets.length === 0} onClick={props.onDownloadZip}>
            <DownloadIcon /> Download all (ZIP)
          </Button>
          <Button
            size="sm"
            variant="secondary"
            disabled={props.selected.size === 0}
            onClick={props.onDownloadSelectedZip}
          >
            <DownloadIcon /> Download selected (ZIP)
          </Button>
        </div>

        <div className="rounded-md border">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="w-10">
                  <Checkbox
                    aria-label="Select all presets"
                    checked={
                      props.allSelected ? true : props.someSelected ? "indeterminate" : false
                    }
                    onCheckedChange={() => props.onToggleAll()}
                  />
                </TableHead>
                <TableHead className="w-12">#</TableHead>
                <TableHead>Preset</TableHead>
                <TableHead>Channel</TableHead>
                <TableHead>Author</TableHead>
                <TableHead>Category</TableHead>
                <TableHead>Version</TableHead>
                <TableHead className="text-right">Size</TableHead>
                <TableHead>Status</TableHead>
                <TableHead className="w-10" />
              </TableRow>
            </TableHeader>
            <TableBody>
              {result.presets.map((preset) => (
                <TableRow key={preset.index}>
                  <TableCell>
                    <Checkbox
                      aria-label={`Select ${preset.preset_name || preset.plugin_name}`}
                      checked={props.selected.has(preset.index)}
                      onCheckedChange={(checked) =>
                        props.onToggleRow(preset.index, checked === true)
                      }
                    />
                  </TableCell>
                  <TableCell className="text-muted-foreground">{preset.index}</TableCell>
                  <TableCell className="font-medium">{preset.preset_name || "(unnamed)"}</TableCell>
                  <TableCell>{preset.channel_name || preset.channel}</TableCell>
                  <TableCell>{preset.author || "—"}</TableCell>
                  <TableCell>{preset.category || "—"}</TableCell>
                  <TableCell>
                    <Badge variant="outline" className="tabular-nums">
                      v{preset.version_f32.toFixed(4)}
                    </Badge>
                  </TableCell>
                  <TableCell className="text-right tabular-nums">
                    {formatBytes(preset.state_bytes)}
                  </TableCell>
                  <TableCell>
                    <PresetStatus preset={preset} />
                  </TableCell>
                  <TableCell>
                    <Tooltip>
                      <TooltipTrigger asChild>
                        <Button
                          variant="ghost"
                          size="icon"
                          className="size-7"
                          aria-label={`Download ${preset.preset_name || "preset"}`}
                          onClick={() => props.onDownloadOne(preset)}
                        >
                          <DownloadIcon />
                        </Button>
                      </TooltipTrigger>
                      <TooltipContent>Download .fxp ({presetFilename(preset)})</TooltipContent>
                    </Tooltip>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      </CardContent>
    </Card>
  )
}

function PresetStatus({ preset }: { preset: Preset }) {
  const issues = [...preset.errors, ...preset.warnings]
  if (issues.length === 0) {
    return <Badge className="bg-emerald-600 text-white">Valid</Badge>
  }
  if (preset.errors.length > 0) {
    return (
      <Tooltip>
        <TooltipTrigger asChild>
          <Badge variant="destructive">Invalid</Badge>
        </TooltipTrigger>
        <TooltipContent>
          <ul className="list-disc pl-4">
            {issues.map((issue, i) => (
              <li key={i}>{issue}</li>
            ))}
          </ul>
        </TooltipContent>
      </Tooltip>
    )
  }
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Badge variant="secondary">
          <TriangleAlertIcon /> Warnings
        </Badge>
      </TooltipTrigger>
      <TooltipContent>
        <ul className="list-disc pl-4">
          {issues.map((issue, i) => (
            <li key={i}>{issue}</li>
          ))}
        </ul>
      </TooltipContent>
    </Tooltip>
  )
}
