import { useEffect, useState } from "react";
import {
  AlertTriangle,
  CheckCircle2,
  Copy,
  FolderOpen,
  Loader2,
  Search,
  ShieldCheck,
  Trash2,
} from "lucide-react";
import {
  cancelDuplicateScan,
  deleteDuplicateFiles,
  findDuplicateFiles,
  openDuplicateFile,
  type DuplicateGroup,
  type DuplicateScanProgress,
  type DuplicateScanResult,
} from "../lib/api";
import { formatBytes } from "../lib/format";

const SCOPES = [
  { id: "user", label: "文档/下载/图片/视频/桌面（推荐）" },
  { id: "downloads", label: "下载" },
  { id: "documents", label: "文档" },
  { id: "pictures", label: "图片" },
  { id: "videos", label: "视频" },
  { id: "desktop", label: "桌面" },
];

const inputClass =
  "w-full rounded-xl border border-emerald-900/15 bg-white px-3 py-2 text-sm outline-none transition focus:border-emerald-600 focus:ring-2 focus:ring-emerald-600/20 dark:border-emerald-100/15 dark:bg-emerald-950/50";

function phaseLabel(phase: string): string {
  switch (phase) {
    case "collecting":
      return "收集文件";
    case "partial_hash":
      return "快速哈希";
    case "full_hash":
      return "完整哈希";
    default:
      return phase;
  }
}

function DuplicateGroupCard({
  group,
  selected,
  confirming,
  onToggle,
  onSelectDuplicates,
  onClear,
  onConfirmStart,
  onConfirmCancel,
  onDelete,
  onOpen,
  deleting,
}: {
  group: DuplicateGroup;
  selected: string[];
  confirming: boolean;
  onToggle: (fileId: string) => void;
  onSelectDuplicates: () => void;
  onClear: () => void;
  onConfirmStart: () => void;
  onConfirmCancel: () => void;
  onDelete: () => void;
  onOpen: (fileId: string) => void;
  deleting: boolean;
}) {
  const [ack, setAck] = useState(false);
  useEffect(() => {
    if (!confirming) setAck(false);
  }, [confirming]);
  const allSelected = selected.length >= group.files.length;
  const keepCount = group.files.length - selected.length;
  const canDelete = selected.length > 0 && keepCount >= 1 && !deleting;

  return (
    <section className="rounded-2xl border border-emerald-900/10 bg-white p-5 shadow-sm dark:border-emerald-100/10 dark:bg-emerald-950/40">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <div className="flex items-center gap-2">
            <Copy className="h-4 w-4 text-emerald-700 dark:text-emerald-300" />
            <h3 className="font-semibold">
              {group.files.length} 个相同文件 · 每个{" "}
              {formatBytes(group.size)}
            </h3>
          </div>
          <p className="mt-1 text-xs text-emerald-900/50 dark:text-emerald-100/50">
            可释放 {formatBytes(group.wasted_bytes)} · 哈希{" "}
            <span className="font-mono">{group.hash.slice(0, 16)}…</span>
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          <button
            type="button"
            onClick={onSelectDuplicates}
            className="rounded-lg border border-emerald-900/15 px-2.5 py-1.5 text-xs font-medium text-emerald-800 hover:bg-emerald-50 dark:border-emerald-100/15 dark:text-emerald-200 dark:hover:bg-emerald-500/10"
          >
            选择除第一个外的全部
          </button>
          <button
            type="button"
            onClick={onClear}
            disabled={selected.length === 0}
            className="rounded-lg border border-emerald-900/15 px-2.5 py-1.5 text-xs font-medium text-emerald-900/60 hover:bg-emerald-50 disabled:cursor-not-allowed disabled:opacity-40 dark:border-emerald-100/15 dark:text-emerald-100/60 dark:hover:bg-emerald-500/10"
          >
            清空选择
          </button>
        </div>
      </div>

      <div className="mt-4 divide-y divide-emerald-900/10 text-sm dark:divide-emerald-100/10">
        {group.files.map((file, index) => (
          <div
            key={file.id}
            className="flex items-center gap-3 py-2.5"
          >
            <input
              type="checkbox"
              checked={selected.includes(file.id)}
              onChange={() => onToggle(file.id)}
              className="h-4 w-4 shrink-0 accent-rose-600"
              aria-label={`选择删除 ${file.path}`}
            />
            <div className="min-w-0 flex-1">
              <div className="truncate font-medium" title={file.path}>
                {file.path}
              </div>
              <div className="text-xs text-emerald-900/45 dark:text-emerald-100/45">
                {index === 0 ? "建议保留 · " : ""}
                {file.modified_at
                  ? new Date(file.modified_at * 1000).toLocaleString()
                  : "修改时间未知"}
              </div>
            </div>
            <button
              type="button"
              onClick={() => onOpen(file.id)}
              className="inline-flex shrink-0 items-center gap-1 rounded-lg border border-emerald-900/15 px-2 py-1 text-xs text-emerald-800 hover:bg-emerald-50 dark:border-emerald-100/15 dark:text-emerald-200 dark:hover:bg-emerald-500/10"
            >
              <FolderOpen className="h-3.5 w-3.5" />
              位置
            </button>
          </div>
        ))}
      </div>

      <div className="mt-3 flex flex-wrap items-center justify-between gap-2">
        <p className="text-xs text-emerald-900/50 dark:text-emerald-100/50">
          已选择 {selected.length} 个 · 保留 {keepCount} 个
          {allSelected ? "（必须至少保留一个）" : ""}
        </p>
        <button
          type="button"
          onClick={onConfirmStart}
          disabled={!canDelete || confirming}
          className="inline-flex items-center gap-2 rounded-xl border border-rose-300/60 px-3 py-2 text-sm font-medium text-rose-700 hover:bg-rose-50 disabled:cursor-not-allowed disabled:opacity-50 dark:border-rose-400/30 dark:text-rose-300 dark:hover:bg-rose-500/10"
        >
          <Trash2 className="h-4 w-4" />
          移至回收站
        </button>
      </div>

      {confirming ? (
        <div className="mt-3 rounded-xl border border-rose-300/60 bg-rose-50 p-3 text-sm text-rose-900 dark:border-rose-400/30 dark:bg-rose-500/10 dark:text-rose-100">
          <p>
            将把所选 {selected.length} 个文件移到回收站，保留 {keepCount} 个。
            移动后可从回收站恢复；如果文件位于 OneDrive 等同步目录，删除会同步到云端。
          </p>
          <label className="mt-2 flex items-start gap-2 text-xs">
            <input
              type="checkbox"
              checked={ack}
              onChange={(event) => setAck(event.target.checked)}
              className="mt-0.5 h-4 w-4 accent-rose-600"
            />
            我确认这些文件是重复项，并了解删除会同步到云同步目录（如适用）。
          </label>
          <div className="mt-3 flex justify-end gap-2">
            <button
              type="button"
              onClick={onConfirmCancel}
              disabled={deleting}
              className="rounded-lg border border-rose-300/50 px-3 py-1.5 text-xs font-medium hover:bg-white/50 disabled:opacity-50 dark:border-rose-400/30 dark:hover:bg-rose-500/10"
            >
              取消
            </button>
            <button
              type="button"
              onClick={onDelete}
              disabled={!ack || !canDelete}
              className="inline-flex items-center gap-1.5 rounded-lg bg-rose-700 px-3 py-1.5 text-xs font-medium text-white hover:bg-rose-800 disabled:cursor-not-allowed disabled:opacity-50"
            >
              {deleting ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <Trash2 className="h-3.5 w-3.5" />
              )}
              确认移至回收站
            </button>
          </div>
        </div>
      ) : null}
    </section>
  );
}

export default function DuplicatesPage() {
  const [scope, setScope] = useState("user");
  const [minSizeMb, setMinSizeMb] = useState(1);
  const [scanning, setScanning] = useState(false);
  const [progress, setProgress] = useState<DuplicateScanProgress | null>(null);
  const [result, setResult] = useState<DuplicateScanResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [selected, setSelected] = useState<Record<string, string[]>>({});
  const [confirming, setConfirming] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<string | null>(null);
  const [visibleGroups, setVisibleGroups] = useState(20);

  async function handleScan() {
    setScanning(true);
    setError(null);
    setMessage(null);
    setResult(null);
    setProgress(null);
    setSelected({});
    setConfirming(null);
    setVisibleGroups(20);
    try {
      const scan = await findDuplicateFiles(
        scope,
        Math.round(Math.max(0.004, minSizeMb) * 1024 * 1024),
        setProgress,
      );
      setResult(scan);
    } catch (scanError) {
      const text = String(scanError);
      setError(
        text.toLowerCase().includes("cancel") || text.includes("取消")
          ? "重复文件扫描已取消。"
          : text,
      );
    } finally {
      setScanning(false);
      setProgress(null);
    }
  }

  async function handleCancel() {
    try {
      await cancelDuplicateScan();
    } catch (cancelError) {
      setError(String(cancelError));
    }
  }

  function toggleFile(groupId: string, fileId: string) {
    setSelected((current) => {
      const list = current[groupId] ?? [];
      return {
        ...current,
        [groupId]: list.includes(fileId)
          ? list.filter((id) => id !== fileId)
          : [...list, fileId],
      };
    });
    setConfirming(null);
  }

  function selectDuplicates(group: DuplicateGroup) {
    setSelected((current) => ({
      ...current,
      [group.id]: group.files.slice(1).map((file) => file.id),
    }));
    setConfirming(null);
  }

  function clearGroup(group: DuplicateGroup) {
    setSelected((current) => ({ ...current, [group.id]: [] }));
    setConfirming(null);
  }

  async function handleDelete(group: DuplicateGroup) {
    if (!result) return;
    const deleteIds = selected[group.id] ?? [];
    const keep = group.files.find((file) => !deleteIds.includes(file.id));
    if (!keep || deleteIds.length === 0 || deleteIds.length >= group.files.length) {
      setError("请至少保留一个文件，并选择要删除的重复项。");
      return;
    }
    setDeleting(group.id);
    setError(null);
    setMessage(null);
    try {
      const cleanup = await deleteDuplicateFiles(
        result.scan_id,
        group.id,
        keep.id,
        deleteIds,
      );
      if (cleanup.failed.length > 0) {
        setMessage(
          `已移动 ${cleanup.deleted} 个文件到回收站，${cleanup.failed.length} 个失败。建议重新扫描后再处理失败项。`,
        );
      } else {
        setMessage(`已将 ${cleanup.deleted} 个文件移到回收站。`);
      }
      const remaining = result.groups.filter((item) => item.id !== group.id);
      setResult({
        ...result,
        groups: remaining,
        total_wasted_bytes: remaining.reduce(
          (sum, item) => sum + item.wasted_bytes,
          0,
        ),
      });
      setSelected((current) => {
        const next = { ...current };
        delete next[group.id];
        return next;
      });
      setConfirming(null);
    } catch (deleteError) {
      setError(String(deleteError));
    } finally {
      setDeleting(null);
    }
  }

  async function handleOpen(fileId: string) {
    if (!result) return;
    try {
      await openDuplicateFile(result.scan_id, fileId);
    } catch (openError) {
      setError(String(openError));
    }
  }

  const groups = result?.groups.slice(0, visibleGroups) ?? [];

  return (
    <div className="space-y-5">
      <section className="rounded-2xl border border-emerald-900/10 bg-white p-6 shadow-sm dark:border-emerald-100/10 dark:bg-emerald-950/40">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div>
            <div className="flex items-center gap-2">
              <Copy className="h-5 w-5 text-emerald-700 dark:text-emerald-300" />
              <h2 className="text-lg font-semibold">重复文件查找</h2>
            </div>
            <p className="mt-1 max-w-2xl text-sm leading-6 text-emerald-900/60 dark:text-emerald-100/60">
              只读取用户目录中的文件内容计算 BLAKE3 哈希；先按大小、再按快速哈希、最后按完整哈希分组。
              结果只保存在本机内存，不上传。删除只通过回收站，且必须至少保留一个文件。
            </p>
          </div>
          <span className="rounded-full bg-emerald-100 px-3 py-1 text-xs font-medium text-emerald-800 dark:bg-emerald-500/15 dark:text-emerald-200">
            只读扫描 · 回收站删除
          </span>
        </div>

        {error ? (
          <div className="mt-4 flex items-start gap-2 rounded-xl bg-rose-50 p-3 text-sm text-rose-800 dark:bg-rose-500/10 dark:text-rose-200">
            <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
            {error}
          </div>
        ) : null}
        {message ? (
          <div className="mt-4 flex items-start gap-2 rounded-xl bg-emerald-50 p-3 text-sm text-emerald-800 dark:bg-emerald-500/10 dark:text-emerald-200">
            <CheckCircle2 className="mt-0.5 h-4 w-4 shrink-0" />
            {message}
          </div>
        ) : null}

        <div className="mt-5 grid gap-4 md:grid-cols-3">
          <label className="space-y-1.5">
            <span className="text-sm font-medium">扫描范围</span>
            <select
              value={scope}
              onChange={(event) => setScope(event.target.value)}
              className={inputClass}
              disabled={scanning}
            >
              {SCOPES.map((item) => (
                <option key={item.id} value={item.id}>
                  {item.label}
                </option>
              ))}
            </select>
          </label>
          <label className="space-y-1.5">
            <span className="text-sm font-medium">最小文件大小（MB）</span>
            <input
              type="number"
              min={0.004}
              max={10240}
              step={0.5}
              value={minSizeMb}
              onChange={(event) =>
                setMinSizeMb(Math.max(0, Number(event.target.value) || 0))
              }
              className={inputClass}
              disabled={scanning}
            />
            <span className="block text-xs text-emerald-900/45 dark:text-emerald-100/45">
              小于该大小的文件会被忽略，默认 1MB。
            </span>
          </label>
          <div className="flex items-end gap-2">
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
              {scanning ? "扫描中…" : "开始查找"}
            </button>
            {scanning ? (
              <button
                type="button"
                onClick={handleCancel}
                className="rounded-xl border border-rose-300/60 px-3 py-2.5 text-sm font-medium text-rose-700 hover:bg-rose-50 dark:border-rose-400/30 dark:text-rose-300 dark:hover:bg-rose-500/10"
              >
                取消
              </button>
            ) : null}
          </div>
        </div>

        {scanning ? (
          <div className="mt-4 rounded-xl border border-emerald-700/20 bg-emerald-50/70 p-3 dark:border-emerald-300/20 dark:bg-emerald-500/10">
            <div className="flex flex-wrap items-center justify-between gap-2 text-xs">
              <span>
                {phaseLabel(progress?.phase ?? "collecting")} · 已扫描{" "}
                {(progress?.files_seen ?? 0).toLocaleString()} 个文件 ·
                候选 {(progress?.candidate_files ?? 0).toLocaleString()}
              </span>
              <span className="text-emerald-900/55 dark:text-emerald-100/55">
                哈希 {progress?.hashed_files ?? 0} 次 · 组{" "}
                {progress?.groups_found ?? 0} ·{" "}
                {((progress?.elapsed_ms ?? 0) / 1000).toFixed(1)}s
              </span>
            </div>
            <div className="mt-1 truncate font-mono text-[11px] text-emerald-900/50 dark:text-emerald-100/50">
              {progress?.current || "正在准备扫描…"}
            </div>
          </div>
        ) : null}

        {result ? (
          <div className="mt-4 grid gap-3 text-sm sm:grid-cols-4">
            <div className="rounded-xl bg-emerald-50/70 p-3 dark:bg-emerald-500/10">
              <div className="text-xs text-emerald-900/50 dark:text-emerald-100/50">
                重复组
              </div>
              <div className="mt-1 text-lg font-semibold">
                {result.groups.length.toLocaleString()}
              </div>
            </div>
            <div className="rounded-xl bg-emerald-50/70 p-3 dark:bg-emerald-500/10">
              <div className="text-xs text-emerald-900/50 dark:text-emerald-100/50">
                可释放
              </div>
              <div className="mt-1 text-lg font-semibold">
                {formatBytes(result.total_wasted_bytes)}
              </div>
            </div>
            <div className="rounded-xl bg-emerald-50/70 p-3 dark:bg-emerald-500/10">
              <div className="text-xs text-emerald-900/50 dark:text-emerald-100/50">
                扫描文件
              </div>
              <div className="mt-1 text-lg font-semibold">
                {result.files_scanned.toLocaleString()}
              </div>
            </div>
            <div className="rounded-xl bg-emerald-50/70 p-3 dark:bg-emerald-500/10">
              <div className="text-xs text-emerald-900/50 dark:text-emerald-100/50">
                用时
              </div>
              <div className="mt-1 text-lg font-semibold">
                {(result.duration_ms / 1000).toFixed(1)}s
              </div>
            </div>
          </div>
        ) : null}
        {result?.truncated ? (
          <p className="mt-2 text-xs text-amber-700 dark:text-amber-300">
            结果已截断（文件数/组数/每组文件数达到上限）。可提高最小文件大小后重新扫描。
          </p>
        ) : null}
        {result && result.error_count > 0 ? (
          <p className="mt-2 text-xs text-amber-700 dark:text-amber-300">
            {result.error_count} 个文件因被占用或权限不足未能读取。
          </p>
        ) : null}
      </section>

      {result && result.groups.length === 0 ? (
        <section className="rounded-2xl border border-dashed border-emerald-900/20 bg-white/60 p-10 text-center text-sm text-emerald-900/60 dark:border-emerald-100/15 dark:bg-emerald-950/20 dark:text-emerald-100/60">
          <ShieldCheck className="mx-auto mb-2 h-6 w-6" />
          在所选范围内未发现大于阈值的重复文件。
        </section>
      ) : null}

      {groups.map((group) => (
        <DuplicateGroupCard
          key={group.id}
          group={group}
          selected={selected[group.id] ?? []}
          confirming={confirming === group.id}
          onToggle={(fileId) => toggleFile(group.id, fileId)}
          onSelectDuplicates={() => selectDuplicates(group)}
          onClear={() => clearGroup(group)}
          onConfirmStart={() => {
            setConfirming(group.id);
            setError(null);
          }}
          onConfirmCancel={() => setConfirming(null)}
          onDelete={() => handleDelete(group)}
          onOpen={handleOpen}
          deleting={deleting === group.id}
        />
      ))}

      {result && result.groups.length > visibleGroups ? (
        <div className="flex justify-center">
          <button
            type="button"
            onClick={() => setVisibleGroups((count) => count + 20)}
            className="rounded-xl border border-emerald-900/15 px-4 py-2 text-sm font-medium text-emerald-800 hover:bg-emerald-50 dark:border-emerald-100/15 dark:text-emerald-200 dark:hover:bg-emerald-500/10"
          >
            显示更多（剩余 {result.groups.length - visibleGroups} 组）
          </button>
        </div>
      ) : null}

      <section className="rounded-2xl border border-emerald-900/10 bg-white p-5 text-sm leading-6 text-emerald-900/65 dark:border-emerald-100/10 dark:bg-emerald-950/40 dark:text-emerald-100/65">
        <div className="flex items-start gap-3">
          <ShieldCheck className="mt-0.5 h-5 w-5 shrink-0 text-emerald-600 dark:text-emerald-400" />
          <div>
            <h3 className="font-medium">安全边界</h3>
            <p className="mt-1">
              扫描范围仅限当前用户目录及其常见子目录，不扫描 Windows、Program Files、
              WinSxS 或 System Volume Information。删除前后端会再次校验路径、红线、
              reparse point 与文件是否存在；前端只传文件和组的 ID，不能传任意删除路径。
            </p>
            <p className="mt-2">
              建议先保留每个组中修改时间最早或最常用的文件，确认无误后再删除其余副本；
              首次使用可先只删除一个组验证结果。
            </p>
          </div>
        </div>
      </section>
    </div>
  );
}
