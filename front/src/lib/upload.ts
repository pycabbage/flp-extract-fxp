import { unzipSync } from "fflate"

export type FlpSource = {
  readonly name: string
  readonly data: Uint8Array
}

export type CollectedFiles = {
  readonly sources: FlpSource[]
  readonly ignoredEntries: number
  readonly failures: string[]
}

type FileOutcome = {
  readonly sources: FlpSource[]
  readonly ignoredEntries: number
  readonly failure: string | null
}

const FLP_SUFFIX = ".flp"
const ZIP_SUFFIX = ".zip"

function hasSuffix(name: string, suffix: string): boolean {
  return name.toLowerCase().endsWith(suffix)
}

async function collectFromFile(file: File): Promise<FileOutcome> {
  if (hasSuffix(file.name, FLP_SUFFIX)) {
    return {
      sources: [{ name: file.name, data: new Uint8Array(await file.arrayBuffer()) }],
      ignoredEntries: 0,
      failure: null,
    }
  }
  if (!hasSuffix(file.name, ZIP_SUFFIX)) {
    return { sources: [], ignoredEntries: 1, failure: null }
  }
  try {
    const entries = unzipSync(new Uint8Array(await file.arrayBuffer()))
    const fileEntries = Object.entries(entries).filter(([name]) => !name.endsWith("/"))
    const flpEntries = fileEntries.filter(([name]) => hasSuffix(name, FLP_SUFFIX))
    return {
      sources: flpEntries.map(([name, data]) => ({ name, data })),
      ignoredEntries: fileEntries.length - flpEntries.length,
      failure: flpEntries.length === 0 ? `${file.name}: no .flp entries inside` : null,
    }
  } catch {
    return {
      sources: [],
      ignoredEntries: 0,
      failure: `${file.name}: could not read ZIP archive`,
    }
  }
}

export async function collectFlpSources(files: readonly File[]): Promise<CollectedFiles> {
  const outcomes: FileOutcome[] = []
  for (const file of files) {
    outcomes.push(await collectFromFile(file))
  }
  return {
    sources: outcomes.flatMap((outcome) => outcome.sources),
    ignoredEntries: outcomes.reduce((sum, outcome) => sum + outcome.ignoredEntries, 0),
    failures: outcomes.flatMap((outcome) => (outcome.failure === null ? [] : [outcome.failure])),
  }
}
