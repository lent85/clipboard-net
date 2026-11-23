import { Check, FolderOpen, FileUp, FileDown, X } from "lucide-react";
import type { TransferInfo } from "../types";
import { formatBytes, formatSpeed } from "../api";

interface Props {
  transfers: TransferInfo[];
  onApprove: (id: string) => void;
  onReject: (id: string) => void;
  onCancel: (id: string) => void;
  onOpenFolder: (path: string) => void;
}

export function TransferStrip({
  transfers,
  onApprove,
  onReject,
  onCancel,
  onOpenFolder,
}: Props) {
  const visible = transfers.filter(
    (t) =>
      t.state === "pending_approval" ||
      t.state === "transferring" ||
      (t.state === "completed" && Date.now() - t.updated_at_ms < 25_000) ||
      (t.state === "failed" && Date.now() - t.updated_at_ms < 25_000)
  );

  if (visible.length === 0) return null;

  return (
    <div className="space-y-2">
      {visible.map((t) => {
        const pct =
          t.total_bytes > 0
            ? Math.min(100, Math.round((t.bytes_transferred / t.total_bytes) * 100))
            : 100;
        const isIncoming = t.direction === "incoming";
        const Icon = isIncoming ? FileDown : FileUp;
        const label =
          t.file_names.length === 1
            ? t.file_names[0]
            : `${t.file_count} files (${t.file_names.slice(0, 2).join(", ")}${
                t.file_count > 2 ? ", ..." : ""
              })`;

        const firstDest = t.dest_paths[0];
        const parentDir = firstDest
          ? firstDest.replace(/[\\/][^\\/]+$/, "")
          : "";

        return (
          <div
            key={t.transfer_id}
            className="rounded-xl border border-cyan-500/30 bg-white p-3 shadow-xs dark:bg-slate-900/90"
          >
            <div className="flex flex-wrap items-center justify-between gap-2">
              <div className="flex items-center gap-2.5 min-w-0">
                <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-cyan-500/15 text-cyan-600 dark:text-cyan-300">
                  <Icon className="h-4 w-4" />
                </div>
                <div className="min-w-0">
                  <div className="truncate text-xs font-semibold text-slate-900 dark:text-slate-100">
                    {isIncoming ? `Receiving from ${t.peer_name}` : `Sending to ${t.peer_name}`}:{" "}
                    <span className="font-normal text-slate-600 dark:text-slate-300">{label}</span>
                  </div>
                  <div className="mt-0.5 flex flex-wrap items-center gap-2 text-[11px] text-slate-500 dark:text-slate-400">
                    <span>
                      {formatBytes(t.bytes_transferred)} / {formatBytes(t.total_bytes)} ({pct}%)
                    </span>
                    {t.state === "transferring" && t.speed_bps > 0 && (
                      <span>
                        · {formatSpeed(t.speed_bps)}
                        {t.eta_secs !== null ? ` · ~${t.eta_secs}s left` : ""}
                      </span>
                    )}
                    {t.state === "completed" && (
                      <span className="font-medium text-emerald-600 dark:text-emerald-400">
                        · Completed & verified (SHA-256)
                      </span>
                    )}
                    {t.state === "failed" && (
                      <span className="font-medium text-rose-500">
                        · Failed: {t.error ?? "Transfer error"}
                      </span>
                    )}
                  </div>
                </div>
              </div>

              <div className="flex items-center gap-1.5">
                {t.state === "pending_approval" && isIncoming && (
                  <>
                    <button
                      type="button"
                      onClick={() => onApprove(t.transfer_id)}
                      className="inline-flex items-center gap-1 rounded-lg bg-emerald-500 px-2.5 py-1 text-xs font-semibold text-slate-950 hover:bg-emerald-400"
                    >
                      <Check className="h-3.5 w-3.5" />
                      Accept
                    </button>
                    <button
                      type="button"
                      onClick={() => onReject(t.transfer_id)}
                      className="inline-flex items-center gap-1 rounded-lg bg-slate-200 px-2.5 py-1 text-xs font-medium text-slate-700 hover:bg-slate-300 dark:bg-slate-800 dark:text-slate-300 dark:hover:bg-slate-700"
                    >
                      <X className="h-3.5 w-3.5" />
                      Decline
                    </button>
                  </>
                )}

                {t.state === "transferring" && (
                  <button
                    type="button"
                    onClick={() => onCancel(t.transfer_id)}
                    className="inline-flex items-center gap-1 rounded-lg border border-rose-500/30 px-2 py-1 text-xs font-medium text-rose-500 hover:bg-rose-500/10"
                  >
                    <X className="h-3.5 w-3.5" />
                    Cancel
                  </button>
                )}

                {t.state === "completed" && isIncoming && parentDir && (
                  <button
                    type="button"
                    onClick={() => onOpenFolder(parentDir)}
                    className="inline-flex items-center gap-1 rounded-lg bg-slate-100 px-2.5 py-1 text-xs font-medium text-slate-700 hover:bg-slate-200 dark:bg-slate-800 dark:text-slate-200 dark:hover:bg-slate-700"
                  >
                    <FolderOpen className="h-3.5 w-3.5" />
                    Open Folder
                  </button>
                )}
              </div>
            </div>

            {/* Progress Bar */}
            {(t.state === "transferring" || t.state === "completed") && (
              <div className="mt-2 h-1.5 w-full overflow-hidden rounded-full bg-slate-200 dark:bg-slate-800">
                <div
                  className={`h-full rounded-full transition-all duration-150 ${
                    t.state === "completed" ? "bg-emerald-500" : "bg-cyan-500"
                  }`}
                  style={{ width: `${pct}%` }}
                />
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}
