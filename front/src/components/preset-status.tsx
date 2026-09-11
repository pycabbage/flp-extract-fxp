import { TriangleAlertIcon } from "lucide-react"

import { Badge } from "@/components/ui/badge"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import type { Preset } from "@/lib/wasm"

export function PresetStatus({ preset }: { preset: Preset }) {
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
