import { useEffect, useState } from "react";
import {
  AlertTriangle,
  History,
  Loader2,
  RefreshCw,
  ShieldCheck,
} from "lucide-react";
import { getAuditHistory, type AuditEntry } from "../lib/api";
import { formatBytes } from "../lib/format";

const RESULT_LABEL: Record<string, string> = {
  success: "成功",
  partial: "部分成功",
  failed: "失败",
};

const RESULT_STYLE: Record<string, string> = {
  success:
    "bg-emerald-100 text-emerald-800 dark:bg-emerald-500/15 dark:text-emerald-200",
  partial:
    "bg-amber-100 text-amber-900 dark:bg-amber-500/15 dark:text-amber-200",
  failed: "bg-rose-100 text-rose-800 dark:bg-rose-500/15 dark:text-rose-200",
};

export default function HistoryPage() {
  const [entries, setEntries] = useState<AuditEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function load() {
    setLoading(true);
    setError(null);
    try {
      const result = await getAuditHistory(200);
      setEntries(result);
    } catch (loadError) {
      setError(String(loadError));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    load();
  }, []);

  const totalFreed = entries.reduce(
    (total, entry) => total + (entry.bytes || 0),
    0,
  );

  return (
    <div className="space-y-5">
      <section className="flex flex-wrap items-center justify-between gap-4 rounded-2xl border border-emerald-900/10 bg-white p-5 dark:border-emerald-100/10 dark:bg-emerald-950/40">
        <div>
          <div className="flex items-center gap-2">
            <History className="h-5 w-5 text-emerald-700 dark:text-emerald-300" />
            <h2 className="text-lg font-semibold">本地清理历史</h2>
          </div>
          <p className="mt-1 text-sm text-emerald-900/55 dark:text-emerald-100/55">
            共 {entries.length} 条审计记录 · 删除文件合计{" "}
            {formatBytes(totalFreed)} · 仅保存在本机
          </p>
        </div>
        <button
          type="button"
          onClick={load}
          disabled={loading}
          className="inline-flex items-center gap-2 rounded-xl border border-emerald-700/30 px-3 py-2 text-sm font-medium text-emerald-800 hover:bg-emerald-50 disabled:opacity-50 dark:border-emerald-300/30 dark:text-emerald-200 dark:hover:bg-emerald-500/10"
        >
          {loading ? (
            <Loader2 className="h-4 w-4 animate-spin" />
          ) : (
            <RefreshCw className="h-4 w-4" />
          )}
          刷新
        </button>
      </section>

      {error ? (
        <div className="flex items-start gap-2 rounded-xl bg-rose-50 p-3 text-sm text-rose-800 dark:bg-rose-500/10 dark:text-rose-200">
          <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
          {error}
        </div>
      ) : null}

      <section className="overflow-hidden rounded-2xl border border-emerald-900/10 bg-white dark:border-emerald-100/10 dark:bg-emerald-950/40">
        {entries.length === 0 && !loading ? (
          <div className="flex min-h-[280px] flex-col items-center justify-center p-10 text-center">
            <ShieldCheck className="h-10 w-10 text-emerald-600/50 dark:text-emerald-400/50" />
            <h3 className="mt-4 font-semibold">还没有清理记录</h3>
            <p className="mt-2 max-w-md text-sm text-emerald-900/55 dark:text-emerald-100/55">
              每次执行清理都会写入本地审计日志；这里不会上传任何数据。
            </p>
          </div>
        ) : (
          <div className="divide-y divide-emerald-900/10 dark:divide-emerald-100/10">
            {entries.map((entry) => (
              <div key={entry.id} className="px-5 py-3.5">
                <div className="flex flex-wrap items-center gap-3">
                  <span
                    className={`rounded-full px-2 py-0.5 text-xs font-medium ${
                      RESULT_STYLE[entry.result] ?? RESULT_STYLE.failed
                    }`}
                  >
                    {RESULT_LABEL[entry.result] ?? entry.result}
                  </span>
                  <span className="font-mono text-xs text-emerald-900/60 dark:text-emerald-100/60">
                    {entry.rule_id}
                  </span>
                  <span className="text-xs text-emerald-900/45 dark:text-emerald-100/45">
                    {entry.action}
                  </span>
                  <span className="ml-auto text-xs text-emerald-900/45 dark:text-emerald-100/45">
                    {new Date(entry.timestamp * 1000).toLocaleString("zh-CN")}
                  </span>
                </div>
                <div className="mt-1.5 flex flex-wrap items-center gap-3 text-sm">
                  <span className="font-medium">{formatBytes(entry.bytes)}</span>
                  <span className="text-emerald-900/55 dark:text-emerald-100/55">
                    {entry.paths.length.toLocaleString()} 个文件
                  </span>
                  {entry.elevation ? (
                    <span className="text-xs text-amber-700 dark:text-amber-300">
                      管理员权限
                    </span>
                  ) : null}
                </div>
                {entry.paths.length > 0 ? (
                  <details className="mt-2 text-xs text-emerald-900/45 dark:text-emerald-100/45">
                    <summary className="cursor-pointer">查看路径</summary>
                    <ul className="mt-1 max-h-40 space-y-0.5 overflow-y-auto">
                      {entry.paths.slice(0, 200).map((path) => (
                        <li key={path} className="break-all font-mono">
                          {path}
                        </li>
                      ))}
                    </ul>
                    {entry.paths.length > 200 ? (
                      <p className="mt-1">
                        仅显示前 200 条，共 {entry.paths.length} 条。
                      </p>
                    ) : null}
                  </details>
                ) : null}
              </div>
            ))}
          </div>
        )}
      </section>
    </div>
  );
}
