import { useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  BarChart3,
  ChevronRight,
  Copy,
  Gauge,
  HardDrive,
  History,
  Loader2,
  ScanLine,
  Settings,
  ShieldCheck,
  Sparkles,
  Zap,
} from "lucide-react";
import {
  cancelScan,
  getLatestScan,
  getVolumes,
  scanVolume,
  scanVolumeMft,
  type ScanProgress,
  type ScanSnapshot,
  type VolumeInfo,
} from "./lib/api";
import LargeFilesPanel from "./components/LargeFilesPanel";
import { formatBytes, formatPercent } from "./lib/format";
import { loadQuickScanSummary } from "./lib/quickScanSummary";
import CleanupPage from "./pages/CleanupPage";
import DuplicatesPage from "./pages/DuplicatesPage";
import ExplorePage from "./pages/ExplorePage";
import HistoryPage from "./pages/HistoryPage";
import SettingsPage from "./pages/SettingsPage";

type PageId =
  | "dashboard"
  | "explore"
  | "cleanup"
  | "duplicates"
  | "history"
  | "settings";

const NAV: Array<{
  id: PageId;
  label: string;
  icon: typeof Gauge;
}> = [
  { id: "dashboard", label: "仪表盘", icon: Gauge },
  { id: "explore", label: "空间分析", icon: BarChart3 },
  { id: "cleanup", label: "清理建议", icon: Sparkles },
  { id: "duplicates", label: "重复文件", icon: Copy },
  { id: "history", label: "历史记录", icon: History },
  { id: "settings", label: "设置", icon: Settings },
];

const SAMPLE_VOLUME: VolumeInfo = {
  letter: "C",
  label: "Windows-SSD",
  filesystem: "NTFS",
  total_bytes: 200 * 1024 ** 3,
  free_bytes: 71.6 * 1024 ** 3,
};

function DriveRing({ volume }: { volume: VolumeInfo }) {
  const used = Math.max(volume.total_bytes - volume.free_bytes, 0);
  const ratio = volume.total_bytes > 0 ? used / volume.total_bytes : 0;
  const radius = 54;
  const circumference = 2 * Math.PI * radius;
  const dash = circumference * ratio;

  return (
    <div className="relative h-40 w-40 shrink-0">
      <svg viewBox="0 0 140 140" className="h-full w-full -rotate-90">
        <circle
          cx="70"
          cy="70"
          r={radius}
          fill="none"
          stroke="currentColor"
          strokeWidth="14"
          className="text-emerald-950/10 dark:text-emerald-100/10"
        />
        <circle
          cx="70"
          cy="70"
          r={radius}
          fill="none"
          stroke="currentColor"
          strokeWidth="14"
          strokeLinecap="round"
          strokeDasharray={`${dash} ${circumference - dash}`}
          className="text-emerald-600 dark:text-emerald-400"
        />
      </svg>
      <div className="absolute inset-0 flex flex-col items-center justify-center">
        <span className="text-2xl font-semibold">
          {formatPercent(ratio * 100)}
        </span>
        <span className="text-xs text-emerald-900/60 dark:text-emerald-100/60">
          已使用
        </span>
      </div>
    </div>
  );
}

function TopDirs({ snapshot }: { snapshot: ScanSnapshot }) {
  const dirs = snapshot.top_dirs.slice(0, 8);
  const max = Math.max(...dirs.map((dir) => dir.allocated_bytes), 1);

  return (
    <section className="rounded-2xl border border-emerald-900/10 bg-white p-5 dark:border-emerald-100/10 dark:bg-emerald-950/40">
      <div className="flex items-center justify-between">
        <div>
          <h3 className="font-semibold">空间占用 Top 目录</h3>
          <p className="text-xs text-emerald-900/50 dark:text-emerald-100/50">
            来自本次扫描（目录遍历或 MFT）；可在「空间分析」中查看 treemap
          </p>
        </div>
        <BarChart3 className="h-5 w-5 text-emerald-700/60 dark:text-emerald-300/60" />
      </div>
      <div className="mt-4 space-y-3">
        {dirs.length === 0 ? (
          <p className="text-sm text-emerald-900/55 dark:text-emerald-100/55">
            没有可展示的目录数据（可能权限不足）。
          </p>
        ) : (
          dirs.map((dir) => (
            <div key={dir.path}>
              <div className="mb-1 flex items-center justify-between gap-3 text-sm">
                <span className="truncate font-medium">{dir.name || dir.path}</span>
                <span className="shrink-0 text-emerald-900/60 dark:text-emerald-100/60">
                  {formatBytes(dir.allocated_bytes)}
                </span>
              </div>
              <div className="h-1.5 overflow-hidden rounded-full bg-emerald-950/10 dark:bg-emerald-100/10">
                <div
                  className="h-full rounded-full bg-emerald-600 dark:bg-emerald-400"
                  style={{ width: `${Math.max((dir.allocated_bytes / max) * 100, 1)}%` }}
                />
              </div>
            </div>
          ))
        )}
      </div>
    </section>
  );
}

function Dashboard({
  volume,
  snapshot,
  scanning,
  mftScanning,
  progress,
  error,
  onScan,
  onMftScan,
  onCancel,
  onOpenCleanup,
  onSnapshotChange,
}: {
  volume: VolumeInfo;
  snapshot: ScanSnapshot | null;
  scanning: boolean;
  mftScanning: boolean;
  progress: ScanProgress | null;
  error: string | null;
  onScan: () => void;
  onMftScan: () => void;
  onCancel: () => void;
  onOpenCleanup: () => void;
  onSnapshotChange: (snapshot: ScanSnapshot) => void;
}) {
  const displayVolume = snapshot
    ? { ...volume, free_bytes: snapshot.free_bytes }
    : volume;
  const quickSummary = useMemo(() => loadQuickScanSummary(), []);

  return (
    <div className="space-y-5">
      <section className="rounded-2xl border border-emerald-900/10 bg-white p-6 shadow-sm dark:border-emerald-100/10 dark:bg-emerald-950/40">
        <div className="flex flex-wrap items-center justify-between gap-6">
          <div className="flex items-center gap-6">
            <DriveRing volume={displayVolume} />
            <div>
              <div className="flex items-center gap-2">
                <HardDrive className="h-5 w-5 text-emerald-700 dark:text-emerald-300" />
                <h2 className="text-lg font-semibold">
                  {volume.letter}: {volume.label || "本地磁盘"}
                </h2>
                <span className="rounded-full bg-emerald-100 px-2 py-0.5 text-xs text-emerald-800 dark:bg-emerald-500/15 dark:text-emerald-200">
                  {volume.filesystem || "NTFS"}
                </span>
              </div>
              <p className="mt-1 text-sm text-emerald-900/60 dark:text-emerald-100/60">
                总容量 {formatBytes(volume.total_bytes)} · 可用{" "}
                {formatBytes(displayVolume.free_bytes)}
              </p>
              <p className="mt-3 max-w-md text-sm text-emerald-900/70 dark:text-emerald-100/70">
                {snapshot
                  ? `扫描完成（${snapshot.engine === "mft" ? "MFT 秒扫" : "目录遍历"}）：${snapshot.file_count.toLocaleString()} 个文件，${snapshot.dir_count.toLocaleString()} 个目录，用时 ${(snapshot.duration_ms / 1000).toFixed(1)} 秒。`
                  : "先做一次只读扫描，查看目录占用与大文件，再决定清理哪些项目。"}
              </p>
            </div>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <button
              type="button"
              onClick={onScan}
              disabled={scanning || mftScanning}
              className="inline-flex items-center gap-2 rounded-xl bg-emerald-700 px-4 py-2.5 text-sm font-medium text-white transition hover:bg-emerald-800 disabled:cursor-not-allowed disabled:opacity-60 dark:bg-emerald-500 dark:text-emerald-950 dark:hover:bg-emerald-400"
            >
              {scanning ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <ScanLine className="h-4 w-4" />
              )}
              {scanning ? "扫描中…" : `扫描 ${volume.letter} 盘`}
            </button>
            <button
              type="button"
              onClick={onMftScan}
              disabled={scanning || mftScanning}
              title="管理员全盘扫描：需要 UAC，通过 helper 读取 NTFS MFT（只读），通常数秒完成。"
              className="inline-flex items-center gap-2 rounded-xl border border-emerald-700/30 px-4 py-2.5 text-sm font-medium text-emerald-800 transition hover:bg-emerald-50 disabled:cursor-not-allowed disabled:opacity-60 dark:border-emerald-300/30 dark:text-emerald-200 dark:hover:bg-emerald-500/10"
            >
              {mftScanning ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <Zap className="h-4 w-4" />
              )}
              {mftScanning ? "管理员扫描中…" : "管理员全盘扫描"}
            </button>
          </div>
        </div>

        {mftScanning ? (
          <div className="mt-4 flex items-start gap-2 rounded-xl border border-amber-300/50 bg-amber-50 p-3 text-xs text-amber-900 dark:border-amber-400/30 dark:bg-amber-500/10 dark:text-amber-200">
            <Loader2 className="mt-0.5 h-4 w-4 shrink-0 animate-spin" />
            正在请求管理员权限并通过 disksage-helper 读取 NTFS MFT（只读）。
            如果取消 UAC，本次扫描会终止；普通扫描仍可继续使用。
          </div>
        ) : null}

        {scanning ? (
          <div className="mt-4 rounded-xl border border-emerald-700/20 bg-emerald-50/70 p-3 dark:border-emerald-300/20 dark:bg-emerald-500/10">
            <div className="flex flex-wrap items-center justify-between gap-2 text-xs">
              <span>
                已扫描 {(progress?.files ?? 0).toLocaleString()} 个文件 ·{" "}
                {(progress?.dirs ?? 0).toLocaleString()} 个目录
              </span>
              <span className="text-emerald-900/55 dark:text-emerald-100/55">
                {formatBytes(progress?.bytes ?? 0)} ·{" "}
                {((progress?.elapsed_ms ?? 0) / 1000).toFixed(1)}s
              </span>
            </div>
            <div className="mt-1 truncate font-mono text-[11px] text-emerald-900/50 dark:text-emerald-100/50">
              {progress?.current ?? "正在启动扫描…"}
            </div>
            <div className="mt-2 flex items-center justify-between gap-3">
              <span className="text-[11px] text-emerald-900/45 dark:text-emerald-100/45">
                {progress && progress.errors > 0
                  ? `${progress.errors} 个权限错误（可改用「管理员全盘扫描」）`
                  : "只读扫描，不会修改任何文件"}
              </span>
              <button
                type="button"
                onClick={onCancel}
                className="rounded-lg border border-rose-300/60 px-2.5 py-1 text-xs font-medium text-rose-700 hover:bg-rose-50 dark:border-rose-400/30 dark:text-rose-300 dark:hover:bg-rose-500/10"
              >
                取消扫描
              </button>
            </div>
          </div>
        ) : null}

        {snapshot?.node_limit_hit ? (
          <div className="mt-4 flex items-start gap-2 rounded-xl bg-amber-50 p-3 text-xs text-amber-900 dark:bg-amber-500/10 dark:text-amber-200">
            <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
            目录节点超过上限，超小文件未全部计入；结果用于空间分析足够，回收量以规则扫描为准。
          </div>
        ) : null}

        {snapshot && snapshot.error_count > 0 ? (
          <p className="mt-3 text-xs text-amber-700 dark:text-amber-300">
            有 {snapshot.error_count} 个目录/文件因权限或占用无法读取；可使用「管理员全盘扫描」
            查看系统目录占用。
          </p>
        ) : null}

        {error ? (
          <div className="mt-4 flex items-start gap-2 rounded-xl bg-rose-50 p-3 text-sm text-rose-800 dark:bg-rose-500/10 dark:text-rose-200">
            <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
            {error}
          </div>
        ) : null}
      </section>

      <section className="rounded-2xl border border-emerald-900/10 bg-white p-5 shadow-sm dark:border-emerald-100/10 dark:bg-emerald-950/40">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div className="min-w-0">
            <div className="flex items-center gap-2">
              <Sparkles className="h-5 w-5 text-emerald-700 dark:text-emerald-300" />
              <h3 className="font-semibold">可清理项</h3>
            </div>
            <p className="mt-1 max-w-2xl text-sm leading-6 text-emerald-900/60 dark:text-emerald-100/60">
              全盘扫描只做空间分析，不会直接删除文件。清理前请在「清理建议」执行规则扫描；
              每一项都有规则来源、红线校验和证据链。
            </p>
            {quickSummary ? (
              <p className="mt-2 text-sm text-emerald-900/70 dark:text-emerald-100/70">
                上次快速扫描：安全项{" "}
                <strong>{formatBytes(quickSummary.safe_bytes)}</strong>（
                {quickSummary.safe_count} 项）· 谨慎项{" "}
                {formatBytes(quickSummary.caution_bytes)}（
                {quickSummary.caution_count} 项）·{" "}
                {new Date(quickSummary.scanned_at * 1000).toLocaleString(
                  "zh-CN",
                )}
              </p>
            ) : (
              <p className="mt-2 text-sm text-emerald-900/55 dark:text-emerald-100/55">
                还没有快速扫描记录。
              </p>
            )}
          </div>
          <button
            type="button"
            onClick={onOpenCleanup}
            className="inline-flex shrink-0 items-center gap-1.5 rounded-xl border border-emerald-700/30 px-4 py-2 text-sm font-medium text-emerald-800 transition hover:bg-emerald-50 dark:border-emerald-300/30 dark:text-emerald-200 dark:hover:bg-emerald-500/10"
          >
            去清理建议
            <ChevronRight className="h-4 w-4" />
          </button>
        </div>
      </section>

      {snapshot ? (
        <div className="grid gap-4 lg:grid-cols-2">
          <TopDirs snapshot={snapshot} />
          <LargeFilesPanel
            snapshot={snapshot}
            onSnapshotChange={onSnapshotChange}
          />
        </div>
      ) : (
        <div className="grid gap-4 md:grid-cols-3">
          <ActionCard
            icon={ScanLine}
            title="快速扫描"
            description="只扫描规则目标目录，几秒内给出可清理项。"
            badge="M3"
          />
          <ActionCard
            icon={BarChart3}
            title="空间分析"
            description="Treemap 与文件夹树，快速定位 C 盘空间大户。"
            badge="M1"
          />
          <ActionCard
            icon={Sparkles}
            title="智能清理"
            description="规则证据链 + 可选 LLM 分析，由你决定是否清理。"
            badge="M2–M6"
          />
        </div>
      )}

      <section className="rounded-2xl border border-emerald-900/10 bg-white p-5 dark:border-emerald-100/10 dark:bg-emerald-950/40">
        <div className="flex items-start gap-3">
          <ShieldCheck className="mt-0.5 h-5 w-5 shrink-0 text-emerald-600 dark:text-emerald-400" />
          <div>
            <h3 className="font-medium">安全第一</h3>
            <p className="mt-1 text-sm text-emerald-900/65 dark:text-emerald-100/65">
              默认删除到回收站；不碰 WinSxS、Windows Installer、浏览器密码与聊天记录；
              管理员动作通过独立提权 helper 执行。没有预览就不会删除任何文件。
            </p>
          </div>
        </div>
      </section>
    </div>
  );
}

function ActionCard({
  icon: Icon,
  title,
  description,
  badge,
}: {
  icon: typeof Gauge;
  title: string;
  description: string;
  badge: string;
}) {
  return (
    <section className="rounded-2xl border border-emerald-900/10 bg-white p-5 shadow-sm transition hover:border-emerald-700/30 dark:border-emerald-100/10 dark:bg-emerald-950/40">
      <div className="flex items-center justify-between">
        <span className="inline-flex h-10 w-10 items-center justify-center rounded-xl bg-emerald-100 text-emerald-700 dark:bg-emerald-500/15 dark:text-emerald-300">
          <Icon className="h-5 w-5" />
        </span>
        <span className="rounded-full bg-emerald-100 px-2.5 py-1 text-xs font-medium text-emerald-800 dark:bg-emerald-500/15 dark:text-emerald-200">
          {badge}
        </span>
      </div>
      <h3 className="mt-4 font-semibold">{title}</h3>
      <p className="mt-1 text-sm leading-6 text-emerald-900/60 dark:text-emerald-100/60">
        {description}
      </p>
      <div className="mt-4 inline-flex items-center gap-1 text-sm font-medium text-emerald-700 dark:text-emerald-300">
        使用左侧导航 <ChevronRight className="h-4 w-4" />
      </div>
    </section>
  );
}

export default function App() {
  const [active, setActive] = useState<PageId>("dashboard");
  const [volumes, setVolumes] = useState<VolumeInfo[]>([]);
  const [selected, setSelected] = useState("C");
  const [snapshot, setSnapshot] = useState<ScanSnapshot | null>(null);
  const [scanning, setScanning] = useState(false);
  const [mftScanning, setMftScanning] = useState(false);
  const [scanProgress, setScanProgress] = useState<ScanProgress | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    getVolumes()
      .then((list) => {
        if (cancelled) return;
        if (list.length > 0) {
          setVolumes(list);
          setSelected((current) =>
            list.some((item) => item.letter === current)
              ? current
              : list[0].letter,
          );
        }
      })
      .catch(() => {
        if (!cancelled) {
          setError(
            "无法读取卷信息。若在浏览器中预览，请改用 npm run tauri dev 启动桌面应用。",
          );
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    getLatestScan()
      .then((latest) => {
        if (cancelled || !latest) return;
        setSnapshot(latest);
        setSelected(latest.volume);
      })
      .catch(() => {
        // A missing/corrupt local database should not block the app.
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const volume =
    volumes.find((item) => item.letter === selected) ??
    volumes[0] ??
    SAMPLE_VOLUME;

  async function handleScan() {
    setScanning(true);
    setError(null);
    setScanProgress(null);
    try {
      const result = await scanVolume(selectionLetter(), setScanProgress);
      setSnapshot(result);
    } catch (scanError) {
      const message = String(scanError);
      setError(
        message.toLowerCase().includes("cancel")
          ? "扫描已取消。"
          : message,
      );
    } finally {
      setScanning(false);
      setScanProgress(null);
    }
  }

  async function handleMftScan() {
    setMftScanning(true);
    setError(null);
    try {
      const result = await scanVolumeMft(selectionLetter());
      setSnapshot(result);
    } catch (scanError) {
      const message = String(scanError);
      setError(
        message.toLowerCase().includes("cancel") || message.includes("取消")
          ? "MFT 扫描已取消或未获得管理员授权。"
          : message,
      );
    } finally {
      setMftScanning(false);
    }
  }

  async function handleCancelScan() {
    try {
      await cancelScan();
    } catch (cancelError) {
      setError(String(cancelError));
    }
  }

  function selectionLetter(): string {
    return volume.letter;
  }

  function handleVolumeChange(letter: string) {
    setSelected(letter);
    setSnapshot(null);
    setError(null);
  }

  return (
    <div className="flex h-full min-h-screen">
      <aside className="hidden w-60 shrink-0 flex-col border-r border-emerald-900/10 bg-white/70 p-4 backdrop-blur md:flex dark:border-emerald-100/10 dark:bg-emerald-950/50">
        <div className="flex items-center gap-3 px-2 py-3">
          <div className="flex h-10 w-10 items-center justify-center rounded-xl bg-emerald-700 text-sm font-bold text-white dark:bg-emerald-500 dark:text-emerald-950">
            CC
          </div>
          <div>
            <div className="font-semibold leading-tight">CC DiskSage</div>
            <div className="text-xs text-emerald-900/50 dark:text-emerald-100/50">
              磁盘空间管家
            </div>
          </div>
        </div>

        <nav className="mt-4 space-y-1">
          {NAV.map(({ id, label, icon: Icon }) => {
            const selected = active === id;
            return (
              <button
                key={id}
                type="button"
                onClick={() => setActive(id)}
                className={`flex w-full items-center gap-3 rounded-xl px-3 py-2.5 text-sm transition ${
                  selected
                    ? "bg-emerald-700 text-white shadow-sm dark:bg-emerald-500 dark:text-emerald-950"
                    : "text-emerald-900/70 hover:bg-emerald-100/70 dark:text-emerald-100/70 dark:hover:bg-emerald-500/10"
                }`}
              >
                <Icon className="h-4 w-4" />
                {label}
              </button>
            );
          })}
        </nav>

        <div className="mt-auto rounded-xl bg-emerald-100/70 p-3 text-xs leading-5 text-emerald-900/70 dark:bg-emerald-500/10 dark:text-emerald-100/70">
          <ShieldCheck className="mb-1 h-4 w-4" />
          离线可用 · LLM 可选 · 无遥测
        </div>
      </aside>

      <main className="min-w-0 flex-1 overflow-y-auto">
        <header className="sticky top-0 z-10 flex items-center justify-between gap-4 border-b border-emerald-900/10 bg-[#f4f7f5]/85 px-6 py-4 backdrop-blur dark:border-emerald-100/10 dark:bg-[#101613]/85">
          <div>
            <h1 className="text-xl font-semibold">
              {NAV.find((item) => item.id === active)?.label}
            </h1>
            <p className="text-xs text-emerald-900/50 dark:text-emerald-100/50">
              Windows · 离线优先 · 全盘只读扫描
            </p>
          </div>
          <div className="flex items-center gap-2">
            {volumes.length > 1 ? (
              <select
                value={selected}
                onChange={(event) => handleVolumeChange(event.target.value)}
                className="rounded-lg border border-emerald-900/15 bg-white px-2.5 py-1.5 text-sm dark:border-emerald-100/15 dark:bg-emerald-950"
              >
                {volumes.map((item) => (
                  <option key={item.letter} value={item.letter}>
                    {item.letter}: {item.label || "本地磁盘"}
                  </option>
                ))}
              </select>
            ) : null}
            <span className="rounded-full border border-emerald-700/20 bg-emerald-100 px-3 py-1 text-xs font-medium text-emerald-800 dark:border-emerald-300/20 dark:bg-emerald-500/15 dark:text-emerald-200">
              本地模式
            </span>
          </div>
        </header>

        <div className="mx-auto max-w-6xl p-6">
          {active === "dashboard" ? (
            <Dashboard
              volume={volume}
              snapshot={snapshot}
              scanning={scanning}
              mftScanning={mftScanning}
              progress={scanProgress}
              error={error}
              onScan={handleScan}
              onMftScan={handleMftScan}
              onCancel={handleCancelScan}
              onOpenCleanup={() => setActive("cleanup")}
              onSnapshotChange={(next) => setSnapshot(next)}
            />
          ) : active === "cleanup" ? (
            <CleanupPage />
          ) : active === "duplicates" ? (
            <DuplicatesPage />
          ) : active === "explore" ? (
            <ExplorePage
              snapshot={snapshot}
              scanning={scanning}
              onScan={handleScan}
            />
          ) : active === "history" ? (
            <HistoryPage />
          ) : (
            <SettingsPage />
          )}
        </div>
      </main>
    </div>
  );
}
