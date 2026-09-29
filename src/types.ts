export interface CacheEntry {
  name: string;
  path: string;
  size: number;
  kind: string;
  extension: string;
  exportable: boolean;
  likelyWallpaper: boolean;
  note: string;
}

export interface ScanResult {
  directory: string;
  total: number;
  exportable: number;
  wallpapers: number;
  entries: CacheEntry[];
}

export interface ExportResult {
  exported: number;
  skipped: number;
  destination: string;
  files: string[];
}

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(2)} MB`;
}
