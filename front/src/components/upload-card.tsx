import { FileAudioIcon, UploadIcon } from "lucide-react"

import { Button } from "@/components/ui/button"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Progress } from "@/components/ui/progress"

export function UploadCard(props: {
  loading: boolean
  dragging: boolean
  onDragging: (dragging: boolean) => void
  onPick: () => void
  onDrop: (event: React.DragEvent<HTMLDivElement>) => void
}) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>Load project files</CardTitle>
        <CardDescription>
          Drop .flp files or zipped loop packages (.zip) below, or pick them from disk. Each project
          gets its own preset list; the scan looks for Serum plugin instances and extracts their
          preset state.
        </CardDescription>
      </CardHeader>
      <CardContent>
        <div
          role="button"
          tabIndex={0}
          aria-label="Upload .flp or .zip files"
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
                <p className="text-sm font-medium">
                  Drag &amp; drop .flp files or a .zip archive here
                </p>
                <p className="text-muted-foreground text-xs">
                  or click to browse ? files are processed locally
                </p>
              </div>
              <Button
                type="button"
                onClick={(event) => {
                  event.stopPropagation()
                  props.onPick()
                }}
              >
                <UploadIcon /> Select files
              </Button>
            </>
          )}
        </div>
      </CardContent>
    </Card>
  )
}
