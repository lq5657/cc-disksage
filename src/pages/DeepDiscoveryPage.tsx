import { useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  FolderOpen,
  Loader2,
  Search,
  ShieldCheck,
} from "lucide-react";
import {
  cancelDeepInspect,
  deepInspect,
  openDiscoveryItem,
  type DiscoveryItem,
  type DiscoveryProgress,
  type DiscoveryResult,
  type VolumeInfo,
} from "../lib/api";
import { formatBytes } from "../lib/format";

const CATEGORY_LABELS: Record<DiscoveryItem["category"], string> = {
  large_file: "大文件",
  large_log: "大日志",
  dump: "转储",
  old_installer: "旧安装包",
  old_archive: "旧压缩包",
};

const FILTERS: Array<{ id: "all" | DiscoveryItem["category"]; label: string }> = [
  { id: "all", label: "全部" },
  { id: "large_file", label: "大文件" },
  { id: "large_log", label: "大日志" },
  { id: "dump", label: "转储" },
  { id: "old_installer", label: "旧安装包" },
  { id: "old_archive", label: "旧压缩包" },
];

const CATEGORY_STYLES: Record<DiscoveryItem["category"], string> = {
  large_file: "bg-emerald-100 text-emerald-800 dark:bg-emerald-500/15 dark:text-emerald-200",
  large_log: "bg-amber-100 text-amber-800 dark:bg-amber-500/15 dark:text-amber-200",
  dump: "bg-rose-100 text-rose-800 dark:bg-rose-500/15 dark:text-rose-200",
  old_installer: "bg-sky-100 text-sky-800 dark:bg-sky-500/15 dark:text-sky-200",
  old_archive: "bg-violet-100 text-violet-800 dark:bg-violet-500/15 dark:text-violet-200",
};

function formatDate(value: number | null): string {
  if (!value) return "未知";
  return new Date(value * 1000).toLocaleDateString("zh-CN");
}

export default function DeepDiscoveryPage({ volume }: { volume: VolumeInfo }) {
  const [result, setResult] = useState<DiscoveryResult | null>(null);
  const [scanning, setScanning] = useState(false);
  const [progress, setProgress] = useState<DiscoveryProgress | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState<"all" | DiscoveryItem["category"]>("all");

  useEffect(() => {
    setResult(null);
    setProgress(null);
    setError(null);
    setFilter("all");
  }, [volume.letter]);

  const filtered = useMemo(() => {
    if (!result) return [];
    if (filter === "all") return result.items;
    return result.items.filter((item) => item.category === filter);
  }, [result, filter]);

  const totalBytes = useMemo(
    () => (result?.items ?? []).reduce((sum, item) => sum + item.bytes, 0),
    [result],
  );

  async function handleScan() {
    setScanning(true);
    setError(null);
    setProgress(null);
    setResult(null);
    setFilter("all");
    try {
      const next = await deepInspect(volume.letter, setProgress);
      setResult(next);
    } catch (scanError) {
      const message = String(scanError);
      setError(
        message.toLowerCase().includes("cancel") || message.includes("取消")
          ? "深度发现已取消。"
          : message,
      );
    } finally {
      setScanning(false);
      setProgress(null);
    }
  }

  async function handleCancel() {
    try {
      await cancelDeepInspect();
    } catch (cancelError) {
      setError(String(cancelError));
    }
  }

  async function handleOpen(item: DiscoveryItem) {
    try {
      await openDiscoveryItem(item.index);
    } catch (openError) {
      setError(String(openError));
    }
  }

  return (
    <div className="space-y-5">
      <section className="flex flex-wrap items-start justify-between gap-4 rounded-2xl border border-emerald-900/10 bg-white p-5 dark:border-emerald-100/10 dark:bg-emerald-950/40">
        <div className="max-w-2xl">
          <div className="flex items-center gap-2">
            <Search className="h-5 w-5 text-emerald-700 dark:text-emerald-300" />
            <h2 className="text-lg font-semibold">深度发现</h2>
          </div>
          <p className="mt-2 text-sm leading-6 text-emerald-900/60 dark:text-emerald-100/60">
            只读扫描 {volume.letter}: 盘，发现值得人工复核的大文件、大日志/ETL、转储、
            旧安装包与旧压缩包。本页不会删除或修改任何文件。
          </p>
          <p className="mt-1 text-xs text-emerald-900/45 dark:text-emerald-100/45">
            阈值：大文件 ≥100 MB；日志/ETL ≥50 MB；转储 ≥50 MB；安装包/压缩包 ≥100 MB
            且超过 180 天。跳过 Windows/Program Files/WinSxS/Installer/reparse point 等受保护位置。
          </p>
        </div>
        <div className="flex items-center gap-2">
          {scanning ? (
            <button
              type="button"
              onClick={handleCancel}
              className="inline-flex items-center gap-2 rounded-xl border border-rose-300/60 px-3 py-2 text-sm font-medium text-rose-700 hover:bg-rose-50 dark:border-rose-400/30 dark:text-rose-200 dark:hover:bg-rose-500/10"
            >
              取消扫描
            </button>
          ) : null}
          <button
            type="button"
            onClick={handleScan}
            disabled={scanning}
            className="inline-flex items-center gap-2 rounded-xl bg-emerald-700 px-4 py-2.5 text-sm font-medium text-white hover:bg-emerald-800 disabled:cursor-not-allowed disabled:opacity-60 dark:bg-emerald-500 dark:text-emerald-950"
          >
            {scanning ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <Search className="h-4 w-4" />
            )}
            {scanning ? "深度扫描中…" : "开始深度发现"}
          </button>
        </div>
      </section>

      {error ? (
        <div className="flex items-start gap-2 rounded-xl bg-rose-50 p-3 text-sm text-rose-800 dark:bg-rose-500/10 dark:text-rose-200">
          <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
          {error}
        </div>
      ) : null}

      {scanning || progress ? (
        <section className="rounded-2xl border border-emerald-900/10 bg-white p-4 dark:border-emerald-100/10 dark:bg-emerald-950/40">
          <div className="flex items-center gap-3">
            <Loader2 className="h-4 w-4 animate-spin text-emerald-700 dark:text-emerald-300" />
            <div className="text-sm font-medium">
              {progress
                ? `已扫描 ${progress.files.toLocaleString()} 个文件 / ${progress.dirs.toLocaleString()} 个目录 · ${(progress.elapsed_ms / 1000).toFixed(1)}s`
                : "正在准备深度扫描…"}
            </div>
          </div>
          {progress?.current ? (
            <div className="mt-2 truncate text-xs text-emerald-900/45 dark:text-emerald-100/45">
              {progress.current}
            </div>
          ) : null}
        </section>
      ) : null}

      {result ? (
        <>
          <section className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
            <div className="rounded-xl border border-emerald-900/10 bg-white p-4 dark:border-emerald-100/10 dark:bg-emerald-950/40">
              <div className="text-xs text-emerald-900/45 dark:text-emerald-100/45">
                候选总大小
              </div>
              <div className="mt-1 text-2xl font-semibold">{formatBytes(totalBytes)}</div>
            </div>
            <div className="rounded-xl border border-emerald-900/10 bg-white p-4 dark:border-emerald-100/10 dark:bg-emerald-950/40">
              <div className="text-xs text-emerald-900/45 dark:text-emerald-100/45">
                候选数量
              </div>
              <div className="mt-1 text-2xl font-semibold">{result.items.length}</div>
            </div>
            <div className="rounded-xl border border-emerald-900/10 bg-white p-4 dark:border-emerald-100/10 dark:bg-emerald-950/40">
              <div className="text-xs text-emerald-900/45 dark:text-emerald-100/45">
                已扫描
              </div>
              <div className="mt-1 text-lg font-semibold">
                {result.scanned_files.toLocaleString()} 文件 /{" "}
                {result.scanned_dirs.toLocaleString()} 目录
              </div>
            </div>
            <div className="rounded-xl border border-emerald-900/10 bg-white p-4 dark:border-emerald-100/10 dark:bg-emerald-950/40">
              <div className="text-xs text-emerald-900/45 dark:text-emerald-100/45">
                用时
              </div>
              <div className="mt-1 text-lg font-semibold">
                {(result.elapsed_ms / 1000).toFixed(1)} 秒
              </div>
            </div>
          </section>

          <section className="rounded-2xl border border-emerald-900/10 bg-white dark:border-emerald-100/10 dark:bg-emerald-950/40">
            <div className="flex flex-wrap items-center justify-between gap-3 border-b border-emerald-900/10 p-4 dark:border-emerald-100/10">
              <div className="flex flex-wrap gap-2">
                {FILTERS.map((item) => (
                  <button
                    key={item.id}
                    type="button"
                    onClick={() => setFilter(item.id)}
                    className={`rounded-full px-3 py-1.5 text-xs font-medium transition ${
                      filter === item.id
                        ? "bg-emerald-700 text-white dark:bg-emerald-500 dark:text-emerald-950"
                        : "bg-emerald-100/70 text-emerald-900/70 hover:bg-emerald-100 dark:bg-emerald-500/10 dark:text-emerald-100/70"
                    }`}
                  >
                    {item.label}
                  </button>
                ))}
              </div>
              <div className="flex items-center gap-1 text-xs text-emerald-900/45 dark:text-emerald-100/45">
                <ShieldCheck className="h-3.5 w-3.5" />
                只读发现，不会删除文件
              </div>
            </div>

            {filtered.length === 0 ? (
              <div className="p-10 text-center text-sm text-emerald-900/55 dark:text-emerald-100/55">
                {result.items.length === 0
                  ? "没有发现超过阈值的大文件、日志、转储或旧安装包。"
                  : "当前分类下没有条目。"}
              </div>
            ) : (
              <div className="divide-y divide-emerald-900/5 dark:divide-emerald-100/5">
                {filtered.map((item) => (
                  <div
                    key={`${item.index}-${item.path}`}
                    className="flex flex-col gap-3 p-4 sm:flex-row sm:items-center sm:justify-between"
                  >
                    <div className="min-w-0 flex-1">
                      <div className="flex flex-wrap items-center gap-2">
                        <span
                          className={`rounded-full px-2 py-0.5 text-[11px] font-medium ${CATEGORY_STYLES[item.category]}`}
                        >
                          {CATEGORY_LABELS[item.category]}
                        </span>
                        <span className="truncate font-medium">{item.name}</span>
                      </div>
                      <div className="mt-1 truncate text-xs text-emerald-900/45 dark:text-emerald-100/45">
                        {item.path}
                      </div>
                      <div className="mt-1 text-xs text-emerald-900/55 dark:text-emerald-100/55">
                        {item.reason_zh}
                      </div>
                    </div>
                    <div className="flex shrink-0 items-center gap-4 text-sm">
                      <div className="text-right">
                        <div className="font-semibold">{formatBytes(item.bytes)}</div>
                        <div className="text-xs text-emerald-900/45 dark:text-emerald-100/45">
                          {formatDate(item.modified_at)}
                        </div>
                      </div>
                      <button
                        type="button"
                        onClick={() => handleOpen(item)}
                        className="inline-flex items-center gap-1.5 rounded-lg border border-emerald-700/30 px-2.5 py-1.5 text-xs font-medium text-emerald-800 hover:bg-emerald-50 dark:border-emerald-300/30 dark:text-emerald-200 dark:hover:bg-emerald-500/10"
                      >
                        <FolderOpen className="h-3.5 w-3.5" />
                        打开目录
                      </button>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </section>

          {result.truncated ? (
            <p className="text-xs text-amber-700 dark:text-amber-300">
              仅显示按大小排序的前 {result.items.length} 项；请先处理较大的条目或缩小扫描范围。
            </p>
          ) : null}
        </>
      ) : null}
    </div>
  );
}
