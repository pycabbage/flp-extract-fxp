import { TriangleAlertIcon } from "lucide-react"

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { formatBytes } from "@/lib/download"
import type { ConvertDetail, ConvertOutcome, Preset } from "@/lib/wasm"

function stateBytesFor(detail: ConvertDetail, presets: Preset[]): number | null {
  if (detail.channel === "") return null
  const match = presets.find((preset) => preset.channel === detail.channel)
  return match === undefined ? null : match.state_bytes
}

function sizeCell(detail: ConvertDetail, presets: Preset[]): string {
  const cid3 = formatBytes(detail.payloadLen)
  const state = stateBytesFor(detail, presets)
  return state === null ? cid3 : `${formatBytes(state)} → ${cid3}`
}

export function ConvertResultCard(props: {
  outcome: ConvertOutcome
  inputSize: number
  presets: Preset[]
}) {
  const outcome = props.outcome
  const inputSize = props.inputSize
  const outputSize = outcome.flp.length
  const delta = outputSize - inputSize
  const deltaLabel =
    delta === 0 ? "±0 B" : `${delta > 0 ? "+" : "-"}${formatBytes(Math.abs(delta))}`
  return (
    <Card>
      <CardHeader>
        <CardTitle>Conversion report</CardTitle>
        <CardDescription className="tabular-nums">
          {outcome.convertedCount} Serum instance{outcome.convertedCount === 1 ? "" : "s"} converted
          to Serum2 · {formatBytes(inputSize)} → {formatBytes(outputSize)} ({deltaLabel})
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        {outcome.warnings.length > 0 && (
          <Alert>
            <TriangleAlertIcon />
            <AlertTitle>
              {outcome.warnings.length} skipped or warned instance
              {outcome.warnings.length === 1 ? "" : "s"}
            </AlertTitle>
            <AlertDescription>
              <ul className="list-disc pl-4">
                {outcome.warnings.map((message, i) => (
                  <li key={i}>{message}</li>
                ))}
              </ul>
            </AlertDescription>
          </Alert>
        )}
        {outcome.details.length === 0 ? (
          <p className="text-muted-foreground text-sm">No Serum instances were converted.</p>
        ) : (
          <div className="rounded-md border">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Channel</TableHead>
                  <TableHead>Preset</TableHead>
                  <TableHead className="text-right">Size (state → cid3)</TableHead>
                  <TableHead>Notes</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {outcome.details.map((detail, i) => (
                  <TableRow key={i}>
                    <TableCell>{detail.channelName || detail.channel || "—"}</TableCell>
                    <TableCell className="font-medium">
                      {detail.presetName || "(unnamed)"}
                    </TableCell>
                    <TableCell className="text-right tabular-nums">
                      {sizeCell(detail, props.presets)}
                    </TableCell>
                    <TableCell className="text-muted-foreground">
                      {detail.notes.length === 0 ? "—" : detail.notes.join("; ")}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>
        )}
      </CardContent>
    </Card>
  )
}
