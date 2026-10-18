import { useState } from "react";
import {
  AlertTriangle,
  Bot,
  CheckCircle2,
  FolderOpen,
  Loader2,
  ShieldCheck,
  Trash2,
  X,
} from "lucide-react";
import {
  deleteLargeFile,
  llmAssessLargeFile,
  llmPreviewLargeFile,
  openLargeFile,
  type LargeFile,
  type LlmAssessment,
  type ScanSnapshot,
} from "../lib/api";
import { formatBytes } from "../lib/format";

const VERDICT_LABEL: Record<LlmAssessment["verdict"], string> = {
  cleanable: "AI 认为可清理",
  review: "需要人工确认",
  do_not_clean: "AI 不建议清理",
};

const VERDICT_STYLE: Record<LlmAssessment["verdict"], string> = {
  cleanable:
    "bg-emerald-100 text-emerald-900 dark:bg-emerald-500/15 dark:text-emerald-100",
  review:
    "bg-amber-100 text-amber-900 dark:bg-amber-500/15 dark:text-amber-100",
  do_not_clean:
    "bg-rose-100 text-rose-900 dark:bg-rose-500/15 dark:text-rose-100",
};

const CONFIDENCE_LABEL: Record<LlmAssessment["confidence"], string> = {
  low: "低",
  medium: "中",
  high: "高",
};

type Target = { index: number; file: LargeFile };

export default function LargeFilesPanel({
  snapshot,
  onSnapshotChange,
}: {
  snapshot: ScanSnapshot;
  onSnapshotChange: (snapshot: ScanSnapshot) => void;
}) {
  const [showAll, setShowAll] = useState(false);
  const files = snapshot.large_files.slice(
    0,
    showAll ? snapshot.large_files.length : 10,
  );
  const [notice, setNotice] = useState<string | null>(null);

  const [aiTarget, setAiTarget] = useState<Target | null>(null);
  const [aiPayload, setAiPayload] = useState<unknown>(null);
  const [aiAssessment, setAiAssessment] = useState<LlmAssessment | null>(null);
  const [aiConsent, setAiConsent] = useState(false);
  const [aiLoading, setAiLoading] = useState(false);
  const [aiError, setAiError] = useState<string | null>(null);

  const [deleteTarget, setDeleteTarget] = useState<Target | null>(null);
  const [deleteConfirmed, setDeleteConfirmed] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);

  async function handleOpen(index: number) {
    setNotice(null);
    try {
      await openLargeFile(snapshot.id, index);
    } catch (error) {
      setNotice(String(error));
    }
  }

  async function handlePreviewAi(target: Target) {
    setAiTarget(target);
    setAiPayload(null);
    setAiAssessment(null);
    setAiConsent(false);
    setAiError(null);
    setAiLoading(true);
    try {
      setAiPayload(await llmPreviewLargeFile(snapshot.id, target.index));
    } catch (error) {
      setAiError(String(error));
    } finally {
      setAiLoading(false);
    }
  }

  async function handleAssess() {
    if (!aiTarget || !aiConsent) return;
    setAiLoading(true);
    setAiError(null);
    try {
      setAiAssessment(await llmAssessLargeFile(snapshot.id, aiTarget.index));
    } catch (error) {
      setAiError(String(error));
    } finally {
      setAiLoading(false);
    }
  }

  async function handleDelete() {
    if (!deleteTarget || !deleteConfirmed) return;
    setDeleting(true);
    setDeleteError(null);
    try {
      const response = await deleteLargeFile(
        snapshot.id,
        deleteTarget.index,
        true,
      );
      onSnapshotChange(response.snapshot);
      setNotice(
        `已移到回收站：${deleteTarget.file.path} · ${formatBytes(
          deleteTarget.file.allocated_bytes,
        )}。重新扫描可更新目录统计。`,
      );
      setDeleteTarget(null);
      setDeleteConfirmed(false);
      setAiTarget(null);
    } catch (error) {
      setDeleteError(String(error));
    } finally {
      setDeleting(false);
    }
  }

  return (
    <section className="rounded-2xl border border-emerald-900/10 bg-white p-5 dark:border-emerald-100/10 dark:bg-emerald-950/40">
      <div className="flex items-center justify-between gap-3">
        <div>
          <h3 className="font-semibold">大文件</h3>
          <p className="text-xs text-emerald-900/50 dark:text-emerald-100/50">
            超过 500MB 的文件，按占用排序；可先让 AI 看元数据，再由你确认是否移到回收站
          </p>
        </div>
        <ShieldCheck className="h-5 w-5 shrink-0 text-emerald-700/60 dark:text-emerald-300/60" />
      </div>

      {notice ? (
        <div className="mt-3 flex items-start gap-2 rounded-xl bg-emerald-50 p-3 text-xs text-emerald-900 dark:bg-emerald-500/10 dark:text-emerald-100">
          <CheckCircle2 className="mt-0.5 h-4 w-4 shrink-0" />
          <span className="break-all">{notice}</span>
        </div>
      ) : null}

      <div className="mt-4 divide-y divide-emerald-900/10 text-sm dark:divide-emerald-100/10">
        {files.length === 0 ? (
          <p className="py-3 text-emerald-900/55 dark:text-emerald-100/55">
            未发现超过阈值的大文件。
          </p>
        ) : (
          files.map((file, index) => (
            <div
              key={`${file.path}-${index}`}
              className="flex flex-wrap items-center justify-between gap-3 py-2.5"
            >
              <div className="min-w-0 flex-1">
                <div className="truncate font-medium" title={file.path}>
                  {file.path}
                </div>
                <div className="text-xs text-emerald-900/45 dark:text-emerald-100/45">
                  {file.extension ? `.${file.extension}` : "无扩展名"}
                  {file.modified_at
                    ? ` · ${new Date(
                        file.modified_at * 1000,
                      ).toLocaleDateString("zh-CN")}`
                    : ""}
                </div>
              </div>
              <span className="shrink-0 text-emerald-900/60 dark:text-emerald-100/60">
                {formatBytes(file.allocated_bytes)}
              </span>
              <div className="flex w-full shrink-0 flex-wrap justify-end gap-1 sm:w-auto">
                <button
                  type="button"
                  onClick={() => handlePreviewAi({ index, file })}
                  title="AI 只分析脱敏元数据，不读取文件内容，也不能删除文件"
                  className="inline-flex items-center gap-1 rounded-lg border border-emerald-900/15 px-2 py-1 text-xs font-medium text-emerald-800 hover:bg-emerald-50 dark:border-emerald-100/15 dark:text-emerald-200 dark:hover:bg-emerald-500/10"
                >
                  <Bot className="h-3.5 w-3.5" />
                  AI 分析
                </button>
                <button
                  type="button"
                  onClick={() => handleOpen(index)}
                  className="inline-flex items-center gap-1 rounded-lg border border-emerald-900/15 px-2 py-1 text-xs font-medium text-emerald-800 hover:bg-emerald-50 dark:border-emerald-100/15 dark:text-emerald-200 dark:hover:bg-emerald-500/10"
                >
                  <FolderOpen className="h-3.5 w-3.5" />
                  打开目录
                </button>
                <button
                  type="button"
                  onClick={() => {
                    setDeleteTarget({ index, file });
                    setDeleteConfirmed(false);
                    setDeleteError(null);
                  }}
                  className="inline-flex items-center gap-1 rounded-lg border border-rose-300/60 px-2 py-1 text-xs font-medium text-rose-700 hover:bg-rose-50 dark:border-rose-400/30 dark:text-rose-300 dark:hover:bg-rose-500/10"
                >
                  <Trash2 className="h-3.5 w-3.5" />
                  移到回收站
                </button>
              </div>
            </div>
          ))
        )}
      </div>

      {snapshot.large_files.length > 10 ? (
        <button
          type="button"
          onClick={() => setShowAll((current) => !current)}
          className="mt-3 rounded-lg border border-emerald-900/15 px-3 py-1.5 text-xs font-medium text-emerald-800 hover:bg-emerald-50 dark:border-emerald-100/15 dark:text-emerald-200 dark:hover:bg-emerald-500/10"
        >
          {showAll
            ? "收起列表"
            : `显示更多（还有 ${snapshot.large_files.length - 10} 个）`}
        </button>
      ) : null}

      {aiTarget ? (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-emerald-950/30 p-4 backdrop-blur-sm">
          <div className="flex max-h-[90vh] w-full max-w-2xl flex-col overflow-hidden rounded-2xl border border-emerald-900/15 bg-[#f7faf8] shadow-2xl dark:border-emerald-100/15 dark:bg-[#121b16]">
            <header className="flex items-start justify-between gap-4 border-b border-emerald-900/10 p-5 dark:border-emerald-100/10">
              <div className="min-w-0">
                <h2 className="text-lg font-semibold">AI 分析大文件</h2>
                <p className="mt-1 truncate text-sm text-emerald-900/55 dark:text-emerald-100/55">
                  {aiTarget.file.path} ·{" "}
                  {formatBytes(aiTarget.file.allocated_bytes)}
                </p>
              </div>
              <button
                type="button"
                onClick={() => setAiTarget(null)}
                disabled={aiLoading}
                className="rounded-lg p-1.5 text-emerald-900/50 hover:bg-emerald-900/5 disabled:opacity-40 dark:text-emerald-100/50 dark:hover:bg-emerald-100/10"
                aria-label="关闭 AI 分析"
              >
                <X className="h-5 w-5" />
              </button>
            </header>

            <div className="space-y-4 overflow-y-auto p-5">
              <div className="rounded-xl bg-emerald-100/60 p-3 text-xs leading-5 text-emerald-900/75 dark:bg-emerald-500/10 dark:text-emerald-100/75">
                AI 只根据下方脱敏元数据给出建议，不读取文件内容，也不能删除文件。
                最终是否删除必须由你确认。
              </div>

              {aiError ? (
                <div className="flex items-start gap-2 rounded-xl bg-rose-50 p-3 text-xs text-rose-800 dark:bg-rose-500/10 dark:text-rose-200">
                  <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
                  <span className="break-all">{aiError}</span>
                </div>
              ) : null}

              {aiAssessment ? (
                <div className="space-y-3">
                  <div
                    className={`rounded-xl p-3 ${VERDICT_STYLE[aiAssessment.verdict]}`}
                  >
                    <div className="font-semibold">
                      {VERDICT_LABEL[aiAssessment.verdict]} · 置信度{" "}
                      {CONFIDENCE_LABEL[aiAssessment.confidence]}
                    </div>
                    <p className="mt-1 text-sm">{aiAssessment.summary}</p>
                  </div>
                  {aiAssessment.reasoning.length > 0 ? (
                    <div>
                      <h3 className="text-sm font-semibold">推理</h3>
                      <ul className="mt-1 space-y-1 text-sm text-emerald-900/70 dark:text-emerald-100/70">
                        {aiAssessment.reasoning.map((step, index) => (
                          <li key={`${step.text}-${index}`}>
                            • {step.text}{" "}
                            {step.evidence_ids.map((id) => (
                              <span
                                key={id}
                                className="ml-1 rounded bg-emerald-900/10 px-1.5 py-0.5 text-[10px] dark:bg-emerald-100/10"
                              >
                                {id}
                              </span>
                            ))}
                          </li>
                        ))}
                      </ul>
                    </div>
                  ) : null}
                  {aiAssessment.consequence ? (
                    <p className="text-sm text-emerald-900/70 dark:text-emerald-100/70">
                      <strong>删除影响：</strong>
                      {aiAssessment.consequence}
                    </p>
                  ) : null}
                  {aiAssessment.caveats.length > 0 ? (
                    <div>
                      <h3 className="text-sm font-semibold">注意事项</h3>
                      <ul className="mt-1 space-y-1 text-xs text-amber-700 dark:text-amber-300">
                        {aiAssessment.caveats.map((caveat, index) => (
                          <li key={`${caveat}-${index}`}>• {caveat}</li>
                        ))}
                      </ul>
                    </div>
                  ) : null}
                  <p className="text-[11px] text-emerald-900/45 dark:text-emerald-100/45">
                    AI 生成，仅供参考；删除前请确认文件用途。
                  </p>
                </div>
              ) : (
                <>
                  <div>
                    <div className="flex items-center justify-between gap-2">
                      <h3 className="text-sm font-semibold">将发送的 payload</h3>
                      <span className="text-[11px] text-emerald-900/45 dark:text-emerald-100/45">
                        不含文件内容
                      </span>
                    </div>
                    <pre className="mt-2 max-h-64 overflow-auto rounded-xl bg-emerald-950 p-3 text-[11px] leading-5 text-emerald-50">
                      {aiPayload
                        ? JSON.stringify(aiPayload, null, 2)
                        : aiLoading
                          ? "正在准备 payload…"
                          : "—"}
                    </pre>
                  </div>
                  <label className="flex items-start gap-2 text-sm">
                    <input
                      type="checkbox"
                      checked={aiConsent}
                      onChange={(event) => setAiConsent(event.target.checked)}
                      disabled={!aiPayload || aiLoading}
                      className="mt-0.5 h-4 w-4 accent-emerald-700"
                    />
                    我同意将以上脱敏元数据发送给已配置的 LLM
                  </label>
                  <p className="text-[11px] text-emerald-900/45 dark:text-emerald-100/45">
                    如需让 AI 看到文件名，请在设置中开启「发送样本文件名」。
                  </p>
                </>
              )}
            </div>

            <footer className="flex justify-end gap-2 border-t border-emerald-900/10 p-5 dark:border-emerald-100/10">
              <button
                type="button"
                onClick={() => setAiTarget(null)}
                disabled={aiLoading}
                className="rounded-xl border border-emerald-900/15 px-4 py-2 text-sm font-medium text-emerald-800 hover:bg-emerald-50 disabled:opacity-40 dark:border-emerald-100/15 dark:text-emerald-200 dark:hover:bg-emerald-500/10"
              >
                关闭
              </button>
              {!aiAssessment ? (
                <button
                  type="button"
                  onClick={handleAssess}
                  disabled={!aiPayload || !aiConsent || aiLoading}
                  className="inline-flex items-center gap-2 rounded-xl bg-emerald-700 px-4 py-2 text-sm font-medium text-white hover:bg-emerald-800 disabled:cursor-not-allowed disabled:opacity-50 dark:bg-emerald-500 dark:text-emerald-950 dark:hover:bg-emerald-400"
                >
                  {aiLoading ? (
                    <Loader2 className="h-4 w-4 animate-spin" />
                  ) : (
                    <Bot className="h-4 w-4" />
                  )}
                  {aiLoading ? "分析中…" : "发送并分析"}
                </button>
              ) : null}
            </footer>
          </div>
        </div>
      ) : null}

      {deleteTarget ? (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-emerald-950/30 p-4 backdrop-blur-sm">
          <div className="flex max-h-[90vh] w-full max-w-lg flex-col overflow-hidden rounded-2xl border border-emerald-900/15 bg-[#f7faf8] shadow-2xl dark:border-emerald-100/15 dark:bg-[#121b16]">
            <header className="flex items-start justify-between gap-4 border-b border-emerald-900/10 p-5 dark:border-emerald-100/10">
              <div>
                <h2 className="text-lg font-semibold">移到回收站</h2>
                <p className="mt-1 text-sm text-emerald-900/55 dark:text-emerald-100/55">
                  {formatBytes(deleteTarget.file.allocated_bytes)}
                </p>
              </div>
              <button
                type="button"
                onClick={() => setDeleteTarget(null)}
                disabled={deleting}
                className="rounded-lg p-1.5 text-emerald-900/50 hover:bg-emerald-900/5 disabled:opacity-40 dark:text-emerald-100/50 dark:hover:bg-emerald-100/10"
                aria-label="取消删除"
              >
                <X className="h-5 w-5" />
              </button>
            </header>

            <div className="space-y-4 p-5">
              <p className="break-all rounded-xl bg-emerald-900/5 p-3 font-mono text-xs dark:bg-emerald-100/5">
                {deleteTarget.file.path}
              </p>
              <div className="flex items-start gap-2 rounded-xl bg-amber-50 p-3 text-xs leading-5 text-amber-900 dark:bg-amber-500/10 dark:text-amber-200">
                <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
                该文件不来自规则缓存，可能被程序占用或仍有用途。删除后会进入回收站；
                请确认它不是正在使用的虚拟机磁盘、备份或应用数据。Windows/Program Files/
                ProgramData 中的文件会被拒绝。
              </div>
              {deleteError ? (
                <div className="break-all rounded-xl bg-rose-50 p-3 text-xs text-rose-800 dark:bg-rose-500/10 dark:text-rose-200">
                  {deleteError}
                </div>
              ) : null}
              <label className="flex items-start gap-2 text-sm">
                <input
                  type="checkbox"
                  checked={deleteConfirmed}
                  onChange={(event) =>
                    setDeleteConfirmed(event.target.checked)
                  }
                  disabled={deleting}
                  className="mt-0.5 h-4 w-4 accent-rose-600"
                />
                我确认该文件可以删除，并了解可以在回收站恢复
              </label>
            </div>

            <footer className="flex justify-end gap-2 border-t border-emerald-900/10 p-5 dark:border-emerald-100/10">
              <button
                type="button"
                onClick={() => setDeleteTarget(null)}
                disabled={deleting}
                className="rounded-xl border border-emerald-900/15 px-4 py-2 text-sm font-medium text-emerald-800 hover:bg-emerald-50 disabled:opacity-40 dark:border-emerald-100/15 dark:text-emerald-200 dark:hover:bg-emerald-500/10"
              >
                取消
              </button>
              <button
                type="button"
                onClick={handleDelete}
                disabled={!deleteConfirmed || deleting}
                className="inline-flex items-center gap-2 rounded-xl bg-rose-700 px-4 py-2 text-sm font-medium text-white hover:bg-rose-800 disabled:cursor-not-allowed disabled:opacity-50 dark:bg-rose-500 dark:text-rose-950 dark:hover:bg-rose-400"
              >
                {deleting ? (
                  <Loader2 className="h-4 w-4 animate-spin" />
                ) : (
                  <Trash2 className="h-4 w-4" />
                )}
                {deleting ? "正在移动…" : "移到回收站"}
              </button>
            </footer>
          </div>
        </div>
      ) : null}
    </section>
  );
}
