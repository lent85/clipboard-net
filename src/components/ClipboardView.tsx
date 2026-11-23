import { useMemo, useState } from "react";
import {
  Check,
  Code2,
  Copy,
  File as FileIcon,
  FolderOpen,
  Image as ImageIcon,
  Pin,
  RefreshCw,
  Search,
  Send,
  Trash2,
  Type,
  UploadCloud,
  X,
} from "lucide-react";
import type { HistoryEntry, HistoryItemKind, TransferInfo } from "../types";
import { formatBytes, formatRelativeTime, osLabel } from "../api";
import { TransferStrip } from "./TransferStrip";

interface Props {
  history: HistoryEntry[];
  transfers: TransferInfo[];
  onCopy: (id: string) => void;
  onResend: (id: string) => void;
  onTogglePin: (id: string) => void;
  onDelete: (id: string) => void;
  onClearUnpinned: () => void;
  onSendFilesClick: () => void;
  onApproveTransfer: (id: string) => void;
  onRejectTransfer: (id: string) => void;
  onCancelTransfer: (id: string) => void;
  onOpenFolder: (path: string) => void;
}

type FilterTab = "all" | HistoryItemKind | "pinned";

export function ClipboardView({
  history,
  transfers,
  onCopy,
  onResend,
  onTogglePin,
  onDelete,
  onClearUnpinned,
  onSendFilesClick,
  onApproveTransfer,
  onRejectTransfer,
  onCancelTransfer,
  onOpenFolder,
}: Props) {
  const [filter, setFilter] = useState<FilterTab>("all");
  const [query, setQuery] = useState("");
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [previewEntry, setPreviewEntry] = useState<HistoryEntry | null>(null);

  const latest = history[0] ?? null;

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return history.filter((entry) => {
      if (filter === "pinned" && !entry.pinned) return false;
      if (filter !== "all" && filter !== "pinned" && entry.kind !== filter) return false;
      if (!q) return true;
      return (
        entry.text_preview.toLowerCase().includes(q) ||
        entry.origin_device_name.toLowerCase().includes(q) ||
        entry.file_names.some((f) => f.toLowerCase().includes(q))
      );
    });
  }, [history, filter, query]);

  const handleCopy = (id: string) => {
    onCopy(id);
    setCopiedId(id);
    setTimeout(() => setCopiedId((prev) => (prev === id ? null : prev)), 1500);
  };

  const kindBadge = (kind: HistoryItemKind) => {
    switch (kind) {
      case "text":
        return {
          label: "Text",
          icon: Type,
          cls: "bg-slate-200/80 text-slate-700 dark:bg-slate-800 dark:text-slate-300",
        };
      case "html":
        return {
          label: "Rich HTML",
          icon: Code2,
          cls: "bg-violet-500/15 text-violet-700 dark:text-violet-300",
        };
      case "image":
        return {
          label: "PNG Image",
          icon: ImageIcon,
          cls: "bg-cyan-500/15 text-cyan-700 dark:text-cyan-300",
        };
      case "files":
        return {
          label: "Files",
          icon: FileIcon,
          cls: "bg-emerald-500/15 text-emerald-700 dark:text-emerald-300",
        };
    }
  };

  return (
    <div className="space-y-4">
      {/* Live File Transfers */}
      <TransferStrip
        transfers={transfers}
        onApprove={onApproveTransfer}
        onReject={onRejectTransfer}
        onCancel={onCancelTransfer}
        onOpenFolder={onOpenFolder}
      />

      {/* Current Active Clipboard Hero Card */}
      <section className="rounded-2xl border border-slate-200 bg-white p-4 shadow-xs dark:border-slate-800 dark:bg-slate-900/80">
        <div className="mb-2.5 flex flex-wrap items-center justify-between gap-2">
          <div className="flex items-center gap-2">
            <span className="text-xs font-bold uppercase tracking-wider text-slate-500 dark:text-slate-400">
              Latest Clipboard
            </span>
            {latest && (
              <span
                className={`rounded-full px-2 py-0.5 text-[10px] font-semibold ${
                  latest.is_local
                    ? "bg-sky-500/15 text-sky-700 dark:text-sky-300"
                    : "bg-emerald-500/15 text-emerald-700 dark:text-emerald-300"
                }`}
              >
                {latest.is_local
                  ? "Local Capture"
                  : `From ${latest.origin_device_name} (${osLabel(latest.origin_os)})`}
              </span>
            )}
          </div>
          <div className="flex items-center gap-1.5">
            <button
              type="button"
              onClick={onSendFilesClick}
              className="inline-flex items-center gap-1.5 rounded-lg border border-slate-300 bg-slate-50 px-2.5 py-1 text-xs font-medium text-slate-700 hover:bg-slate-100 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-200 dark:hover:bg-slate-700"
            >
              <UploadCloud className="h-3.5 w-3.5 text-cyan-500" />
              Send Files / Drop Anywhere
            </button>
            {latest && (
              <button
                type="button"
                onClick={() => onResend(latest.id)}
                className="inline-flex items-center gap-1 rounded-lg bg-cyan-500/15 px-2.5 py-1 text-xs font-semibold text-cyan-700 hover:bg-cyan-500/25 dark:text-cyan-300"
              >
                <RefreshCw className="h-3.5 w-3.5" />
                Re-sync
              </button>
            )}
          </div>
        </div>

        {latest ? (
          <div className="flex flex-col gap-3 sm:flex-row sm:items-start">
            {latest.kind === "image" && latest.thumbnail_data_url && (
              <img
                src={latest.thumbnail_data_url}
                alt="Latest clipboard preview"
                onClick={() => setPreviewEntry(latest)}
                className="h-24 w-36 cursor-pointer rounded-xl border border-slate-200 object-contain bg-slate-950/30 dark:border-slate-700"
              />
            )}
            <div className="min-w-0 flex-1">
              <div className="line-clamp-3 break-words font-mono text-xs leading-relaxed text-slate-800 dark:text-slate-200">
                {latest.text_preview || "(empty)"}
              </div>
              <div className="mt-2 flex flex-wrap items-center gap-3 text-[11px] text-slate-500 dark:text-slate-400">
                <span>{formatBytes(latest.byte_len)}</span>
                <span>· {formatRelativeTime(latest.timestamp_ms)}</span>
                {latest.image_width && latest.image_height && (
                  <span>
                    · {latest.image_width}×{latest.image_height} px
                  </span>
                )}
              </div>
            </div>
          </div>
        ) : (
          <div className="rounded-xl border border-dashed border-slate-300 py-6 text-center text-xs text-slate-500 dark:border-slate-700 dark:text-slate-400">
            Copy text, images, HTML, or files on any paired machine — or drag & drop files here to sync immediately.
          </div>
        )}
      </section>

      {/* Filter & Search Toolbar */}
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex flex-wrap items-center gap-1">
          {(
            [
              ["all", "All"],
              ["text", "Text"],
              ["html", "HTML"],
              ["image", "Images"],
              ["files", "Files"],
              ["pinned", "Pinned"],
            ] as [FilterTab, string][]
          ).map(([id, label]) => (
            <button
              key={id}
              type="button"
              onClick={() => setFilter(id)}
              className={`rounded-lg px-2.5 py-1 text-xs font-medium transition ${
                filter === id
                  ? "bg-cyan-500 text-slate-950 font-semibold"
                  : "bg-slate-200/70 text-slate-600 hover:bg-slate-200 dark:bg-slate-800/80 dark:text-slate-300 dark:hover:bg-slate-800"
              }`}
            >
              {label}
            </button>
          ))}
        </div>

        <div className="flex items-center gap-2">
          <div className="relative">
            <Search className="pointer-events-none absolute left-2.5 top-2 h-3.5 w-3.5 text-slate-400" />
            <input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Search clipboard history..."
              className="w-52 rounded-lg border border-slate-300 bg-white py-1.5 pl-8 pr-3 text-xs text-slate-800 placeholder:text-slate-400 focus:border-cyan-500 focus:outline-none dark:border-slate-700 dark:bg-slate-900 dark:text-slate-100"
            />
          </div>
          {history.some((h) => !h.pinned) && (
            <button
              type="button"
              onClick={onClearUnpinned}
              className="rounded-lg border border-slate-300 px-2.5 py-1.5 text-xs font-medium text-slate-600 hover:bg-slate-100 dark:border-slate-700 dark:text-slate-300 dark:hover:bg-slate-800"
            >
              Clear Unpinned
            </button>
          )}
        </div>
      </div>

      {/* History List */}
      <div className="space-y-2">
        {filtered.map((entry) => {
          const badge = kindBadge(entry.kind);
          const BadgeIcon = badge.icon;
          const firstDir = entry.file_paths[0]?.replace(/[\\/][^\\/]+$/, "");

          return (
            <article
              key={entry.id}
              className="group rounded-xl border border-slate-200 bg-white p-3 transition hover:border-cyan-500/40 dark:border-slate-800 dark:bg-slate-900/70"
            >
              <div className="flex items-start justify-between gap-3">
                <div className="flex min-w-0 flex-1 items-start gap-3">
                  {entry.kind === "image" && entry.thumbnail_data_url ? (
                    <img
                      src={entry.thumbnail_data_url}
                      alt="Clip thumbnail"
                      onClick={() => setPreviewEntry(entry)}
                      className="h-14 w-20 shrink-0 cursor-pointer rounded-lg border border-slate-200 bg-slate-950/30 object-cover dark:border-slate-700"
                    />
                  ) : (
                    <div
                      className={`mt-0.5 flex h-8 w-8 shrink-0 items-center justify-center rounded-lg ${badge.cls}`}
                    >
                      <BadgeIcon className="h-4 w-4" />
                    </div>
                  )}

                  <div className="min-w-0 flex-1">
                    <div className="mb-1 flex flex-wrap items-center gap-1.5 text-[11px]">
                      <span className={`rounded px-1.5 py-0.5 font-semibold ${badge.cls}`}>
                        {badge.label}
                      </span>
                      <span className="font-medium text-slate-700 dark:text-slate-300">
                        {entry.is_local ? "This device" : entry.origin_device_name}
                      </span>
                      <span className="rounded bg-slate-100 px-1.5 py-0.5 text-[10px] text-slate-500 dark:bg-slate-800 dark:text-slate-400">
                        {osLabel(entry.origin_os)}
                      </span>
                      <span className="text-slate-400">
                        · {formatBytes(entry.byte_len)} · {formatRelativeTime(entry.timestamp_ms)}
                      </span>
                      {entry.pinned && (
                        <span className="inline-flex items-center gap-0.5 rounded bg-amber-500/15 px-1.5 py-0.5 text-[10px] font-semibold text-amber-600 dark:text-amber-300">
                          <Pin className="h-2.5 w-2.5" /> Pinned
                        </span>
                      )}
                    </div>

                    <div
                      onClick={() => setPreviewEntry(entry)}
                      className="cursor-pointer line-clamp-2 break-words font-mono text-xs text-slate-700 hover:text-slate-950 dark:text-slate-200 dark:hover:text-white"
                    >
                      {entry.text_preview}
                    </div>

                    {entry.kind === "files" && entry.file_names.length > 0 && (
                      <div className="mt-1.5 flex flex-wrap gap-1">
                        {entry.file_names.slice(0, 4).map((fn, idx) => (
                          <span
                            key={idx}
                            className="rounded border border-slate-200 bg-slate-50 px-1.5 py-0.5 text-[10px] text-slate-600 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-300"
                          >
                            {fn}
                          </span>
                        ))}
                        {entry.file_names.length > 4 && (
                          <span className="text-[10px] text-slate-400">
                            +{entry.file_names.length - 4} more
                          </span>
                        )}
                      </div>
                    )}
                  </div>
                </div>

                {/* Hover / Quick Actions */}
                <div className="flex shrink-0 items-center gap-1">
                  <button
                    type="button"
                    title="Copy to local clipboard"
                    onClick={() => handleCopy(entry.id)}
                    className="rounded-lg p-1.5 text-slate-500 hover:bg-slate-100 hover:text-cyan-600 dark:text-slate-400 dark:hover:bg-slate-800 dark:hover:text-cyan-300"
                  >
                    {copiedId === entry.id ? (
                      <Check className="h-4 w-4 text-emerald-500" />
                    ) : (
                      <Copy className="h-4 w-4" />
                    )}
                  </button>
                  <button
                    type="button"
                    title="Resend to connected peers"
                    onClick={() => onResend(entry.id)}
                    className="rounded-lg p-1.5 text-slate-500 hover:bg-slate-100 hover:text-cyan-600 dark:text-slate-400 dark:hover:bg-slate-800 dark:hover:text-cyan-300"
                  >
                    <Send className="h-4 w-4" />
                  </button>
                  {entry.kind === "files" && firstDir && (
                    <button
                      type="button"
                      title="Open containing folder"
                      onClick={() => onOpenFolder(firstDir)}
                      className="rounded-lg p-1.5 text-slate-500 hover:bg-slate-100 hover:text-emerald-600 dark:text-slate-400 dark:hover:bg-slate-800 dark:hover:text-emerald-300"
                    >
                      <FolderOpen className="h-4 w-4" />
                    </button>
                  )}
                  <button
                    type="button"
                    title={entry.pinned ? "Unpin" : "Pin item"}
                    onClick={() => onTogglePin(entry.id)}
                    className={`rounded-lg p-1.5 hover:bg-slate-100 dark:hover:bg-slate-800 ${
                      entry.pinned
                        ? "text-amber-500"
                        : "text-slate-400 hover:text-amber-500"
                    }`}
                  >
                    <Pin className="h-4 w-4" />
                  </button>
                  <button
                    type="button"
                    title="Delete item"
                    onClick={() => onDelete(entry.id)}
                    className="rounded-lg p-1.5 text-slate-400 hover:bg-slate-100 hover:text-rose-500 dark:hover:bg-slate-800"
                  >
                    <Trash2 className="h-4 w-4" />
                  </button>
                </div>
              </div>
            </article>
          );
        })}

        {filtered.length === 0 && (
          <div className="rounded-xl border border-slate-200 bg-white/60 py-10 text-center text-xs text-slate-500 dark:border-slate-800 dark:bg-slate-900/40 dark:text-slate-400">
            No matching clipboard entries.
          </div>
        )}
      </div>

      {/* Full Preview Modal */}
      {previewEntry && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-950/75 p-4 backdrop-blur-xs">
          <div className="flex max-h-[85vh] w-full max-w-2xl flex-col rounded-2xl border border-slate-200 bg-white p-5 shadow-2xl dark:border-slate-800 dark:bg-slate-900">
            <div className="mb-3 flex items-center justify-between">
              <div>
                <h3 className="text-sm font-bold text-slate-900 dark:text-white">
                  Clipboard Item Detail
                </h3>
                <p className="text-xs text-slate-500">
                  {previewEntry.is_local ? "This device" : previewEntry.origin_device_name} ·{" "}
                  {formatBytes(previewEntry.byte_len)}
                </p>
              </div>
              <button
                type="button"
                onClick={() => setPreviewEntry(null)}
                className="rounded-lg p-1 text-slate-400 hover:bg-slate-100 dark:hover:bg-slate-800"
              >
                <X className="h-4 w-4" />
              </button>
            </div>
            <div className="flex-1 overflow-auto rounded-xl border border-slate-200 bg-slate-50 p-3 dark:border-slate-800 dark:bg-slate-950">
              {previewEntry.kind === "image" && previewEntry.thumbnail_data_url ? (
                <img
                  src={previewEntry.thumbnail_data_url}
                  alt="Full preview"
                  className="mx-auto max-h-96 rounded-lg object-contain"
                />
              ) : (
                <pre className="whitespace-pre-wrap break-words font-mono text-xs text-slate-800 dark:text-slate-200">
                  {previewEntry.full_text ?? previewEntry.text_preview}
                </pre>
              )}
            </div>
            <div className="mt-4 flex justify-end gap-2">
              <button
                type="button"
                onClick={() => {
                  handleCopy(previewEntry.id);
                  setPreviewEntry(null);
                }}
                className="inline-flex items-center gap-1.5 rounded-lg bg-cyan-500 px-3 py-1.5 text-xs font-semibold text-slate-950 hover:bg-cyan-400"
              >
                <Copy className="h-3.5 w-3.5" />
                Copy to Clipboard
              </button>
              <button
                type="button"
                onClick={() => {
                  onResend(previewEntry.id);
                  setPreviewEntry(null);
                }}
                className="inline-flex items-center gap-1.5 rounded-lg border border-slate-300 px-3 py-1.5 text-xs font-medium text-slate-700 hover:bg-slate-100 dark:border-slate-700 dark:text-slate-200 dark:hover:bg-slate-800"
              >
                <Send className="h-3.5 w-3.5" />
                Resend
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
