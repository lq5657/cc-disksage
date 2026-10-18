import { useEffect, useState } from "react";
import {
  AlertTriangle,
  CheckCircle2,
  KeyRound,
  Loader2,
  Save,
  ShieldCheck,
  Trash2,
  Zap,
} from "lucide-react";
import {
  llmClearKey,
  llmGetConfig,
  llmSetConfig,
  llmSetKey,
  llmTestConnection,
  type LlmConfigInput,
  type LlmConfigPublic,
} from "../lib/api";

function toInput(config: LlmConfigPublic): LlmConfigInput {
  return {
    provider_id: config.provider_id,
    base_url: config.base_url,
    model: config.model,
    temperature: config.temperature,
    max_tokens: config.max_tokens,
    timeout_secs: config.timeout_secs,
    redact_paths: config.redact_paths,
    send_sample_names: config.send_sample_names,
    sample_name_limit: config.sample_name_limit,
  };
}

const inputClass =
  "w-full rounded-xl border border-emerald-900/15 bg-white px-3 py-2 text-sm outline-none transition focus:border-emerald-600 focus:ring-2 focus:ring-emerald-600/20 dark:border-emerald-100/15 dark:bg-emerald-950/50";

export default function SettingsPage() {
  const [config, setConfig] = useState<LlmConfigPublic | null>(null);
  const [draft, setDraft] = useState<LlmConfigInput | null>(null);
  const [apiKey, setApiKey] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    llmGetConfig()
      .then((value) => {
        if (cancelled) return;
        setConfig(value);
        setDraft(toInput(value));
      })
      .catch((loadError) => {
        if (!cancelled) setError(String(loadError));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  function update(patch: Partial<LlmConfigInput>) {
    setDraft((current) => (current ? { ...current, ...patch } : current));
    setStatus(null);
  }

  function selectPreset(providerId: string) {
    if (!draft || !config) return;
    const preset = config.presets.find((item) => item.id === providerId);
    if (!preset) return;
    update({
      provider_id: preset.id,
      base_url: preset.base_url,
      model: preset.model,
    });
  }

  async function handleSaveConfig() {
    if (!draft) return;
    setBusy("config");
    setError(null);
    setStatus(null);
    try {
      const saved = await llmSetConfig(draft);
      setConfig(saved);
      setDraft(toInput(saved));
      setStatus("设置已保存。");
    } catch (saveError) {
      setError(String(saveError));
    } finally {
      setBusy(null);
    }
  }

  async function handleSaveKey() {
    if (!draft || apiKey.trim().length === 0) return;
    setBusy("key");
    setError(null);
    setStatus(null);
    try {
      const saved = await llmSetConfig(draft);
      await llmSetKey(saved.provider_id, apiKey.trim());
      setApiKey("");
      const refreshed = await llmGetConfig();
      setConfig(refreshed);
      setDraft(toInput(refreshed));
      setStatus("设置已保存，API Key 已写入 Windows 凭据管理器。");
    } catch (keyError) {
      setError(String(keyError));
    } finally {
      setBusy(null);
    }
  }

  async function handleClearKey() {
    if (!draft) return;
    setBusy("clear");
    setError(null);
    setStatus(null);
    try {
      await llmSetConfig(draft);
      await llmClearKey(draft.provider_id);
      const refreshed = await llmGetConfig();
      setConfig(refreshed);
      setDraft(toInput(refreshed));
      setStatus("该 provider 的 API Key 已删除。");
    } catch (clearError) {
      setError(String(clearError));
    } finally {
      setBusy(null);
    }
  }

  async function handleTest() {
    if (!draft) return;
    setBusy("test");
    setError(null);
    setStatus(null);
    try {
      await llmSetConfig(draft);
      await llmTestConnection();
      setStatus("连接成功：provider 已返回有效响应。");
    } catch (testError) {
      setError(String(testError));
    } finally {
      setBusy(null);
    }
  }

  if (!draft || !config) {
    return (
      <div className="flex min-h-[320px] items-center justify-center rounded-2xl border border-dashed border-emerald-900/20 bg-white/60 p-10 dark:border-emerald-100/15 dark:bg-emerald-950/20">
        {error ? (
          <div className="flex items-start gap-2 text-sm text-rose-700 dark:text-rose-300">
            <AlertTriangle className="mt-0.5 h-4 w-4" />
            {error}
          </div>
        ) : (
          <div className="flex items-center gap-2 text-sm text-emerald-900/60 dark:text-emerald-100/60">
            <Loader2 className="h-4 w-4 animate-spin" />
            正在读取 LLM 设置…
          </div>
        )}
      </div>
    );
  }

  return (
    <div className="space-y-5">
      <section className="rounded-2xl border border-emerald-900/10 bg-white p-6 shadow-sm dark:border-emerald-100/10 dark:bg-emerald-950/40">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div>
            <div className="flex items-center gap-2">
              <ShieldCheck className="h-5 w-5 text-emerald-700 dark:text-emerald-300" />
              <h2 className="text-lg font-semibold">LLM 智能分析（可选）</h2>
            </div>
            <p className="mt-1 max-w-2xl text-sm leading-6 text-emerald-900/60 dark:text-emerald-100/60">
              默认关闭。只有你在证据面板中点击「AI 分析」并确认发送内容后，才会向 provider 发起请求。
              LLM 只能引用本地证据并给出判断，不能新增路径、命令或删除动作；所有数字由扫描器提供。
            </p>
          </div>
          <span
            className={`rounded-full px-3 py-1 text-xs font-medium ${
              config.has_key
                ? "bg-emerald-100 text-emerald-800 dark:bg-emerald-500/15 dark:text-emerald-200"
                : "bg-amber-100 text-amber-900 dark:bg-amber-500/15 dark:text-amber-200"
            }`}
          >
            {config.has_key ? "已配置 API Key" : "未配置 API Key"}
          </span>
        </div>

        {error ? (
          <div className="mt-4 flex items-start gap-2 rounded-xl bg-rose-50 p-3 text-sm text-rose-800 dark:bg-rose-500/10 dark:text-rose-200">
            <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
            {error}
          </div>
        ) : null}
        {status ? (
          <div className="mt-4 flex items-start gap-2 rounded-xl bg-emerald-50 p-3 text-sm text-emerald-800 dark:bg-emerald-500/10 dark:text-emerald-200">
            <CheckCircle2 className="mt-0.5 h-4 w-4 shrink-0" />
            {status}
          </div>
        ) : null}

        <div className="mt-5 grid gap-4 md:grid-cols-2">
          <label className="space-y-1.5">
            <span className="text-sm font-medium">Provider 预设</span>
            <select
              value={
                config.presets.some(
                  (preset) => preset.id === draft.provider_id,
                )
                  ? draft.provider_id
                  : "__custom__"
              }
              onChange={(event) => {
                if (event.target.value === "__custom__") {
                  update({ provider_id: "custom" });
                } else {
                  selectPreset(event.target.value);
                }
              }}
              className={inputClass}
            >
              {config.presets.map((preset) => (
                <option key={preset.id} value={preset.id}>
                  {preset.name_zh}
                </option>
              ))}
              <option value="__custom__">自定义</option>
            </select>
          </label>

          <label className="space-y-1.5">
            <span className="text-sm font-medium">模型名称</span>
            <input
              value={draft.model}
              onChange={(event) => update({ model: event.target.value })}
              className={inputClass}
              placeholder="例如 deepseek-chat"
            />
          </label>

          <label className="space-y-1.5 md:col-span-2">
            <span className="text-sm font-medium">Base URL（OpenAI 兼容）</span>
            <input
              value={draft.base_url}
              onChange={(event) => update({ base_url: event.target.value })}
              className={inputClass}
              placeholder="https://api.deepseek.com/v1"
            />
            <span className="block text-xs text-emerald-900/45 dark:text-emerald-100/45">
              除 localhost / 127.0.0.1 外，必须使用 HTTPS。Ollama 可填 http://127.0.0.1:11434/v1。
            </span>
          </label>

          <label className="space-y-1.5">
            <span className="text-sm font-medium">API Key</span>
            <div className="flex gap-2">
              <input
                type="password"
                value={apiKey}
                onChange={(event) => setApiKey(event.target.value)}
                className={inputClass}
                placeholder={config.has_key ? "已保存（输入新值可覆盖）" : "sk-..."}
                autoComplete="off"
              />
              <button
                type="button"
                onClick={handleSaveKey}
                disabled={apiKey.trim().length === 0 || busy !== null}
                className="inline-flex shrink-0 items-center gap-2 rounded-xl bg-emerald-700 px-3 py-2 text-sm font-medium text-white hover:bg-emerald-800 disabled:cursor-not-allowed disabled:opacity-50 dark:bg-emerald-500 dark:text-emerald-950"
              >
                {busy === "key" ? (
                  <Loader2 className="h-4 w-4 animate-spin" />
                ) : (
                  <KeyRound className="h-4 w-4" />
                )}
                保存
              </button>
            </div>
            <span className="block text-xs text-emerald-900/45 dark:text-emerald-100/45">
              Key 只写入 Windows 凭据管理器；不进入配置文件、日志或前端持久化。
            </span>
          </label>

          <div className="flex items-end gap-2">
            <button
              type="button"
              onClick={handleTest}
              disabled={busy !== null || !config.has_key}
              className="inline-flex items-center gap-2 rounded-xl border border-emerald-700/30 px-3 py-2 text-sm font-medium text-emerald-800 hover:bg-emerald-50 disabled:cursor-not-allowed disabled:opacity-50 dark:border-emerald-300/30 dark:text-emerald-200 dark:hover:bg-emerald-500/10"
            >
              {busy === "test" ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <Zap className="h-4 w-4" />
              )}
              测试连接
            </button>
            <button
              type="button"
              onClick={handleClearKey}
              disabled={busy !== null || !config.has_key}
              className="inline-flex items-center gap-2 rounded-xl border border-rose-300/50 px-3 py-2 text-sm font-medium text-rose-700 hover:bg-rose-50 disabled:cursor-not-allowed disabled:opacity-50 dark:border-rose-400/30 dark:text-rose-300 dark:hover:bg-rose-500/10"
            >
              {busy === "clear" ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <Trash2 className="h-4 w-4" />
              )}
              删除 Key
            </button>
          </div>
        </div>

        <div className="mt-5 space-y-3 rounded-xl border border-emerald-900/10 p-4 dark:border-emerald-100/10">
          <label className="flex items-start gap-3 text-sm">
            <input
              type="checkbox"
              checked={draft.redact_paths}
              onChange={(event) => update({ redact_paths: event.target.checked })}
              className="mt-0.5 h-4 w-4 accent-emerald-700"
            />
            <span>
              脱敏用户路径（推荐）
              <span className="block text-xs text-emerald-900/45 dark:text-emerald-100/45">
                把 C:\Users\&lt;name&gt; 等替换为 %USERPROFILE%，并去除用户名与计算机名。
              </span>
            </span>
          </label>
          <label className="flex items-start gap-3 text-sm">
            <input
              type="checkbox"
              checked={draft.send_sample_names}
              onChange={(event) =>
                update({
                  send_sample_names: event.target.checked,
                  sample_name_limit: event.target.checked
                    ? Math.max(draft.sample_name_limit, 5)
                    : 0,
                })
              }
              className="mt-0.5 h-4 w-4 accent-emerald-700"
            />
            <span>
              发送样本文件名
              <span className="block text-xs text-emerald-900/45 dark:text-emerald-100/45">
                默认关闭。开启后最多发送{" "}
                <input
                  type="number"
                  min={1}
                  max={10}
                  value={draft.sample_name_limit || 5}
                  onChange={(event) =>
                    update({
                      sample_name_limit: Math.min(
                        10,
                        Math.max(1, Number(event.target.value) || 1),
                      ),
                    })
                  }
                  className="mx-1 w-14 rounded border border-emerald-900/15 bg-white px-1 py-0.5 text-xs dark:border-emerald-100/15 dark:bg-emerald-950"
                />
                个文件名；文件内容永远不会发送。
              </span>
            </span>
          </label>
        </div>

        <details className="mt-4 rounded-xl border border-emerald-900/10 p-4 text-sm dark:border-emerald-100/10">
          <summary className="cursor-pointer font-medium">高级参数</summary>
          <div className="mt-3 grid gap-3 sm:grid-cols-3">
            <label className="space-y-1">
              <span className="text-xs">temperature</span>
              <input
                type="number"
                min={0}
                max={2}
                step={0.1}
                value={draft.temperature}
                onChange={(event) =>
                  update({ temperature: Number(event.target.value) })
                }
                className={inputClass}
              />
            </label>
            <label className="space-y-1">
              <span className="text-xs">max tokens</span>
              <input
                type="number"
                min={64}
                max={8192}
                value={draft.max_tokens}
                onChange={(event) =>
                  update({ max_tokens: Number(event.target.value) })
                }
                className={inputClass}
              />
            </label>
            <label className="space-y-1">
              <span className="text-xs">超时（秒）</span>
              <input
                type="number"
                min={5}
                max={300}
                value={draft.timeout_secs}
                onChange={(event) =>
                  update({ timeout_secs: Number(event.target.value) })
                }
                className={inputClass}
              />
            </label>
          </div>
        </details>

        <div className="mt-5 flex justify-end">
          <button
            type="button"
            onClick={handleSaveConfig}
            disabled={busy !== null}
            className="inline-flex items-center gap-2 rounded-xl bg-emerald-700 px-4 py-2.5 text-sm font-medium text-white hover:bg-emerald-800 disabled:cursor-not-allowed disabled:opacity-50 dark:bg-emerald-500 dark:text-emerald-950"
          >
            {busy === "config" ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <Save className="h-4 w-4" />
            )}
            保存设置
          </button>
        </div>
      </section>

      <section className="rounded-2xl border border-emerald-900/10 bg-white p-5 text-sm leading-6 text-emerald-900/65 dark:border-emerald-100/10 dark:bg-emerald-950/40 dark:text-emerald-100/65">
        <h3 className="font-semibold text-emerald-950 dark:text-emerald-50">
          数据边界
        </h3>
        <p className="mt-1">
          发送内容仅包括：规则 ID、路径模板（默认脱敏）、实测大小/文件数、时间范围、扩展名分布、证据描述，以及可选的样本文件名。
          永远不发送文件内容、注册表值、Cookie、密码、聊天数据库或完整用户路径。
        </p>
        <p className="mt-2">
          网络请求由 Rust 后端直接发起，前端无法读取 API Key。LLM 输出必须通过 evidence ID 白名单与「不含数字」校验，否则自动回退到本地证据说明。
        </p>
      </section>
    </div>
  );
}
