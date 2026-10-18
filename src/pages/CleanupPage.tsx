import { useMemo, useState } from "react";
import {
  AlertTriangle,
  Bot,
  CheckCircle2,
  ChevronRight,
  FileSearch,
  Info,
  Loader2,
  Send,
  ShieldCheck,
  Sparkles,
  Trash2,
  X,
} from "lucide-react";
import {
  createPlan,
  executePlan,
  llmAskFollowup,
  llmAssessFinding,
  llmPreviewPayload,
  quickScan,
  type CleanupResult,
  type Evidence,
  type Finding,
  type LlmAssessment,
  type PlanPreview,
  type Recoverability,
  type RiskLevel,
} from "../lib/api";
import { formatBytes } from "../lib/format";

const RISK_LABEL: Record<RiskLevel, string> = {
  safe: "安全",
  caution: "谨慎",
  danger: "危险",
};

const RISK_STYLE: Record<RiskLevel, string> = {
  safe: "bg-emerald-100 text-emerald-800 dark:bg-emerald-500/15 dark:text-emerald-200",
  caution: "bg-amber-100 text-amber-900 dark:bg-amber-500/15 dark:text-amber-200",
  danger: "bg-rose-100 text-rose-800 dark:bg-rose-500/15 dark:text-rose-200",
};

const SOURCE_LABEL: Record<Evidence["source"], string> = {
  rule: "规则",
  scanner: "扫描器",
  app_detection: "应用检测",
  llm: "AI",
};

const RECOVER_LABEL: Record<Recoverability, string> = {
  regenerable: "自动重建 / 重新下载",
  recycle_bin: "回收站可恢复",
  quarantine: "隔离区可恢复",
  irreversible: "不可恢复",
  user_data: "用户数据（不应清理）",
};

const VERDICT_LABEL: Record<LlmAssessment["verdict"], string> = {
  cleanable: "可清理",
  review: "需人工确认",
  do_not_clean: "不建议清理",
};

const VERDICT_STYLE: Record<LlmAssessment["verdict"], string> = {
  cleanable:
    "bg-emerald-100 text-emerald-800 dark:bg-emerald-500/15 dark:text-emerald-200",
  review:
    "bg-amber-100 text-amber-900 dark:bg-amber-500/15 dark:text-amber-200",
  do_not_clean:
    "bg-rose-100 text-rose-800 dark:bg-rose-500/15 dark:text-rose-200",
};

const CONFIDENCE_LABEL: Record<LlmAssessment["confidence"], string> = {
  low: "低",
  medium: "中",
  high: "高",
};

function formatTime(timestamp: number | null): string {
  if (!timestamp) return "未知";
  return new Date(timestamp * 1000).toLocaleString("zh-CN");
}

function EvidenceRow({ evidence }: { evidence: Evidence }) {
  return (
    <div className="rounded-xl border border-emerald-900/10 bg-white/70 p-3 dark:border-emerald-100/10 dark:bg-emerald-950/30">
      <div className="flex flex-wrap items-center gap-2">
        <span className="rounded-md bg-emerald-700/10 px-1.5 py-0.5 font-mono text-xs font-semibold text-emerald-800 dark:bg-emerald-400/10 dark:text-emerald-200">
          {evidence.id}
        </span>
        <span className="rounded-md bg-emerald-950/5 px-1.5 py-0.5 text-xs text-emerald-900/60 dark:bg-emerald-100/10 dark:text-emerald-100/60">
          {SOURCE_LABEL[evidence.source]}
        </span>
        <span className="text-xs text-emerald-900/45 dark:text-emerald-100/45">
          {evidence.kind}
        </span>
      </div>
      <p className="mt-2 text-sm leading-6">{evidence.claim}</p>
      {evidence.bytes != null ? (
        <p className="mt-1 text-xs text-emerald-900/50 dark:text-emerald-100/50">
          实测大小：{formatBytes(evidence.bytes)}
        </p>
      ) : null}
      {evidence.timestamp ? (
        <p className="mt-1 text-xs text-emerald-900/50 dark:text-emerald-100/50">
          时间：{formatTime(evidence.timestamp)}
        </p>
      ) : null}
      {evidence.path ? (
        <p className="mt-1 break-all font-mono text-xs text-emerald-900/45 dark:text-emerald-100/45">
          {evidence.path}
        </p>
      ) : null}
    </div>
  );
}

function EvidenceDrawer({
  finding,
  onClose,
}: {
  finding: Finding;
  onClose: () => void;
}) {
  const [assessment, setAssessment] = useState<LlmAssessment | null>(null);
  const [payloadPreview, setPayloadPreview] = useState<unknown | null>(null);
  const [consent, setConsent] = useState(false);
  const [aiBusy, setAiBusy] = useState(false);
  const [aiError, setAiError] = useState<string | null>(null);
  const [followup, setFollowup] = useState("");
  const [followupBusy, setFollowupBusy] = useState(false);
  const [followupAnswer, setFollowupAnswer] = useState<string | null>(null);

  async function handlePreview() {
    setAiBusy(true);
    setAiError(null);
    try {
      const payload = await llmPreviewPayload(finding.id);
      setPayloadPreview(payload);
      setConsent(false);
    } catch (previewError) {
      setAiError(String(previewError));
    } finally {
      setAiBusy(false);
    }
  }

  async function handleAnalyze() {
    if (!consent) return;
    setAiBusy(true);
    setAiError(null);
    try {
      const result = await llmAssessFinding(finding.id);
      setAssessment(result);
      setPayloadPreview(null);
      setConsent(false);
      setFollowupAnswer(null);
    } catch (analyzeError) {
      setAiError(String(analyzeError));
    } finally {
      setAiBusy(false);
    }
  }

  async function handleFollowup() {
    const question = followup.trim();
    if (!question) return;
    setFollowupBusy(true);
    setAiError(null);
    try {
      const answer = await llmAskFollowup(finding.id, question);
      setFollowupAnswer(answer);
      setFollowup("");
    } catch (followupError) {
      setAiError(String(followupError));
    } finally {
      setFollowupBusy(false);
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-emerald-950/30 p-4 backdrop-blur-sm">
      <div className="flex max-h-[90vh] w-full max-w-3xl flex-col overflow-hidden rounded-2xl border border-emerald-900/15 bg-[#f7faf8] shadow-2xl dark:border-emerald-100/15 dark:bg-[#121b16]">
        <header className="flex items-start justify-between gap-4 border-b border-emerald-900/10 p-5 dark:border-emerald-100/10">
          <div>
            <div className="flex flex-wrap items-center gap-2">
              <h2 className="text-lg font-semibold">{finding.title_zh}</h2>
              <span
                className={`rounded-full px-2.5 py-1 text-xs font-medium ${RISK_STYLE[finding.risk]}`}
              >
                {RISK_LABEL[finding.risk]}
              </span>
              {finding.requires_admin ? (
                <span className="rounded-full bg-slate-100 px-2.5 py-1 text-xs text-slate-700 dark:bg-slate-500/15 dark:text-slate-200">
                  需要管理员
                </span>
              ) : null}
            </div>
            <p className="mt-1 text-sm text-emerald-900/55 dark:text-emerald-100/55">
              {formatBytes(finding.allocated_bytes)} · {finding.file_count.toLocaleString()} 个文件 ·{" "}
              {RECOVER_LABEL[finding.recoverability]}
            </p>
            <p className="mt-1 font-mono text-xs text-emerald-900/40 dark:text-emerald-100/40">
              {finding.rule_id} v{finding.rule_version} · 大小来源：{finding.size_provenance}
            </p>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="rounded-lg p-1.5 text-emerald-900/50 hover:bg-emerald-900/5 dark:text-emerald-100/50 dark:hover:bg-emerald-100/10"
            aria-label="关闭证据面板"
          >
            <X className="h-5 w-5" />
          </button>
        </header>

        <div className="space-y-4 overflow-y-auto p-5">
          <section>
            <h3 className="text-sm font-semibold">清理后会发生什么</h3>
            <p className="mt-1 text-sm leading-6 text-emerald-900/70 dark:text-emerald-100/70">
              {finding.consequence || "规则未提供额外说明。"}
            </p>
          </section>

          <section>
            <div className="flex items-center gap-2">
              <h3 className="text-sm font-semibold">证据链</h3>
              <span className="text-xs text-emerald-900/45 dark:text-emerald-100/45">
                全部由本地规则与扫描器生成
              </span>
            </div>
            <div className="mt-2 space-y-2">
              {finding.evidence.map((evidence) => (
                <EvidenceRow key={evidence.id} evidence={evidence} />
              ))}
            </div>
          </section>

          {finding.sample_files.length > 0 ? (
            <section>
              <h3 className="text-sm font-semibold">样本文件（仅名称，不读取内容）</h3>
              <div className="mt-2 divide-y divide-emerald-900/10 rounded-xl border border-emerald-900/10 bg-white/60 text-sm dark:divide-emerald-100/10 dark:border-emerald-100/10 dark:bg-emerald-950/30">
                {finding.sample_files.map((file) => (
                  <div
                    key={file.name}
                    className="flex items-center justify-between gap-4 px-3 py-2"
                  >
                    <span className="truncate">{file.name}</span>
                    <span className="shrink-0 text-xs text-emerald-900/50 dark:text-emerald-100/50">
                      {formatBytes(file.bytes)}
                    </span>
                  </div>
                ))}
              </div>
            </section>
          ) : null}

          <section className="rounded-xl border border-emerald-900/10 bg-white/60 p-3 dark:border-emerald-100/10 dark:bg-emerald-950/30">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <div>
                <div className="flex items-center gap-2">
                  <Bot className="h-4 w-4 text-emerald-700 dark:text-emerald-300" />
                  <h3 className="text-sm font-semibold">AI 分析（可选）</h3>
                </div>
                <p className="mt-1 text-xs text-emerald-900/50 dark:text-emerald-100/50">
                  只发送脱敏元数据与证据，不发送文件内容；LLM 不能删除任何东西。
                </p>
              </div>
              <button
                type="button"
                onClick={handlePreview}
                disabled={aiBusy}
                className="inline-flex items-center gap-2 rounded-lg border border-emerald-700/30 px-3 py-1.5 text-xs font-medium text-emerald-800 hover:bg-emerald-50 disabled:cursor-not-allowed disabled:opacity-50 dark:border-emerald-300/30 dark:text-emerald-200 dark:hover:bg-emerald-500/10"
              >
                {aiBusy ? (
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                ) : (
                  <Sparkles className="h-3.5 w-3.5" />
                )}
                {assessment ? "重新分析" : "AI 分析"}
              </button>
            </div>

            {aiError ? (
              <div className="mt-3 flex items-start gap-2 rounded-lg bg-rose-50 p-2.5 text-xs text-rose-800 dark:bg-rose-500/10 dark:text-rose-200">
                <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
                <span>{aiError}（如未配置，请到设置页填写 OpenAI 兼容 provider 与 API Key）</span>
              </div>
            ) : null}

            {payloadPreview ? (
              <div className="mt-3 space-y-2 rounded-lg border border-emerald-900/10 p-3 dark:border-emerald-100/10">
                <p className="text-xs font-medium">即将发送的完整 payload（可检查/复制）</p>
                <pre className="max-h-56 overflow-auto rounded-lg bg-emerald-950/5 p-2 text-[11px] leading-5 dark:bg-emerald-100/5">
                  {JSON.stringify(payloadPreview, null, 2)}
                </pre>
                <label className="flex items-start gap-2 text-xs">
                  <input
                    type="checkbox"
                    checked={consent}
                    onChange={(event) => setConsent(event.target.checked)}
                    className="mt-0.5 h-3.5 w-3.5 accent-emerald-700"
                  />
                  我同意将以上脱敏元数据发送给当前配置的 LLM provider。
                </label>
                <div className="flex gap-2">
                  <button
                    type="button"
                    onClick={handleAnalyze}
                    disabled={!consent || aiBusy}
                    className="inline-flex items-center gap-2 rounded-lg bg-emerald-700 px-3 py-1.5 text-xs font-medium text-white hover:bg-emerald-800 disabled:cursor-not-allowed disabled:opacity-50 dark:bg-emerald-500 dark:text-emerald-950"
                  >
                    {aiBusy ? (
                      <Loader2 className="h-3.5 w-3.5 animate-spin" />
                    ) : (
                      <Send className="h-3.5 w-3.5" />
                    )}
                    发送并分析
                  </button>
                  <button
                    type="button"
                    onClick={() => {
                      setPayloadPreview(null);
                      setConsent(false);
                    }}
                    disabled={aiBusy}
                    className="rounded-lg border border-emerald-900/15 px-3 py-1.5 text-xs font-medium text-emerald-800 hover:bg-emerald-50 disabled:opacity-40 dark:border-emerald-100/15 dark:text-emerald-200 dark:hover:bg-emerald-500/10"
                  >
                    取消
                  </button>
                </div>
              </div>
            ) : null}

            {assessment ? (
              <div className="mt-3 space-y-3 rounded-lg border border-emerald-900/10 p-3 dark:border-emerald-100/10">
                <div className="flex flex-wrap items-center gap-2">
                  <span
                    className={`rounded-full px-2 py-0.5 text-xs font-medium ${VERDICT_STYLE[assessment.verdict]}`}
                  >
                    {VERDICT_LABEL[assessment.verdict]}
                  </span>
                  <span className="text-xs text-emerald-900/50 dark:text-emerald-100/50">
                    置信度：{CONFIDENCE_LABEL[assessment.confidence]}
                  </span>
                  <span className="text-xs text-emerald-900/40 dark:text-emerald-100/40">
                    {assessment.model}
                  </span>
                </div>
                <p className="text-sm font-medium">{assessment.summary}</p>
                <div className="space-y-1.5">
                  {assessment.reasoning.map((step, index) => (
                    <div
                      key={`${step.text}-${index}`}
                      className="rounded-lg bg-emerald-50/70 p-2 dark:bg-emerald-500/5"
                    >
                      <p className="text-xs leading-5">{step.text}</p>
                      <div className="mt-1 flex flex-wrap gap-1">
                        {step.evidence_ids.map((id) => (
                          <span
                            key={id}
                            className="rounded bg-emerald-700/10 px-1.5 py-0.5 font-mono text-[10px] text-emerald-800 dark:bg-emerald-400/10 dark:text-emerald-200"
                          >
                            {id}
                          </span>
                        ))}
                      </div>
                    </div>
                  ))}
                </div>
                <p className="text-xs leading-5 text-emerald-900/70 dark:text-emerald-100/70">
                  <strong>影响：</strong>
                  {assessment.consequence}
                </p>
                {assessment.caveats.length > 0 ? (
                  <ul className="list-disc space-y-0.5 pl-5 text-xs leading-5 text-amber-800 dark:text-amber-200">
                    {assessment.caveats.map((caveat, index) => (
                      <li key={`${caveat}-${index}`}>{caveat}</li>
                    ))}
                  </ul>
                ) : null}
                <p className="text-[11px] text-emerald-900/40 dark:text-emerald-100/40">
                  AI 生成，仅供参考 · payload {assessment.payload_hash.slice(0, 12)}… ·
                  所有数字均由本地扫描器提供
                </p>

                <div className="border-t border-emerald-900/10 pt-3 dark:border-emerald-100/10">
                  <p className="text-xs font-medium">继续追问</p>
                  <div className="mt-1 flex gap-2">
                    <input
                      value={followup}
                      onChange={(event) => setFollowup(event.target.value)}
                      placeholder="例如：如果我现在正在使用这个应用，会有什么影响？"
                      className="min-w-0 flex-1 rounded-lg border border-emerald-900/15 bg-white px-2.5 py-1.5 text-xs outline-none focus:border-emerald-600 dark:border-emerald-100/15 dark:bg-emerald-950/50"
                    />
                    <button
                      type="button"
                      onClick={handleFollowup}
                      disabled={
                        followupBusy || followup.trim().length === 0
                      }
                      className="inline-flex items-center gap-1.5 rounded-lg bg-emerald-700 px-2.5 py-1.5 text-xs font-medium text-white hover:bg-emerald-800 disabled:cursor-not-allowed disabled:opacity-50 dark:bg-emerald-500 dark:text-emerald-950"
                    >
                      {followupBusy ? (
                        <Loader2 className="h-3.5 w-3.5 animate-spin" />
                      ) : (
                        <Send className="h-3.5 w-3.5" />
                      )}
                    </button>
                  </div>
                  {followupAnswer ? (
                    <p className="mt-2 rounded-lg bg-emerald-50 p-2 text-xs leading-5 text-emerald-900/75 dark:bg-emerald-500/10 dark:text-emerald-100/75">
                      {followupAnswer}
                    </p>
                  ) : null}
                </div>
              </div>
            ) : null}

            <p className="mt-3 text-[11px] leading-5 text-emerald-900/45 dark:text-emerald-100/45">
              即使启用 LLM，它也只能引用上述证据进行标注，不能新增路径、命令或删除动作；所有数字以扫描器为准。
            </p>
          </section>
        </div>
      </div>
    </div>
  );
}

function PlanPreviewModal({
  preview,
  confirmed,
  executing,
  onConfirmedChange,
  onCancel,
  onExecute,
}: {
  preview: PlanPreview;
  confirmed: boolean;
  executing: boolean;
  onConfirmedChange: (value: boolean) => void;
  onCancel: () => void;
  onExecute: () => void;
}) {
  const hasIrreversible = preview.items.some(
    (item) => item.recoverability === "irreversible",
  );
  const hasAdmin = preview.items.some((item) => item.requires_admin);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-emerald-950/30 p-4 backdrop-blur-sm">
      <div className="flex max-h-[90vh] w-full max-w-2xl flex-col overflow-hidden rounded-2xl border border-emerald-900/15 bg-[#f7faf8] shadow-2xl dark:border-emerald-100/15 dark:bg-[#121b16]">
        <header className="flex items-start justify-between gap-4 border-b border-emerald-900/10 p-5 dark:border-emerald-100/10">
          <div>
            <h2 className="text-lg font-semibold">确认清理计划</h2>
            <p className="mt-1 text-sm text-emerald-900/55 dark:text-emerald-100/55">
              {formatBytes(preview.total_bytes)} ·{" "}
              {preview.file_count.toLocaleString()} 个文件 · {preview.items.length} 项
            </p>
          </div>
          <button
            type="button"
            onClick={onCancel}
            disabled={executing}
            className="rounded-lg p-1.5 text-emerald-900/50 hover:bg-emerald-900/5 disabled:opacity-40 dark:text-emerald-100/50 dark:hover:bg-emerald-100/10"
            aria-label="取消清理计划"
          >
            <X className="h-5 w-5" />
          </button>
        </header>

        <div className="space-y-4 overflow-y-auto p-5">
          {preview.warnings.length > 0 ? (
            <div className="space-y-1 rounded-xl bg-amber-50 p-3 text-xs leading-5 text-amber-900 dark:bg-amber-500/10 dark:text-amber-200">
              {preview.warnings.map((warning) => (
                <p key={warning.code}>{warning.message_zh}</p>
              ))}
            </div>
          ) : null}

          <div className="divide-y divide-emerald-900/10 rounded-xl border border-emerald-900/10 bg-white/60 dark:divide-emerald-100/10 dark:border-emerald-100/10 dark:bg-emerald-950/30">
            {preview.items.map((item) => (
              <div
                key={item.finding_id}
                className="flex items-center justify-between gap-4 px-3 py-3"
              >
                <div className="min-w-0">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="truncate font-medium">{item.title_zh}</span>
                    <span
                      className={`rounded-full px-2 py-0.5 text-xs font-medium ${RISK_STYLE[item.risk]}`}
                    >
                      {RISK_LABEL[item.risk]}
                    </span>
                    {item.requires_admin ? (
                      <span className="rounded-full bg-slate-100 px-2 py-0.5 text-xs text-slate-700 dark:bg-slate-500/15 dark:text-slate-200">
                        管理员 + UAC
                      </span>
                    ) : null}
                  </div>
                  <div className="mt-0.5 truncate text-xs text-emerald-900/45 dark:text-emerald-100/45">
                    {item.rule_id} · {RECOVER_LABEL[item.recoverability]}
                  </div>
                </div>
                <div className="shrink-0 text-right">
                  <div className="font-semibold">
                    {item.requires_admin && item.bytes === 0
                      ? "待提权实测"
                      : formatBytes(item.bytes)}
                  </div>
                  <div className="text-xs text-emerald-900/45 dark:text-emerald-100/45">
                    {item.requires_admin && item.file_count === 0
                      ? "执行时扫描"
                      : `${item.file_count.toLocaleString()} 个文件`}
                  </div>
                </div>
              </div>
            ))}
          </div>

          <div className="rounded-xl bg-emerald-100/60 p-3 text-xs leading-5 text-emerald-900/75 dark:bg-emerald-500/10 dark:text-emerald-100/75">
            <Info className="mb-1 h-4 w-4" />
            缓存类文件会进入回收站（可在系统回收站恢复）；下次使用时应用会自动重建或重新下载。
            每个项目的证据链可在清理前再次查看。所有操作写入本地审计日志。
          </div>

          {hasAdmin ? (
            <div className="rounded-xl bg-amber-50 p-3 text-xs leading-5 text-amber-900 dark:bg-amber-500/10 dark:text-amber-200">
              <AlertTriangle className="mb-1 h-4 w-4" />
              计划包含管理员项。执行时会弹出 Windows UAC 授权框；请在弹窗中允许
              disksage-helper.exe。helper 会重新扫描管理员目录、再次校验红线，并只执行规则定义的动作。
              管理员项的预览大小可能为 0 或不完整。
            </div>
          ) : null}

          {hasIrreversible ? (
            <div className="rounded-xl bg-rose-50 p-3 text-xs text-rose-800 dark:bg-rose-500/10 dark:text-rose-200">
              计划中包含不可恢复项目，请确认后再执行。
            </div>
          ) : null}
        </div>

        <footer className="border-t border-emerald-900/10 p-5 dark:border-emerald-100/10">
          <label className="flex items-start gap-2 text-sm">
            <input
              type="checkbox"
              checked={confirmed}
              onChange={(event) => onConfirmedChange(event.target.checked)}
              disabled={executing}
              className="mt-0.5 h-4 w-4 accent-emerald-700"
            />
            我确认清理以上项目，并已了解每项的影响与恢复方式。
          </label>
          <div className="mt-4 flex justify-end gap-2">
            <button
              type="button"
              onClick={onCancel}
              disabled={executing}
              className="rounded-xl border border-emerald-900/15 px-4 py-2 text-sm font-medium text-emerald-800 hover:bg-emerald-50 disabled:opacity-40 dark:border-emerald-100/15 dark:text-emerald-200 dark:hover:bg-emerald-500/10"
            >
              取消
            </button>
            <button
              type="button"
              onClick={onExecute}
              disabled={!confirmed || executing}
              className="inline-flex items-center gap-2 rounded-xl bg-emerald-700 px-4 py-2 text-sm font-medium text-white hover:bg-emerald-800 disabled:cursor-not-allowed disabled:opacity-50 dark:bg-emerald-500 dark:text-emerald-950 dark:hover:bg-emerald-400"
            >
              {executing ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <Trash2 className="h-4 w-4" />
              )}
              {executing ? "正在清理…" : "执行清理"}
            </button>
          </div>
        </footer>
      </div>
    </div>
  );
}

export default function CleanupPage() {
  const [findings, setFindings] = useState<Finding[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [scanning, setScanning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [active, setActive] = useState<Finding | null>(null);
  const [scanned, setScanned] = useState(false);
  const [preview, setPreview] = useState<PlanPreview | null>(null);
  const [confirmed, setConfirmed] = useState(false);
  const [previewing, setPreviewing] = useState(false);
  const [executing, setExecuting] = useState(false);
  const [result, setResult] = useState<CleanupResult | null>(null);

  async function handleScan() {
    setScanning(true);
    setError(null);
    setResult(null);
    try {
      const findingsResult = await quickScan();
      setFindings(findingsResult);
      setSelected(
        new Set(
          findingsResult
            .filter((finding) => finding.default_selected && finding.risk === "safe")
            .map((finding) => finding.id),
        ),
      );
      setScanned(true);
    } catch (scanError) {
      setError(String(scanError));
    } finally {
      setScanning(false);
    }
  }

  async function handlePreview() {
    if (selected.size === 0) return;
    setPreviewing(true);
    setError(null);
    try {
      const plan = await createPlan([...selected]);
      setPreview(plan);
      setConfirmed(false);
    } catch (previewError) {
      setError(String(previewError));
    } finally {
      setPreviewing(false);
    }
  }

  async function handleExecute() {
    if (!preview || !confirmed) return;
    setExecuting(true);
    setError(null);
    try {
      const cleanupResult = await executePlan(preview.plan_id);
      setResult(cleanupResult);
      setPreview(null);
      setConfirmed(false);
      setFindings([]);
      setSelected(new Set());
      setScanned(false);
    } catch (executeError) {
      setError(String(executeError));
    } finally {
      setExecuting(false);
    }
  }

  const selectedFindings = useMemo(
    () => findings.filter((finding) => selected.has(finding.id)),
    [findings, selected],
  );
  const selectedBytes = selectedFindings.reduce(
    (total, finding) => total + finding.allocated_bytes,
    0,
  );
  const selectedFiles = selectedFindings.reduce(
    (total, finding) => total + finding.file_count,
    0,
  );

  const totals = useMemo(() => {
    const byRisk: Record<RiskLevel, { count: number; bytes: number }> = {
      safe: { count: 0, bytes: 0 },
      caution: { count: 0, bytes: 0 },
      danger: { count: 0, bytes: 0 },
    };
    for (const finding of findings) {
      byRisk[finding.risk].count += 1;
      byRisk[finding.risk].bytes += finding.allocated_bytes;
    }
    return byRisk;
  }, [findings]);

  function toggle(id: string) {
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  return (
    <div className="space-y-5">
      {result ? (
        <section className="rounded-2xl border border-emerald-700/20 bg-emerald-50 p-5 dark:border-emerald-400/20 dark:bg-emerald-500/10">
          <div className="flex items-start gap-3">
            <CheckCircle2 className="mt-0.5 h-5 w-5 shrink-0 text-emerald-700 dark:text-emerald-300" />
            <div className="min-w-0 flex-1">
              <h2 className="font-semibold">
                {result.cancelled ? "清理已取消" : "清理完成"}
              </h2>
              <p className="mt-1 text-sm text-emerald-900/70 dark:text-emerald-100/70">
                已删除 {result.deleted.toLocaleString()} 个文件，合计{" "}
                {formatBytes(result.freed_bytes)}。
              </p>
              {result.skipped_locked.length > 0 ? (
                <p className="mt-1 text-sm text-amber-700 dark:text-amber-300">
                  {result.skipped_locked.length.toLocaleString()} 个文件正被占用，已跳过；关闭相关应用后可再次扫描。
                </p>
              ) : null}
              {result.failed.length > 0 ? (
                <details className="mt-2 text-xs text-rose-700 dark:text-rose-300">
                  <summary className="cursor-pointer">
                    {result.failed.length} 个文件删除失败
                  </summary>
                  <ul className="mt-1 space-y-0.5">
                    {result.failed.slice(0, 10).map(([path, message]) => (
                      <li key={path} className="break-all">
                        {path}: {message}
                      </li>
                    ))}
                  </ul>
                </details>
              ) : null}
              {result.notes.length > 0 ? (
                <details className="mt-2 text-xs text-emerald-900/60 dark:text-emerald-100/60">
                  <summary className="cursor-pointer">
                    {result.notes.length} 条执行说明
                  </summary>
                  <ul className="mt-1 space-y-0.5">
                    {result.notes.map((note, index) => (
                      <li key={`${note}-${index}`} className="break-all">
                        • {note}
                      </li>
                    ))}
                  </ul>
                </details>
              ) : null}
              <button
                type="button"
                onClick={handleScan}
                disabled={scanning}
                className="mt-3 inline-flex items-center gap-2 rounded-lg border border-emerald-700/30 px-3 py-1.5 text-sm font-medium text-emerald-800 hover:bg-emerald-100 dark:border-emerald-300/30 dark:text-emerald-200 dark:hover:bg-emerald-500/10"
              >
                {scanning ? (
                  <Loader2 className="h-4 w-4 animate-spin" />
                ) : (
                  <FileSearch className="h-4 w-4" />
                )}
                重新扫描
              </button>
            </div>
          </div>
        </section>
      ) : null}

      <section className="rounded-2xl border border-emerald-900/10 bg-white p-6 shadow-sm dark:border-emerald-100/10 dark:bg-emerald-950/40">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div>
            <div className="flex items-center gap-2">
              <Sparkles className="h-5 w-5 text-emerald-700 dark:text-emerald-300" />
              <h2 className="text-lg font-semibold">快速扫描可清理项</h2>
            </div>
            <p className="mt-1 max-w-2xl text-sm leading-6 text-emerald-900/60 dark:text-emerald-100/60">
              只扫描已核验的缓存与临时目录；每一项都附带本地证据链、删除影响与恢复方式。
              管理员级系统清理将在 M4 通过独立 helper 接入。
            </p>
          </div>
          <button
            type="button"
            onClick={handleScan}
            disabled={scanning}
            className="inline-flex items-center gap-2 rounded-xl bg-emerald-700 px-4 py-2.5 text-sm font-medium text-white transition hover:bg-emerald-800 disabled:cursor-not-allowed disabled:opacity-60 dark:bg-emerald-500 dark:text-emerald-950 dark:hover:bg-emerald-400"
          >
            {scanning ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <FileSearch className="h-4 w-4" />
            )}
            {scanning ? "扫描中…" : "开始快速扫描"}
          </button>
        </div>

        {error ? (
          <div className="mt-4 flex items-start gap-2 rounded-xl bg-rose-50 p-3 text-sm text-rose-800 dark:bg-rose-500/10 dark:text-rose-200">
            <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
            {error}
          </div>
        ) : null}

        {scanned ? (
          <>
          <div className="mt-4 grid gap-3 sm:grid-cols-3">
            {(["safe", "caution", "danger"] as RiskLevel[]).map((risk) => (
              <div
                key={risk}
                className="rounded-xl border border-emerald-900/10 p-3 dark:border-emerald-100/10"
              >
                <div className="flex items-center justify-between">
                  <span className={`rounded-full px-2 py-0.5 text-xs font-medium ${RISK_STYLE[risk]}`}>
                    {RISK_LABEL[risk]}
                  </span>
                  <span className="text-xs text-emerald-900/45 dark:text-emerald-100/45">
                    {totals[risk].count} 项
                  </span>
                </div>
                <div className="mt-2 text-lg font-semibold">
                  {formatBytes(totals[risk].bytes)}
                </div>
              </div>
            ))}
          </div>
          <div className="mt-3 rounded-xl bg-emerald-50 p-3 text-sm leading-6 text-emerald-900/75 dark:bg-emerald-500/10 dark:text-emerald-100/75">
            <strong>可清理摘要：</strong>
            共发现 {findings.length} 项候选；安全项{" "}
            {formatBytes(totals.safe.bytes)}、谨慎项{" "}
            {formatBytes(totals.caution.bytes)}、危险项{" "}
            {formatBytes(totals.danger.bytes)}。建议先勾选安全项；谨慎项请逐项查看证据后再决定。
            当前已选 {selectedFindings.length} 项，合计{" "}
            {formatBytes(selectedBytes)}。
            {findings.some((finding) => finding.requires_admin) ? (
              <span className="mt-1 block text-xs text-amber-700 dark:text-amber-300">
                管理员项在预览中可能显示 0 B；执行时会通过 UAC 提权 helper 重新扫描并校验。
              </span>
            ) : null}
          </div>
          </>
        ) : null}
      </section>

      {scanned ? (
        <section className="overflow-hidden rounded-2xl border border-emerald-900/10 bg-white dark:border-emerald-100/10 dark:bg-emerald-950/40">
          <div className="flex items-center justify-between border-b border-emerald-900/10 px-5 py-3 dark:border-emerald-100/10">
            <div>
              <h3 className="font-semibold">清理候选</h3>
              <p className="text-xs text-emerald-900/50 dark:text-emerald-100/50">
                默认只勾选安全项；谨慎项需要你明确确认。
              </p>
            </div>
            <span className="text-xs text-emerald-900/45 dark:text-emerald-100/45">
              共 {findings.length} 项
            </span>
          </div>

          {findings.length === 0 ? (
            <div className="p-10 text-center text-sm text-emerald-900/55 dark:text-emerald-100/55">
              没有发现可清理项。可能相关缓存尚未生成，或目录已被清理。
            </div>
          ) : (
            <div className="divide-y divide-emerald-900/10 dark:divide-emerald-100/10">
              {findings.map((finding) => {
                const checked = selected.has(finding.id);
                return (
                  <div
                    key={finding.id}
                    className="flex flex-wrap items-center gap-4 px-5 py-3.5 transition hover:bg-emerald-50/50 dark:hover:bg-emerald-500/5"
                  >
                    <input
                      type="checkbox"
                      checked={checked}
                      onChange={() => toggle(finding.id)}
                      aria-label={`选择 ${finding.title_zh}`}
                      className="h-4 w-4 accent-emerald-700"
                    />
                    <div className="min-w-0 flex-1">
                      <div className="flex flex-wrap items-center gap-2">
                        <span className="font-medium">{finding.title_zh}</span>
                        <span
                          className={`rounded-full px-2 py-0.5 text-xs font-medium ${RISK_STYLE[finding.risk]}`}
                        >
                          {RISK_LABEL[finding.risk]}
                        </span>
                        {finding.requires_admin ? (
                          <span className="rounded-full bg-slate-100 px-2 py-0.5 text-xs text-slate-700 dark:bg-slate-500/15 dark:text-slate-200">
                            管理员
                          </span>
                        ) : null}
                      </div>
                      <div className="mt-0.5 truncate text-xs text-emerald-900/45 dark:text-emerald-100/45">
                        {finding.rule_id} · {finding.file_count.toLocaleString()} 个文件 ·{" "}
                        {RECOVER_LABEL[finding.recoverability]}
                      </div>
                    </div>
                    <div className="text-right">
                      <div className="font-semibold">
                        {finding.requires_admin && finding.allocated_bytes === 0
                          ? "待扫描"
                          : formatBytes(finding.allocated_bytes)}
                      </div>
                      <div className="text-xs text-emerald-900/45 dark:text-emerald-100/45">
                        {finding.requires_admin ? "提权后实测" : "measured"}
                      </div>
                    </div>
                    <button
                      type="button"
                      onClick={() => setActive(finding)}
                      className="inline-flex items-center gap-1 rounded-lg border border-emerald-900/15 px-2.5 py-1.5 text-xs font-medium text-emerald-800 hover:bg-emerald-50 dark:border-emerald-100/15 dark:text-emerald-200 dark:hover:bg-emerald-500/10"
                    >
                      证据
                      <ChevronRight className="h-3.5 w-3.5" />
                    </button>
                  </div>
                );
              })}
            </div>
          )}

          <div className="flex flex-wrap items-center justify-between gap-3 border-t border-emerald-900/10 bg-emerald-50/50 px-5 py-4 dark:border-emerald-100/10 dark:bg-emerald-500/5">
            <div className="flex items-center gap-2 text-sm">
              <ShieldCheck className="h-4 w-4 text-emerald-700 dark:text-emerald-300" />
              已选 <strong>{selectedFindings.length}</strong> 项 ·{" "}
              <strong>{formatBytes(selectedBytes)}</strong> ·{" "}
              {selectedFiles.toLocaleString()} 个文件
            </div>
            <button
              type="button"
              onClick={handlePreview}
              disabled={selected.size === 0 || previewing || scanning}
              className="inline-flex items-center gap-2 rounded-xl bg-emerald-700 px-4 py-2 text-sm font-medium text-white transition hover:bg-emerald-800 disabled:cursor-not-allowed disabled:opacity-50 dark:bg-emerald-500 dark:text-emerald-950 dark:hover:bg-emerald-400"
            >
              {previewing ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <Trash2 className="h-4 w-4" />
              )}
              {previewing ? "生成计划…" : "预览清理"}
            </button>
          </div>
        </section>
      ) : (
        <div className="flex min-h-[320px] flex-col items-center justify-center rounded-2xl border border-dashed border-emerald-900/20 bg-white/60 p-10 text-center dark:border-emerald-100/15 dark:bg-emerald-950/20">
          <ShieldCheck className="h-10 w-10 text-emerald-600/50 dark:text-emerald-400/50" />
          <h3 className="mt-4 font-semibold">先扫描，再决定</h3>
          <p className="mt-2 max-w-md text-sm leading-6 text-emerald-900/55 dark:text-emerald-100/55">
            快速扫描不会删除任何文件。每一项都会展示规则来源、实测大小、文件类型、影响与恢复方式。
          </p>
        </div>
      )}

      {active ? (
        <EvidenceDrawer finding={active} onClose={() => setActive(null)} />
      ) : null}

      {preview ? (
        <PlanPreviewModal
          preview={preview}
          confirmed={confirmed}
          executing={executing}
          onConfirmedChange={setConfirmed}
          onCancel={() => {
            if (executing) return;
            setPreview(null);
            setConfirmed(false);
          }}
          onExecute={handleExecute}
        />
      ) : null}
    </div>
  );
}
