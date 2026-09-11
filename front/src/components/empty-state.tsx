import { PackageOpenIcon } from "lucide-react"

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"

export function EmptyState() {
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
