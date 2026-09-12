import { AlertCircleIcon, ExternalLinkIcon } from "lucide-react"
import { useRef, useState } from "react"
import { toast } from "sonner"

import { EmptyState } from "@/components/empty-state"
import { ProjectSection, type ProjectEntry } from "@/components/project-section"
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
import { collectFlpSources } from "@/lib/upload"
import { initWasm, scanDoc, type FlpDocHandle } from "@/lib/wasm"

import { ThemeProvider } from "./components/theme-provider"

export default function App() {
  const [projects, setProjects] = useState<readonly ProjectEntry[]>([])
  const projectsRef = useRef<readonly ProjectEntry[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [fatalError, setFatalError] = useState<string | null>(null)
  const inputRef = useRef<HTMLInputElement>(null)
  const nextIdRef = useRef(0)
  const [dragging, setDragging] = useState(false)

  const commitProjects = (next: readonly ProjectEntry[]) => {
    projectsRef.current = next
    setProjects(next)
  }

  const nextProjectId = () => {
    nextIdRef.current += 1
    return `project-${nextIdRef.current}`
  }

  const toMessage = (err: unknown, fallback: string) => {
    if (err instanceof Error) return err.message
    const text = String(err)
    return text.length > 0 ? text : fallback
  }

  const handleFiles = async (files: readonly File[]) => {
    if (files.length === 0) return
    setLoading(true)
    setError(null)
    try {
      await initWasm()
      const collected = await collectFlpSources(files)
      if (collected.sources.length === 0) {
        const message =
          collected.failures.length > 0
            ? collected.failures.join(" ")
            : collected.ignoredEntries > 0
              ? `No .flp files found — ${collected.ignoredEntries} unsupported file${
                  collected.ignoredEntries === 1 ? "" : "s"
                } ignored.`
              : "No .flp files found."
        setError(message)
        toast.error(message)
        return
      }
      const next = [...projectsRef.current]
      const replacedDocs: FlpDocHandle[] = []
      const scanFailures: string[] = []
      const loaded: ProjectEntry[] = []
      for (const source of collected.sources) {
        try {
          const scanned = scanDoc(source.data)
          const unique = scanned.presets.filter((p) => !p.duplicate)
          const duplicates = scanned.presets.length - unique.length
          const entry: ProjectEntry = {
            id: nextProjectId(),
            result: {
              fileName: source.name,
              fileData: source.data,
              presets: unique,
              duplicates,
              serum2Skipped: scanned.serum2Skipped,
              failed: scanned.failedJson,
            },
            doc: scanned,
          }
          const existingIndex = next.findIndex((p) => p.result.fileName === source.name)
          if (existingIndex >= 0) {
            replacedDocs.push(next[existingIndex].doc)
            next[existingIndex] = entry
          } else {
            next.push(entry)
          }
          loaded.push(entry)
          if (unique.length === 0 && scanned.failedJson.length > 0) {
            toast.error(`No presets extracted from ${source.name}`)
          }
          for (const message of scanned.failedJson) {
            toast.warning(`${source.name}: ${message}`)
          }
        } catch (err) {
          scanFailures.push(`${source.name}: ${toMessage(err, "Failed to scan the file.")}`)
        }
      }
      commitProjects(next)
      for (const doc of replacedDocs) {
        doc.free()
      }
      if (loaded.length > 0) {
        const presetTotal = loaded.reduce((sum, p) => sum + p.result.presets.length, 0)
        const ignoredNote =
          collected.ignoredEntries > 0
            ? ` — ${collected.ignoredEntries} unsupported file${
                collected.ignoredEntries === 1 ? "" : "s"
              } ignored`
            : ""
        toast.success(
          `Loaded ${loaded.length} project${loaded.length === 1 ? "" : "s"} with ${presetTotal} unique preset${
            presetTotal === 1 ? "" : "s"
          }${ignoredNote}`
        )
      }
      for (const failure of collected.failures) {
        toast.error(failure)
      }
      if (loaded.length === 0) {
        const message =
          scanFailures.length > 0 ? scanFailures.join(" ") : "No .flp files could be scanned."
        setError(message)
      }
    } catch (err) {
      const message = toMessage(err, "Failed to scan the files.")
      setError(message)
      toast.error(message)
    } finally {
      setLoading(false)
    }
  }

  const onInputChange = (event: React.ChangeEvent<HTMLInputElement>) => {
    const files = Array.from(event.target.files ?? [])
    if (files.length > 0) void handleFiles(files)
    event.target.value = ""
  }

  const onDrop = (event: React.DragEvent<HTMLDivElement>) => {
    event.preventDefault()
    setDragging(false)
    const files = Array.from(event.dataTransfer.files)
    if (files.length > 0) void handleFiles(files)
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
            accept=".flp,.zip"
            multiple
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

          {projects.length === 0 && !loading && !error && <EmptyState />}

          {projects.map((project) => (
            <ProjectSection key={project.id} project={project} />
          ))}

          <footer className="text-muted-foreground mt-auto pt-4 text-center text-xs">
            Runs fully client-side — your .flp files never leave the browser.
          </footer>
        </div>
        <Toaster richColors position="bottom-right" />
      </ThemeProvider>
    </TooltipProvider>
  )
}
