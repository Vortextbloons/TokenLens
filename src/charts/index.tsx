import {
  ResponsiveContainer,
  AreaChart,
  Area,
  LineChart,
  Line,
  BarChart,
  Bar,
  PieChart,
  Pie,
  Cell,
  XAxis,
  YAxis,
  Tooltip,
  CartesianGrid,
  Legend,
} from "recharts";
import type { Breakdown, TimeseriesPoint } from "@/types/contracts";
import { formatNumber, formatUsd } from "@/lib/utils";

const TEAL = "#2dd4bf";
const CYAN = "#06b6d4";
const SLATE = "#64748b";
const AMBER = "#f59e0b";
const VIOLET = "#8b5cf6";
const ROSE = "#f43f5e";
const PALETTE = [TEAL, CYAN, AMBER, VIOLET, ROSE, SLATE, "#10b981", "#3b82f6"];

export function TokensAreaChart({ data }: { data: TimeseriesPoint[] }) {
  return (
    <ResponsiveContainer width="100%" height={280}>
      <AreaChart data={data} margin={{ top: 8, right: 8, left: 0, bottom: 0 }}>
        <defs>
          <linearGradient id="inG" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor={TEAL} stopOpacity={0.5} />
            <stop offset="100%" stopColor={TEAL} stopOpacity={0} />
          </linearGradient>
          <linearGradient id="outG" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor={CYAN} stopOpacity={0.5} />
            <stop offset="100%" stopColor={CYAN} stopOpacity={0} />
          </linearGradient>
          <linearGradient id="rG" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor={AMBER} stopOpacity={0.5} />
            <stop offset="100%" stopColor={AMBER} stopOpacity={0} />
          </linearGradient>
        </defs>
        <CartesianGrid strokeDasharray="3 3" stroke="hsl(var(--border))" vertical={false} />
        <XAxis dataKey="date" tick={{ fontSize: 11 }} stroke="hsl(var(--muted-foreground))" tickFormatter={(d) => d.slice(5)} />
        <YAxis tick={{ fontSize: 11 }} stroke="hsl(var(--muted-foreground))" tickFormatter={formatNumber} width={48} />
        <Tooltip
          contentStyle={{ backgroundColor: "hsl(var(--popover))", border: "1px solid hsl(var(--border))", borderRadius: 8, fontSize: 12 }}
          labelStyle={{ color: "hsl(var(--foreground))" }}
          formatter={(v: any) => formatNumber(Number(v))}
        />
        <Legend wrapperStyle={{ fontSize: 11 }} />
        <Area type="monotone" dataKey="input_tokens" stackId="1" stroke={TEAL} fill="url(#inG)" name="Input" />
        <Area type="monotone" dataKey="output_tokens" stackId="1" stroke={CYAN} fill="url(#outG)" name="Output" />
        <Area type="monotone" dataKey="reasoning_tokens" stackId="1" stroke={AMBER} fill="url(#rG)" name="Reasoning" />
      </AreaChart>
    </ResponsiveContainer>
  );
}

export function CostLineChart({ data }: { data: TimeseriesPoint[] }) {
  return (
    <ResponsiveContainer width="100%" height={280}>
      <LineChart data={data} margin={{ top: 8, right: 8, left: 0, bottom: 0 }}>
        <CartesianGrid strokeDasharray="3 3" stroke="hsl(var(--border))" vertical={false} />
        <XAxis dataKey="date" tick={{ fontSize: 11 }} stroke="hsl(var(--muted-foreground))" tickFormatter={(d) => d.slice(5)} />
        <YAxis tick={{ fontSize: 11 }} stroke="hsl(var(--muted-foreground))" tickFormatter={(v) => "$" + formatNumber(Number(v))} width={48} />
        <Tooltip
          contentStyle={{ backgroundColor: "hsl(var(--popover))", border: "1px solid hsl(var(--border))", borderRadius: 8, fontSize: 12 }}
          labelStyle={{ color: "hsl(var(--foreground))" }}
          formatter={(v: any) => formatUsd(Number(v))}
        />
        <Line type="monotone" dataKey="cost_usd" stroke={VIOLET} strokeWidth={2} dot={false} name="Cost (USD)" />
      </LineChart>
    </ResponsiveContainer>
  );
}

export function ModelBarChart({ data }: { data: Breakdown[] }) {
  const top = data.slice(0, 12);
  const labelWidth = top.length === 0
    ? 100
    : Math.min(220, Math.max(100, ...top.map((d) => d.key.length * 7)));
  return (
    <ResponsiveContainer width="100%" height={Math.max(220, top.length * 36 + 48)}>
      <BarChart data={top} layout="vertical" margin={{ top: 8, right: 24, left: 8, bottom: 0 }}>
        <CartesianGrid strokeDasharray="3 3" stroke="hsl(var(--border))" horizontal={false} />
        <XAxis type="number" tick={{ fontSize: 11 }} stroke="hsl(var(--muted-foreground))" tickFormatter={formatNumber} />
        <YAxis dataKey="key" type="category" tick={{ fontSize: 11 }} stroke="hsl(var(--muted-foreground))" width={labelWidth} />
        <Tooltip
          contentStyle={{ backgroundColor: "hsl(var(--popover))", border: "1px solid hsl(var(--border))", borderRadius: 8, fontSize: 12 }}
          formatter={(v: any) => formatNumber(Number(v))}
        />
        <Bar dataKey="total_tokens" name="Tokens" radius={[0, 4, 4, 0]}>
          {top.map((_, i) => <Cell key={i} fill={PALETTE[i % PALETTE.length]} />)}
        </Bar>
      </BarChart>
    </ResponsiveContainer>
  );
}

export function ProviderDonut({ data }: { data: Breakdown[] }) {
  const top = data.slice(0, 8);
  const total = top.reduce((a, b) => a + b.total_tokens, 0);
  return (
    <div className="flex flex-col sm:flex-row items-center gap-6 w-full">
      <div className="shrink-0">
        <PieChart width={200} height={200}>
          <Pie data={top} dataKey="total_tokens" nameKey="key" innerRadius={58} outerRadius={88} paddingAngle={2}>
            {top.map((_, i) => <Cell key={i} fill={PALETTE[i % PALETTE.length]} />)}
          </Pie>
          <Tooltip
            contentStyle={{ backgroundColor: "hsl(var(--popover))", border: "1px solid hsl(var(--border))", borderRadius: 8, fontSize: 12, color: "hsl(var(--popover-foreground))" }}
            labelStyle={{ color: "hsl(var(--popover-foreground))" }}
            itemStyle={{ color: "hsl(var(--popover-foreground))" }}
            formatter={(v: any) => formatNumber(Number(v))}
          />
        </PieChart>
      </div>
      <div className="flex-1 min-w-0 w-full grid grid-cols-1 sm:grid-cols-2 xl:grid-cols-1 2xl:grid-cols-2 gap-x-6 gap-y-1.5">
        {top.map((d, i) => (
          <div key={d.key} className="flex items-center gap-2 text-xs">
            <span className="h-2.5 w-2.5 rounded-sm shrink-0" style={{ backgroundColor: PALETTE[i % PALETTE.length] }} />
            <span className="truncate">{d.key}</span>
            <span className="ml-auto text-muted-foreground tabular-nums">{total > 0 ? ((d.total_tokens / total) * 100).toFixed(1) : 0}%</span>
          </div>
        ))}
      </div>
    </div>
  );
}

export function StackedBarChart({ data }: { data: TimeseriesPoint[] }) {
  return (
    <ResponsiveContainer width="100%" height={200}>
      <BarChart data={data} margin={{ top: 8, right: 8, left: 0, bottom: 0 }}>
        <CartesianGrid strokeDasharray="3 3" stroke="hsl(var(--border))" vertical={false} />
        <XAxis dataKey="date" tick={{ fontSize: 11 }} stroke="hsl(var(--muted-foreground))" tickFormatter={(d) => d.slice(5)} />
        <YAxis tick={{ fontSize: 11 }} stroke="hsl(var(--muted-foreground))" tickFormatter={formatNumber} width={48} />
        <Tooltip
          contentStyle={{ backgroundColor: "hsl(var(--popover))", border: "1px solid hsl(var(--border))", borderRadius: 8, fontSize: 12 }}
          formatter={(v: any) => formatNumber(Number(v))}
        />
        <Legend wrapperStyle={{ fontSize: 11 }} />
        <Bar dataKey="input_tokens" stackId="a" fill={TEAL} name="Input" />
        <Bar dataKey="output_tokens" stackId="a" fill={CYAN} name="Output" />
        <Bar dataKey="reasoning_tokens" stackId="a" fill={AMBER} name="Reasoning" />
        <Bar dataKey="cache_read_tokens" stackId="a" fill={VIOLET} name="Cache" />
      </BarChart>
    </ResponsiveContainer>
  );
}
