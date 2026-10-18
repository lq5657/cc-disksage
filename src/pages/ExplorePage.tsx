import { useEffect, useRef, useState } from "react";
import * as echarts from "echarts/core";
import { TreemapChart } from "echarts/charts";
import { TooltipComponent } from "echarts/components";
import { CanvasRenderer } from "echarts/renderers";
import { AlertTriangle, BarChart3, Loader2, ScanLine } from "lucide-react";
import {
  getTreemap,
  type ScanSnapshot,
  type TreemapNode,
  type TreemapResponse,
} from "../lib/api";
import { formatBytes } from "../lib/format";

echarts.use([TreemapChart, TooltipComponent, CanvasRenderer]);

function toSeriesData(node: TreemapNode): unknown {
  return {
    name: node.name,
    value: node.value,
    path: node.path,
    fileCount: node.file_count,
    children: node.children.map(toSeriesData),
  };
}

export default function ExplorePage({
  snapshot,
  scanning,
  onScan,
}: {
  snapshot: ScanSnapshot | null;
  scanning: boolean;
  onScan: () => void;
}) {
  const [treemap, setTreemap] = useState<TreemapResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const chartRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!snapshot) {
      setTreemap(null);
      return;
    }
    let cancelled = false;
    setLoading(true);
    setError(null);
    getTreemap(snapshot.id)
      .then((result) => {
        if (!cancelled) setTreemap(result);
      })
      .catch((loadError) => {
        if (!cancelled) setError(String(loadError));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [snapshot]);

  useEffect(() => {
    if (!treemap || !chartRef.current) return;

    const chart = echarts.init(chartRef.current);
    const option = {
      color: [
        "#047857",
        "#059669",
        "#10b981",
        "#34d399",
        "#0f766e",
        "#14b8a6",
        "#2dd4bf",
        "#5eead4",
        "#84cc16",
        "#65a30d",
      ],
      tooltip: {
        formatter: (info: any) => {
          const data = info.data ?? {};
          const path = data.path
            ? `<div style="font-size:11px;opacity:.65;margin-top:2px">${data.path}</div>`
            : "";
          return `<strong>${info.name}</strong><div>${formatBytes(
            Number(info.value) || 0,
          )}</div>${path}`;
        },
      },
      series: [
        {
          type: "treemap",
          data: treemap.root.children.map(toSeriesData),
          roam: false,
          nodeClick: "zoomToNode",
          breadcrumb: {
            show: true,
            bottom: 4,
            height: 22,
            itemStyle: { color: "rgba(4, 120, 87, 0.12)" },
            textStyle: { color: "#065f46", fontSize: 11 },
          },
          label: {
            show: true,
            formatter: "{b}",
            fontSize: 12,
            color: "#ffffff",
            overflow: "truncate",
          },
          upperLabel: {
            show: true,
            height: 24,
            color: "#ffffff",
            fontSize: 12,
          },
          itemStyle: {
            borderColor: "rgba(255,255,255,0.65)",
            borderWidth: 1,
            gapWidth: 1,
          },
          levels: [
            { itemStyle: { borderWidth: 0, gapWidth: 2 } },
            {
              colorSaturation: [0.35, 0.55],
              itemStyle: { borderWidth: 2, gapWidth: 1 },
            },
            { colorSaturation: [0.3, 0.45] },
          ],
        },
      ],
    } as echarts.EChartsCoreOption;

    chart.setOption(option);
    const observer = new ResizeObserver(() => chart.resize());
    observer.observe(chartRef.current);
    return () => {
      observer.disconnect();
      chart.dispose();
    };
  }, [treemap]);

  if (!snapshot) {
    return (
      <div className="flex min-h-[420px] flex-col items-center justify-center rounded-2xl border border-dashed border-emerald-900/20 bg-white/60 p-10 text-center dark:border-emerald-100/15 dark:bg-emerald-950/20">
        <BarChart3 className="h-10 w-10 text-emerald-600/50 dark:text-emerald-400/50" />
        <h2 className="mt-4 text-lg font-semibold">先做一次全盘扫描</h2>
        <p className="mt-2 max-w-md text-sm leading-6 text-emerald-900/55 dark:text-emerald-100/55">
          treemap 会展示 C 盘各目录的实际占用，支持点击下钻。扫描是只读的，不会修改任何文件。
        </p>
        <button
          type="button"
          onClick={onScan}
          disabled={scanning}
          className="mt-4 inline-flex items-center gap-2 rounded-xl bg-emerald-700 px-4 py-2.5 text-sm font-medium text-white hover:bg-emerald-800 disabled:cursor-not-allowed disabled:opacity-60 dark:bg-emerald-500 dark:text-emerald-950"
        >
          {scanning ? (
            <Loader2 className="h-4 w-4 animate-spin" />
          ) : (
            <ScanLine className="h-4 w-4" />
          )}
          {scanning ? "扫描中…" : "扫描系统盘"}
        </button>
      </div>
    );
  }

  return (
    <div className="space-y-5">
      <section className="flex flex-wrap items-center justify-between gap-4 rounded-2xl border border-emerald-900/10 bg-white p-5 dark:border-emerald-100/10 dark:bg-emerald-950/40">
        <div>
          <h2 className="text-lg font-semibold">空间分布</h2>
          <p className="mt-1 text-sm text-emerald-900/55 dark:text-emerald-100/55">
            共 {formatBytes(treemap?.total_bytes ?? snapshot.allocated_bytes)} ·{" "}
            {(treemap?.file_count ?? snapshot.file_count).toLocaleString()} 个文件 ·
            引擎：{snapshot.engine === "mft" ? "NTFS MFT" : "目录遍历"} ·
            点击区块可下钻
          </p>
        </div>
        <button
          type="button"
          onClick={onScan}
          disabled={scanning}
          className="inline-flex items-center gap-2 rounded-xl border border-emerald-700/30 px-3 py-2 text-sm font-medium text-emerald-800 hover:bg-emerald-50 disabled:opacity-50 dark:border-emerald-300/30 dark:text-emerald-200 dark:hover:bg-emerald-500/10"
        >
          {scanning ? (
            <Loader2 className="h-4 w-4 animate-spin" />
          ) : (
            <ScanLine className="h-4 w-4" />
          )}
          重新扫描
        </button>
      </section>

      {error ? (
        <div className="flex items-start gap-2 rounded-xl bg-rose-50 p-3 text-sm text-rose-800 dark:bg-rose-500/10 dark:text-rose-200">
          <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
          {error}
        </div>
      ) : null}

      <section className="relative overflow-hidden rounded-2xl border border-emerald-900/10 bg-white p-2 dark:border-emerald-100/10 dark:bg-emerald-950/40">
        {loading ? (
          <div className="absolute inset-0 z-10 flex items-center justify-center bg-white/70 dark:bg-emerald-950/70">
            <div className="flex items-center gap-2 text-sm text-emerald-900/60 dark:text-emerald-100/60">
              <Loader2 className="h-4 w-4 animate-spin" />
              正在生成 treemap…
            </div>
          </div>
        ) : null}
        <div ref={chartRef} className="h-[540px] w-full" />
      </section>

      <section className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
        {(treemap?.root.children ?? []).slice(0, 8).map((node) => (
          <div
            key={node.path}
            className="rounded-xl border border-emerald-900/10 bg-white p-3 dark:border-emerald-100/10 dark:bg-emerald-950/40"
          >
            <div className="truncate text-sm font-medium">{node.name}</div>
            <div className="mt-1 text-lg font-semibold">
              {formatBytes(node.value)}
            </div>
            <div className="text-xs text-emerald-900/45 dark:text-emerald-100/45">
              {node.file_count.toLocaleString()} 个文件
            </div>
          </div>
        ))}
      </section>

      {treemap?.truncated ? (
        <p className="text-xs text-amber-700 dark:text-amber-300">
          为保证流畅，treemap 仅渲染占用最大的目录层级；完整数据保留在本地扫描快照中。
        </p>
      ) : null}
    </div>
  );
}
