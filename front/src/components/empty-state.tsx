import { PackageOpenIcon } from "lucide-react"

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"

export function EmptyState() {
  return (
    <Alert>
      <PackageOpenIcon />
      <AlertTitle>Nothing scanned yet</AlertTitle>
      <AlertDescription>
        Load .flp files or a ZIP archive above to see the Serum presets they contain.
      </AlertDescription>
    </Alert>
  )
}
