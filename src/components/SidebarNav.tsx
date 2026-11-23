import {
  ClipboardList,
  Laptop,
  Network,
  QrCode,
  Send,
  Settings,
} from "lucide-react";
import type { TelemetrySnapshot } from "../types";
import { osLabel } from "../api";

export type NavTab = "clipboard" | "devices" | "connections" | "settings";

interface Props {
  activeTab: NavTab;
  onSelectTab: (tab: NavTab) => void;
  snapshot: TelemetrySnapshot;
  onSendFilesClick: () => void;
  onShowQrClick: () => void;
}

export function SidebarNav({
  activeTab,
  onSelectTab,
  snapshot,
  onSendFilesClick,
  onShowQrClick,
}: Props) {
  const activeProfilesCount = snapshot.profile_statuses.filter(
    (p) => p.state !== "disabled"
  ).length;
  const hasDiagWarning =
    snapshot.clipboard_health.status !== "healthy" ||
    snapshot.embedded_hub.error !== null ||
    snapshot.profile_statuses.some((p) => p.state === "error");

  const items: {
    id: NavTab;
    label: string;
    icon: typeof ClipboardList;
    badge?: string | number;
    pulse?: boolean;
    warning?: boolean;
  }[] = [
    {
      id: "clipboard",
      label: "Clipboard",
      icon: ClipboardList,
      badge:
        snapshot.active_transfers_count > 0
          ? `${snapshot.active_transfers_count} xfer`
          : undefined,
      pulse: snapshot.active_transfers_count > 0,
    },
    {
      id: "devices",
      label: "Devices",
      icon: Laptop,
      badge:
        snapshot.pending_pairs_count > 0
          ? `${snapshot.pending_pairs_count} pair!`
          : snapshot.connected_peers_count > 0
          ? snapshot.connected_peers_count
          : snapshot.discovered_peers.length > 0
          ? `${snapshot.discovered_peers.length} near`
          : undefined,
      pulse: snapshot.pending_pairs_count > 0,
    },
    {
      id: "connections",
      label: "Connections",
      icon: Network,
      badge:
        snapshot.embedded_hub.running
          ? "Hub ON"
          : activeProfilesCount > 0
          ? activeProfilesCount
          : undefined,
    },
    {
      id: "settings",
      label: "Settings",
      icon: Settings,
      warning: hasDiagWarning,
    },
  ];

  return (
    <aside className="flex w-56 shrink-0 flex-col justify-between border-r border-slate-200 bg-slate-50/90 p-3 dark:border-slate-800/80 dark:bg-slate-900/60">
      <div>
        {/* Brand */}
        <div className="mb-5 flex items-center gap-2.5 px-2 pt-1">
          <div className="flex h-9 w-9 items-center justify-center rounded-xl bg-gradient-to-br from-cyan-400 to-sky-600 text-slate-950 shadow-sm">
            <Network className="h-5 w-5" />
          </div>
          <div>
            <div className="text-sm font-bold tracking-tight text-slate-900 dark:text-white">
              ClipboardNet
            </div>
            <div className="text-[11px] text-slate-500 dark:text-slate-400">
              Cross-OS Sync Mesh
            </div>
          </div>
        </div>

        {/* Navigation Links */}
        <nav className="space-y-1">
          {items.map((item) => {
            const Icon = item.icon;
            const active = activeTab === item.id;
            return (
              <button
                key={item.id}
                type="button"
                onClick={() => onSelectTab(item.id)}
                className={`flex w-full items-center justify-between rounded-xl px-3 py-2.5 text-sm font-medium transition ${
                  active
                    ? "bg-cyan-500/15 text-cyan-700 dark:bg-cyan-500/15 dark:text-cyan-300"
                    : "text-slate-600 hover:bg-slate-200/60 hover:text-slate-900 dark:text-slate-400 dark:hover:bg-slate-800/70 dark:hover:text-slate-100"
                }`}
              >
                <span className="flex items-center gap-2.5">
                  <Icon className="h-4 w-4" />
                  <span>{item.label}</span>
                </span>
                {item.badge !== undefined && (
                  <span
                    className={`rounded-full px-2 py-0.5 text-[10px] font-semibold ${
                      item.pulse
                        ? "animate-pulse bg-cyan-500 text-slate-950"
                        : "bg-slate-200 text-slate-700 dark:bg-slate-800 dark:text-slate-300"
                    }`}
                  >
                    {item.badge}
                  </span>
                )}
                {item.warning && (
                  <span className="h-2 w-2 rounded-full bg-amber-500" />
                )}
              </button>
            );
          })}
        </nav>
      </div>

      {/* Bottom Quick Actions + This Machine Card */}
      <div className="space-y-2.5">
        <div className="grid grid-cols-2 gap-1.5">
          <button
            type="button"
            onClick={onSendFilesClick}
            className="flex items-center justify-center gap-1.5 rounded-xl bg-cyan-500 px-2.5 py-2 text-xs font-semibold text-slate-950 shadow-sm transition hover:bg-cyan-400"
          >
            <Send className="h-3.5 w-3.5" />
            <span>Send File</span>
          </button>
          <button
            type="button"
            onClick={onShowQrClick}
            className="flex items-center justify-center gap-1.5 rounded-xl border border-slate-300 bg-white px-2.5 py-2 text-xs font-semibold text-slate-700 transition hover:bg-slate-100 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-200 dark:hover:bg-slate-700"
          >
            <QrCode className="h-3.5 w-3.5" />
            <span>QR / URI</span>
          </button>
        </div>

        <div className="rounded-xl border border-slate-200 bg-white/90 p-2.5 dark:border-slate-800 dark:bg-slate-950/70">
          <div className="flex items-center justify-between gap-2">
            <span className="truncate text-xs font-semibold text-slate-800 dark:text-slate-100">
              {snapshot.device_name}
            </span>
            <span className="shrink-0 rounded bg-slate-200 px-1.5 py-0.5 text-[10px] font-medium text-slate-700 dark:bg-slate-800 dark:text-slate-300">
              {osLabel(snapshot.os)}
            </span>
          </div>
          <div className="mt-1 truncate font-mono text-[10px] text-slate-400">
            FP: {snapshot.fingerprint}
          </div>
        </div>
      </div>
    </aside>
  );
}
