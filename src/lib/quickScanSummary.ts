import type { Finding } from "./api";

/** A small, path-free summary kept for the dashboard. */
export interface QuickScanSummary {
  version: 1;
  safe_bytes: number;
  safe_count: number;
  caution_bytes: number;
  caution_count: number;
  total_count: number;
  scanned_at: number;
}

const STORAGE_KEY = "cc-disksage.quickScanSummary";

export function saveQuickScanSummary(findings: Finding[]): void {
  const summary: QuickScanSummary = {
    version: 1,
    safe_bytes: 0,
    safe_count: 0,
    caution_bytes: 0,
    caution_count: 0,
    total_count: findings.length,
    scanned_at: Math.floor(Date.now() / 1000),
  };
  for (const finding of findings) {
    if (finding.risk === "safe") {
      summary.safe_bytes += finding.allocated_bytes;
      summary.safe_count += 1;
    } else if (finding.risk === "caution") {
      summary.caution_bytes += finding.allocated_bytes;
      summary.caution_count += 1;
    }
  }
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(summary));
  } catch {
    // Storage can be unavailable in private mode; the dashboard simply
    // falls back to the "no summary" state.
  }
}

export function loadQuickScanSummary(): QuickScanSummary | null {
  try {
    const text = localStorage.getItem(STORAGE_KEY);
    if (!text) return null;
    const value = JSON.parse(text) as Partial<QuickScanSummary>;
    if (
      value.version !== 1 ||
      typeof value.safe_bytes !== "number" ||
      typeof value.safe_count !== "number" ||
      typeof value.caution_bytes !== "number" ||
      typeof value.caution_count !== "number" ||
      typeof value.total_count !== "number" ||
      typeof value.scanned_at !== "number"
    ) {
      return null;
    }
    return value as QuickScanSummary;
  } catch {
    return null;
  }
}

export function clearQuickScanSummary(): void {
  try {
    localStorage.removeItem(STORAGE_KEY);
  } catch {
    // Ignore unavailable storage.
  }
}
