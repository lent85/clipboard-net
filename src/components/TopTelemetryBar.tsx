import { useState } from "react";
import {
  Activity,
  ArrowDownLeft,
  ArrowLeftRight,
  ArrowUpRight,
  Lock,
  MonitorCheck,
  Pause,
  ShieldAlert,
  Wifi,
} from "lucide-react";
import type { OverallStatus, SyncDirection, TelemetrySnapshot } from "../types";
import { formatRelativeTime } from "../api";

interface Props {
  snapshot: TelemetrySnapshot;
  onChangeDirection: (dir: SyncDirection) => void;
  onOpenSettingsDiagnostics: () => void;
}

const STATUS_META: Record<
  OverallStatus,
  { label: string; dot: string; ring: string; pill: string }
> = {
  connected: {
    label: "Connected",
    dot: "bg-emerald-500",
    ring: "bg-emerald-400",
    pill: "border-emerald-500/30 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300",
  },
  idle: {
    label: "Ready",
    dot: "bg-sky-500",
    ring: "bg-sky-400",
    pill: "border-sky-500/30 bg-sky-500/10 text-sky-700 dark:text-sky-300",
  },
  syncing: {
    label: "Syncing",
    dot: "bg-cyan-400",
    ring: "bg-cyan-300",
    pill: "border-cyan-500/30 bg-cyan-500/15 text-cyan-700 dark:text-cyan-200",
  },
  paused: {
    label: "Paused",
    dot: "bg-amber-500",
    ring: "bg-amber-400",
    pill: "border-amber-500/30 bg-amber-500/10 text-amber-700 dark:text-amber-300",
  },
  reconnecting: {
    label: "Reconnecting",
    dot: "bg-orange-500",
    ring: "bg-orange-400",
    pill: "border-orange-500/30 bg-orange-500/10 text-orange-700 dark:text-orange-300",
  },
  error: {
    label: "Attention",
    dot: "bg-rose-500",
    ring: "bg-rose-400",
    pill: "border-rose-500/30 bg-rose-500/10 text-rose-700 dark:text-rose-300",
  },
};

export function TopTelemetryBar({
  snapshot,
  onChangeDirection,
  onOpenSettingsDiagnostics,
}: Props) {
  const [showHealthPopover, setShowHealthPopover] = useState(false);
  const meta = STATUS_META[snapshot.overall_status];
  const cb = snapshot.clipboard_health;

  const healthDot =
    cb.status === "healthy"
      ? "bg-emerald-500"
      : cb.status === "degraded"
      ? "bg-amber-500"
      : "bg-rose-500";

  const directions: { id: SyncDirection; label: string; icon: typeof ArrowLeftRight }[] = [
    { id: "bidirectional", label: "Both", icon: ArrowLeftRight },
    { id: "send_only", label: "Send", icon: ArrowUpRight },
    { id: "receive_only", label: "Recv", icon: ArrowDownLeft },
    { id: "paused", label: "Pause", icon: Pause },
  ];

  return (
    <header className="relative z-20 flex flex-wrap items-center justify-between gap-3 border-b border-slate-200 bg-white/80 px-4 py-2.5 backdrop-blur dark:border-slate-800/80 dark:bg-slate-900/80">
      {/* Left: Live Overall Status + Peer Count + RTT */}
      <div className="flex items-center gap-2.5">
        <div
          className={`inline-flex items-center gap-2 rounded-full border px-3 py-1 text-xs font-semibold ${meta.pill}`}
        >
          <span className="relative flex h-2.5 w-2.5">
            {(snapshot.overall_status === "connected" ||
              snapshot.overall_status === "syncing") && (
              <span
                className={`animate-pulse-ring absolute inline-flex h-full w-full rounded-full ${meta.ring}`}
              />
            )}
            <span className={`relative inline-flex h-2.5 w-2.5 rounded-full ${meta.dot}`} />
          </span>
          <span>{meta.label}</span>
        </div>

        <div className="inline-flex items-center gap-1.5 rounded-full border border-slate-200 bg-slate-100/80 px-2.5 py-1 text-xs font-medium text-slate-700 dark:border-slate-800 dark:bg-slate-800/70 dark:text-slate-300">
          <Wifi className="h-3.5 w-3.5 text-cyan-500" />
          <span>
            {snapshot.connected_peers_count}{" "}
            {snapshot.connected_peers_count === 1 ? "peer" : "peers"}
          </span>
          {snapshot.avg_rtt_ms !== null && (
            <span className="ml-1 border-l border-slate-300 pl-1.5 font-mono text-[11px] text-slate-500 dark:border-slate-700 dark:text-slate-400">
              {snapshot.avg_rtt_ms} ms
            </span>
          )}
        </div>
      </div>

      {/* Center-Right: E2EE Badge + Clipboard Engine Health Pill */}
      <div className="flex items-center gap-2">
        <div
          title={`End-to-End Encrypted (AES-256-GCM)\nDevice Fingerprint: ${snapshot.fingerprint}`}
          className="hidden items-center gap-1.5 rounded-full border border-slate-200 bg-slate-100/70 px-2.5 py-1 text-[11px] font-medium text-slate-600 sm:inline-flex dark:border-slate-800 dark:bg-slate-800/60 dark:text-slate-300"
        >
          <Lock className="h-3 w-3 text-emerald-500" />
          <span>E2EE</span>
          <span className="font-mono text-[10px] text-slate-400">
            {snapshot.fingerprint.slice(0, 9)}
          </span>
        </div>

        <div className="relative">
          <button
            type="button"
            onClick={() => setShowHealthPopover((v) => !v)}
            className="inline-flex items-center gap-1.5 rounded-full border border-slate-200 bg-slate-100/80 px-2.5 py-1 text-xs font-medium text-slate-700 transition hover:border-cyan-500/40 hover:bg-slate-200/70 dark:border-slate-800 dark:bg-slate-800/70 dark:text-slate-200 dark:hover:bg-slate-800"
          >
            <span className={`h-2 w-2 rounded-full ${healthDot}`} />
            {cb.status === "healthy" ? (
              <MonitorCheck className="h-3.5 w-3.5 text-slate-400" />
            ) : (
              <ShieldAlert className="h-3.5 w-3.5 text-amber-400" />
            )}
            <span>{cb.backend_name}</span>
          </button>

          {showHealthPopover && (
            <div className="absolute right-0 mt-2 w-80 rounded-xl border border-slate-200 bg-white p-3.5 shadow-xl dark:border-slate-700 dark:bg-slate-900">
              <div className="mb-2 flex items-center justify-between">
                <span className="flex items-center gap-1.5 text-xs font-semibold text-slate-800 dark:text-slate-100">
                  <Activity className="h-3.5 w-3.5 text-cyan-500" />
                  Clipboard Engine Health
                </span>
                <span
                  className={`rounded-full px-2 py-0.5 text-[10px] font-semibold uppercase ${
                    cb.status === "healthy"
                      ? "bg-emerald-500/15 text-emerald-600 dark:text-emerald-300"
                      : cb.status === "degraded"
                      ? "bg-amber-500/15 text-amber-600 dark:text-amber-300"
                      : "bg-rose-500/15 text-rose-600 dark:text-rose-300"
                  }`}
                >
                  {cb.status}
                </span>
              </div>
              <dl className="space-y-1.5 text-xs">
                <div className="flex justify-between">
                  <dt className="text-slate-500 dark:text-slate-400">Backend</dt>
                  <dd className="font-medium text-slate-800 dark:text-slate-200">
                    {cb.backend_name}
                  </dd>
                </div>
                <div className="flex justify-between">
                  <dt className="text-slate-500 dark:text-slate-400">Session</dt>
                  <dd className="font-medium text-slate-800 dark:text-slate-200">
                    {cb.session_type}
                  </dd>
                </div>
                <div className="flex justify-between">
                  <dt className="text-slate-500 dark:text-slate-400">Last Capture</dt>
                  <dd className="text-slate-700 dark:text-slate-300">
                    {cb.last_capture_ms ? formatRelativeTime(cb.last_capture_ms) : "None yet"}
                  </dd>
                </div>
                <div className="flex justify-between">
                  <dt className="text-slate-500 dark:text-slate-400">Privacy Skipped</dt>
                  <dd className="text-slate-700 dark:text-slate-300">
                    {cb.skipped_privacy_count}
                  </dd>
                </div>
              </dl>
              <p className="mt-2.5 rounded-lg bg-slate-100 p-2 text-[11px] leading-relaxed text-slate-600 dark:bg-slate-800/80 dark:text-slate-300">
                {cb.detail}
              </p>
              <div className="mt-2.5 flex justify-end gap-2">
                <button
                  type="button"
                  onClick={() => {
                    setShowHealthPopover(false);
                    onOpenSettingsDiagnostics();
                  }}
                  className="text-xs font-medium text-cyan-600 hover:underline dark:text-cyan-400"
                >
                  View Diagnostics Logs
                </button>
                <button
                  type="button"
                  onClick={() => setShowHealthPopover(false)}
                  className="text-xs text-slate-500 hover:text-slate-700 dark:hover:text-slate-300"
                >
                  Close
                </button>
              </div>
            </div>
          )}
        </div>

        {/* 4-Way Sync Direction Control */}
        <div className="inline-flex rounded-lg border border-slate-200 bg-slate-100 p-0.5 dark:border-slate-800 dark:bg-slate-950">
          {directions.map((d) => {
            const Icon = d.icon;
            const active = snapshot.sync_direction === d.id;
            const activeClass =
              d.id === "paused"
                ? "bg-amber-500 text-slate-950 shadow-xs"
                : "bg-cyan-500 text-slate-950 shadow-xs";
            return (
              <button
                key={d.id}
                type="button"
                onClick={() => onChangeDirection(d.id)}
                title={`Sync Mode: ${d.label}`}
                className={`inline-flex items-center gap-1 rounded-md px-2 py-1 text-xs font-medium transition ${
                  active
                    ? activeClass
                    : "text-slate-600 hover:text-slate-900 dark:text-slate-400 dark:hover:text-slate-100"
                }`}
              >
                <Icon className="h-3.5 w-3.5" />
                <span className="hidden md:inline">{d.label}</span>
              </button>
            );
          })}
        </div>
      </div>
    </header>
  );
}
