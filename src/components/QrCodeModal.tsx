import { useEffect, useState } from "react";
import QRCode from "qrcode";
import { Check, Copy, Link2, X } from "lucide-react";

interface Props {
  open: boolean;
  shareUri: string;
  onClose: () => void;
  onImportUri: (uri: string) => Promise<void>;
}

export function QrCodeModal({ open, shareUri, onClose, onImportUri }: Props) {
  const [svgMarkup, setSvgMarkup] = useState("");
  const [copied, setCopied] = useState(false);
  const [importText, setImportText] = useState("");
  const [importing, setImporting] = useState(false);

  useEffect(() => {
    if (!open || !shareUri) return;
    QRCode.toString(shareUri, {
      type: "svg",
      margin: 2,
      color: { dark: "#0f172a", light: "#ffffff" },
    })
      .then(setSvgMarkup)
      .catch(() => setSvgMarkup(""));
  }, [open, shareUri]);

  if (!open) return null;

  const handleCopy = async () => {
    await navigator.clipboard.writeText(shareUri);
    setCopied(true);
    setTimeout(() => setCopied(false), 1800);
  };

  const handleImport = async () => {
    if (!importText.trim()) return;
    setImporting(true);
    try {
      await onImportUri(importText.trim());
      setImportText("");
      onClose();
    } finally {
      setImporting(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-950/70 p-4 backdrop-blur-xs">
      <div className="w-full max-w-md rounded-2xl border border-slate-200 bg-white p-5 shadow-2xl dark:border-slate-800 dark:bg-slate-900">
        <div className="mb-4 flex items-center justify-between">
          <h3 className="text-base font-bold text-slate-900 dark:text-white">
            Quick Share & Import Invite URI
          </h3>
          <button
            type="button"
            onClick={onClose}
            className="rounded-lg p-1 text-slate-400 hover:bg-slate-100 hover:text-slate-700 dark:hover:bg-slate-800 dark:hover:text-slate-200"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        <div className="flex flex-col items-center rounded-xl border border-slate-200 bg-slate-50 p-4 dark:border-slate-800 dark:bg-slate-950/60">
          {svgMarkup && (
            <div
              className="h-44 w-44 overflow-hidden rounded-xl bg-white p-2 shadow-sm"
              dangerouslySetInnerHTML={{ __html: svgMarkup }}
            />
          )}
          <p className="mt-3 text-center text-xs text-slate-500 dark:text-slate-400">
            Share this invite URI with your other Ubuntu or Windows machine for instant setup.
          </p>
          <div className="mt-2.5 flex w-full items-center gap-2">
            <input
              readOnly
              value={shareUri}
              className="flex-1 rounded-lg border border-slate-300 bg-white px-2.5 py-1.5 font-mono text-[11px] text-slate-700 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-300"
            />
            <button
              type="button"
              onClick={handleCopy}
              className="inline-flex items-center gap-1 rounded-lg bg-cyan-500 px-3 py-1.5 text-xs font-semibold text-slate-950 hover:bg-cyan-400"
            >
              {copied ? <Check className="h-3.5 w-3.5" /> : <Copy className="h-3.5 w-3.5" />}
              {copied ? "Copied" : "Copy"}
            </button>
          </div>
        </div>

        <div className="mt-4 border-t border-slate-200 pt-4 dark:border-slate-800">
          <label className="mb-1.5 flex items-center gap-1.5 text-xs font-semibold text-slate-700 dark:text-slate-200">
            <Link2 className="h-3.5 w-3.5 text-cyan-500" />
            Import Hub or Relay Invite URI from another device
          </label>
          <div className="flex gap-2">
            <input
              value={importText}
              onChange={(e) => setImportText(e.target.value)}
              placeholder="clipnet://hub?... or clipnet://relay?..."
              className="flex-1 rounded-lg border border-slate-300 bg-white px-3 py-1.5 font-mono text-xs text-slate-800 placeholder:text-slate-400 dark:border-slate-700 dark:bg-slate-950 dark:text-slate-100"
            />
            <button
              type="button"
              disabled={importing || !importText.trim()}
              onClick={handleImport}
              className="rounded-lg bg-slate-900 px-3 py-1.5 text-xs font-semibold text-white hover:bg-slate-800 disabled:opacity-50 dark:bg-slate-100 dark:text-slate-950 dark:hover:bg-white"
            >
              {importing ? "Importing..." : "Import"}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
