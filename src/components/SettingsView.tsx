import { useEffect, useState } from "react";
import {
  Activity,
  Check,
  Copy,
  FolderOpen,
  RefreshCw,
  Shield,
  Sliders,
  Terminal,
} from "lucide-react";
import type { AppConfig, TelemetrySnapshot, ThemeMode } from "../types";

interface Props {
  config: AppConfig;
  snapshot: TelemetrySnapshot;
  onSaveConfig: (cfg: AppConfig) => Promise<void>;
  onPickDownloadDir: () => Promise<string | null>;
  onRegenerateIdentity: () => Promise<void>;
  onExportDiagnostics: () => Promise<string>;
}

export function SettingsView({
  config,
  snapshot,
  onSaveConfig,
  onPickDownloadDir,
  onRegenerateIdentity,
  onExportDiagnostics,
}: Props) {
  const [draft, setDraft] = useState<AppConfig>(config);
  const [patternsText, setPatternsText] = useState(
    config.privacy.blocked_patterns.join("\n")
  );
  const [savedBanner, setSavedBanner] = useState(false);
  const [copiedDiag, setCopiedDiag] = useState(false);

  useEffect(() => {
    setDraft(config);
    setPatternsText(config.privacy.blocked_patterns.join("\n"));
  }, [config]);

  const commit = async (next: AppConfig) => {
    setDraft(next);
    await onSaveConfig(next);
    setSavedBanner(true);
    setTimeout(() => setSavedBanner(false), 1500);
  };

  const handleCopyDiag = async () => {
    const report = await onExportDiagnostics();
    await navigator.clipboard.writeText(report);
    setCopiedDiag(true);
    setTimeout(() => setCopiedDiag(false), 1800);
  };

  return (
    <div className="space-y-5">
      {savedBanner && (
        <div className="rounded-xl border border-emerald-500/30 bg-emerald-500/10 px-3 py-2 text-xs font-semibold text-emerald-700 dark:text-emerald-300">
          Settings saved and applied.
        </div>
      )}

      {/* General & Appearance */}
      <section className="rounded-2xl border border-slate-200 bg-white p-4 shadow-xs dark:border-slate-800 dark:bg-slate-900/80">
        <div className="mb-3 flex items-center gap-2">
          <Sliders className="h-4 w-4 text-cyan-500" />
          <h3 className="text-sm font-bold text-slate-900 dark:text-white">
            General & System Behavior
          </h3>
        </div>

        <div className="grid gap-3 sm:grid-cols-2 text-xs">
          <label className="block">
            <span className="font-semibold text-slate-700 dark:text-slate-300">
              Device Name
            </span>
            <input
              value={draft.identity.device_name}
              onChange={(e) =>
                setDraft({
                  ...draft,
                  identity: { ...draft.identity, device_name: e.target.value },
                })
              }
              className="mt-1 w-full rounded-lg border border-slate-300 bg-white px-3 py-1.5 dark:border-slate-700 dark:bg-slate-950"
            />
          </label>

          <label className="block">
            <span className="font-semibold text-slate-700 dark:text-slate-300">
              Theme
            </span>
            <select
              value={draft.ui.theme}
              onChange={(e) => {
                const next = {
                  ...draft,
                  ui: { ...draft.ui, theme: e.target.value as ThemeMode },
                };
                commit(next);
              }}
              className="mt-1 w-full rounded-lg border border-slate-300 bg-white px-3 py-1.5 dark:border-slate-700 dark:bg-slate-950"
            >
              <option value="dark">Dark</option>
              <option value="light">Light</option>
              <option value="system">System</option>
            </select>
          </label>
        </div>

        <div className="mt-3 grid gap-2 sm:grid-cols-2 text-xs">
          {(
            [
              ["minimize_to_tray", "Minimize to System Tray on close"],
              ["start_minimized", "Start minimized in Tray"],
              ["notifications_enabled", "Show desktop notifications for incoming syncs"],
              ["autostart", "Launch automatically on OS login"],
            ] as const
          ).map(([key, label]) => (
            <label key={key} className="flex items-center gap-2">
              <input
                type="checkbox"
                checked={draft.ui[key]}
                onChange={(e) => {
                  const next = {
                    ...draft,
                    ui: { ...draft.ui, [key]: e.target.checked },
                  };
                  commit(next);
                }}
                className="h-4 w-4 accent-cyan-500"
              />
              <span className="text-slate-700 dark:text-slate-200">{label}</span>
            </label>
          ))}
        </div>

        <div className="mt-3 flex justify-end">
          <button
            type="button"
            onClick={() => commit(draft)}
            className="rounded-lg bg-cyan-500 px-3.5 py-1.5 text-xs font-bold text-slate-950 hover:bg-cyan-400"
          >
            Save General
          </button>
        </div>
      </section>

      {/* Formats & Limits */}
      <section className="rounded-2xl border border-slate-200 bg-white p-4 shadow-xs dark:border-slate-800 dark:bg-slate-900/80">
        <h3 className="mb-3 text-sm font-bold text-slate-900 dark:text-white">
          Formats, File Limits & Storage
        </h3>

        <div className="mb-3 flex flex-wrap gap-4 text-xs">
          {(
            [
              ["text", "Plain Text"],
              ["html", "Rich HTML"],
              ["image", "Images (PNG)"],
              ["files", "Files & Folders"],
            ] as const
          ).map(([key, label]) => (
            <label key={key} className="flex items-center gap-2">
              <input
                type="checkbox"
                checked={draft.formats[key]}
                onChange={(e) => {
                  const next = {
                    ...draft,
                    formats: { ...draft.formats, [key]: e.target.checked },
                  };
                  commit(next);
                }}
                className="h-4 w-4 accent-cyan-500"
              />
              <span className="font-medium text-slate-700 dark:text-slate-200">
                {label}
              </span>
            </label>
          ))}
        </div>

        <div className="grid gap-3 sm:grid-cols-3 text-xs">
          <label className="block">
            <span className="font-semibold">Auto-Accept File Limit (MB)</span>
            <input
              type="number"
              min={1}
              value={draft.formats.max_auto_sync_mb}
              onChange={(e) =>
                setDraft({
                  ...draft,
                  formats: {
                    ...draft.formats,
                    max_auto_sync_mb: Number(e.target.value) || 25,
                  },
                })
              }
              className="mt-1 w-full rounded-lg border border-slate-300 bg-white px-3 py-1.5 font-mono dark:border-slate-700 dark:bg-slate-950"
            />
          </label>
          <label className="block">
            <span className="font-semibold">Max Inline Image (MB)</span>
            <input
              type="number"
              min={1}
              value={draft.formats.max_inline_image_mb}
              onChange={(e) =>
                setDraft({
                  ...draft,
                  formats: {
                    ...draft.formats,
                    max_inline_image_mb: Number(e.target.value) || 10,
                  },
                })
              }
              className="mt-1 w-full rounded-lg border border-slate-300 bg-white px-3 py-1.5 font-mono dark:border-slate-700 dark:bg-slate-950"
            />
          </label>
          <label className="block">
            <span className="font-semibold">History Retention</span>
            <input
              type="number"
              min={10}
              value={draft.ui.history_limit}
              onChange={(e) =>
                setDraft({
                  ...draft,
                  ui: {
                    ...draft.ui,
                    history_limit: Number(e.target.value) || 100,
                  },
                })
              }
              className="mt-1 w-full rounded-lg border border-slate-300 bg-white px-3 py-1.5 font-mono dark:border-slate-700 dark:bg-slate-950"
            />
          </label>
        </div>

        <div className="mt-3 flex flex-wrap items-end gap-2 text-xs">
          <label className="flex-1 block">
            <span className="font-semibold">Received Files Folder</span>
            <input
              value={draft.ui.download_dir}
              onChange={(e) =>
                setDraft({
                  ...draft,
                  ui: { ...draft.ui, download_dir: e.target.value },
                })
              }
              className="mt-1 w-full rounded-lg border border-slate-300 bg-white px-3 py-1.5 font-mono dark:border-slate-700 dark:bg-slate-950"
            />
          </label>
          <button
            type="button"
            onClick={async () => {
              const dir = await onPickDownloadDir();
              if (dir) {
                const next = { ...draft, ui: { ...draft.ui, download_dir: dir } };
                commit(next);
              }
            }}
            className="inline-flex items-center gap-1 rounded-lg border border-slate-300 px-3 py-1.5 font-medium hover:bg-slate-100 dark:border-slate-700 dark:hover:bg-slate-800"
          >
            <FolderOpen className="h-3.5 w-3.5" />
            Browse
          </button>
          <button
            type="button"
            onClick={() => commit(draft)}
            className="rounded-lg bg-cyan-500 px-3.5 py-1.5 font-bold text-slate-950 hover:bg-cyan-400"
          >
            Apply Limits
          </button>
        </div>
      </section>

      {/* Privacy & Security */}
      <section className="rounded-2xl border border-slate-200 bg-white p-4 shadow-xs dark:border-slate-800 dark:bg-slate-900/80">
        <div className="mb-3 flex items-center justify-between">
          <div className="flex items-center gap-2">
            <Shield className="h-4 w-4 text-emerald-500" />
            <h3 className="text-sm font-bold text-slate-900 dark:text-white">
              Privacy & Identity
            </h3>
          </div>
          <button
            type="button"
            onClick={() => onRegenerateIdentity()}
            className="inline-flex items-center gap-1 rounded-lg border border-slate-300 px-2.5 py-1 text-xs font-medium text-slate-700 hover:bg-slate-100 dark:border-slate-700 dark:text-slate-200 dark:hover:bg-slate-800"
          >
            <RefreshCw className="h-3 w-3" />
            Regenerate Keypair
          </button>
        </div>

        <label className="mb-3 flex items-center gap-2 text-xs">
          <input
            type="checkbox"
            checked={draft.privacy.skip_password_managers}
            onChange={(e) => {
              const next = {
                ...draft,
                privacy: {
                  ...draft.privacy,
                  skip_password_managers: e.target.checked,
                },
              };
              commit(next);
            }}
            className="h-4 w-4 accent-cyan-500"
          />
          <span>
            Skip sensitive password-manager text secrets (KeePassXC / KDE / macOS Concealed / Windows ExcludeClipboardContent; images & files are never blocked)
          </span>
        </label>

        <label className="block text-xs">
          <span className="font-semibold">
            Blocked Regex Patterns (one per line, e.g. <code>^ghp_[A-Za-z0-9]+</code>)
          </span>
          <textarea
            rows={3}
            value={patternsText}
            onChange={(e) => setPatternsText(e.target.value)}
            className="mt-1 w-full rounded-lg border border-slate-300 bg-white p-2.5 font-mono text-xs dark:border-slate-700 dark:bg-slate-950"
          />
        </label>
        <div className="mt-2 flex items-center justify-between">
          <span className="font-mono text-[11px] text-slate-400">
            Identity Fingerprint: {snapshot.fingerprint}
          </span>
          <button
            type="button"
            onClick={() => {
              const patterns = patternsText
                .split("\n")
                .map((s) => s.trim())
                .filter(Boolean);
              commit({
                ...draft,
                privacy: { ...draft.privacy, blocked_patterns: patterns },
              });
            }}
            className="rounded-lg bg-cyan-500 px-3 py-1 text-xs font-bold text-slate-950 hover:bg-cyan-400"
          >
            Save Filters
          </button>
        </div>
      </section>

      {/* Diagnostics & Event Log */}
      <section className="rounded-2xl border border-slate-200 bg-white p-4 shadow-xs dark:border-slate-800 dark:bg-slate-900/80">
        <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
          <div className="flex items-center gap-2">
            <Terminal className="h-4 w-4 text-cyan-500" />
            <h3 className="text-sm font-bold text-slate-900 dark:text-white">
              Live Diagnostics & Event Log
            </h3>
          </div>
          <button
            type="button"
            onClick={handleCopyDiag}
            className="inline-flex items-center gap-1.5 rounded-lg border border-slate-300 px-2.5 py-1 text-xs font-medium text-slate-700 hover:bg-slate-100 dark:border-slate-700 dark:text-slate-200 dark:hover:bg-slate-800"
          >
            {copiedDiag ? (
              <Check className="h-3.5 w-3.5 text-emerald-500" />
            ) : (
              <Copy className="h-3.5 w-3.5" />
            )}
            {copiedDiag ? "Copied Report" : "Copy Diagnostics JSON"}
          </button>
        </div>

        <div className="mb-3 grid gap-2 rounded-xl bg-slate-100 p-3 text-xs sm:grid-cols-3 dark:bg-slate-950">
          <div>
            <span className="text-slate-400">Clipboard Backend:</span>{" "}
            <strong className="text-slate-800 dark:text-slate-100">
              {snapshot.clipboard_health.backend_name}
            </strong>
          </div>
          <div>
            <span className="text-slate-400">Display Session:</span>{" "}
            <strong className="text-slate-800 dark:text-slate-100">
              {snapshot.clipboard_health.session_type}
            </strong>
          </div>
          <div className="flex items-center gap-1">
            <Activity className="h-3.5 w-3.5 text-cyan-500" />
            <span>{snapshot.clipboard_health.detail}</span>
          </div>
        </div>

        <div className="max-h-52 overflow-y-auto rounded-xl border border-slate-200 bg-slate-950 p-3 font-mono text-[11px] text-slate-300 dark:border-slate-800">
          {snapshot.recent_logs
            .slice()
            .reverse()
            .map((log, i) => (
              <div key={i} className="py-0.5 leading-relaxed">
                <span className="text-slate-500">
                  {new Date(log.timestamp_ms).toLocaleTimeString()}
                </span>{" "}
                <span
                  className={
                    log.level === "error"
                      ? "text-rose-400"
                      : log.level === "info"
                      ? "text-cyan-400"
                      : "text-slate-400"
                  }
                >
                  [{log.level.toUpperCase()}]
                </span>{" "}
                <span>{log.message}</span>
              </div>
            ))}
          {snapshot.recent_logs.length === 0 && (
            <div className="text-slate-500">No logs recorded yet.</div>
          )}
        </div>
      </section>
    </div>
  );
}
