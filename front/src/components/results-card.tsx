import { DownloadIcon, TriangleAlertIcon } from "lucide-react"

import { PresetStatus } from "@/components/preset-status"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Checkbox } from "@/components/ui/checkbox"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { formatBytes, presetFilename } from "@/lib/download"
import type { Preset } from "@/lib/wasm"

export type ScanResult = {
  fileName: string
  fileData: Uint8Array
  presets: Preset[]
  duplicates: number
  serum2Skipped: number
  failed: string[]
}

export function ResultsCard(props: {
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
