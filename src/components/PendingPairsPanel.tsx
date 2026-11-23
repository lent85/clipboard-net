import { useEffect, useState } from "react";
import { Check, Clock, Copy, KeyRound, ShieldCheck, X } from "lucide-react";
import type { IncomingPairUi, PairResponseAction } from "../types";
import { osLabel } from "../api";

function formatCountdown(expiresAtMs: number, nowMs: number): string {
  const secs = Math.max(0, Math.ceil((expiresAtMs - nowMs) / 1000));
  const mins = Math.floor(secs / 60);
  const rem = secs % 60;
  return `${mins}:${rem.toString().padStart(2, "0")}`;
}

interface CardProps {
  request: IncomingPairUi;
  compact?: boolean;
  onRespond: (requestId: string, action: PairResponseAction) => Promise<void>;
}

export function IncomingPairCard({ request, compact = false, onRespond }: CardProps) {
  const [nowMs, setNowMs] = useState(() => Date.now());
  const [busyAction, setBusyAction] = useState<PairResponseAction | null>(null);
  const [copiedPin, setCopiedPin] = useState(false);

  useEffect(() => {
    const id = setInterval(() => setNowMs(Date.now()), 1000);
    return () => clearInterval(id);
  }, []);

  const handleAction = async (action: PairResponseAction) => {
    setBusyAction(action);
    try {
      await onRespond(request.request_id, action);
    } finally {
      setBusyAction(null);
    }
  };

  const handleCopyPin = async () => {
    await navigator.clipboard.writeText(request.pin);
    setCopiedPin(true);
    setTimeout(() => setCopiedPin(false), 1600);
  };

  const countdown = formatCountdown(request.expires_at_ms, nowMs);
  const spacedPin = `${request.pin.slice(0, 3)} ${request.pin.slice(3)}`;

  return (
    <div
      className={`rounded-2xl border border-cyan-500/45 bg-cyan-500/10 ${
        compact ? "p-3.5" : "p-4"
      }`}
    >
      <div className="flex items-start justify-between gap-3">
        <div className="flex min-w-0 items-start gap-2.5">
          <div className="mt-0.5 flex h-9 w-9 shrink-0 items-center justify-center rounded-xl bg-cyan-500 text-slate-950">
            <KeyRound className="h-4 w-4" />
          </div>
          <div className="min-w-0">
            <div className="truncate text-sm font-bold text-slate-900 dark:text-white">
              {request.device_name}{" "}
              <span className="text-xs font-medium text-slate-500 dark:text-slate-300">
                ({osLabel(request.os)})
              </span>
            </div>
            <div className="mt-0.5 text-[11px] text-slate-600 dark:text-slate-300">
              IP: <span className="font-mono">{request.remote_addr}</span> · FP:{" "}
              <span className="font-mono">{request.fingerprint}</span>
            </div>
          </div>
        </div>

        <span className="inline-flex shrink-0 items-center gap-1 rounded-full border border-cyan-500/30 bg-white/80 px-2 py-0.5 font-mono text-[11px] font-semibold text-cyan-700 dark:bg-slate-950/80 dark:text-cyan-300">
          <Clock className="h-3 w-3" />
          {countdown}
        </span>
      </div>

      {request.status === "pin_issued" ? (
        <div className="mt-3 space-y-2.5">
          <div className="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-cyan-500/40 bg-white px-3.5 py-2.5 dark:bg-slate-950">
            <div>
              <div className="text-[10px] font-semibold uppercase tracking-wider text-slate-400">
                Share This 6-Digit PIN With {request.device_name}
              </div>
              <div className="mt-0.5 font-mono text-2xl font-black tracking-[0.22em] text-cyan-600 dark:text-cyan-300">
                {spacedPin}
              </div>
            </div>
            <button
              type="button"
              onClick={handleCopyPin}
              className="inline-flex items-center gap-1.5 rounded-lg border border-slate-300 bg-slate-50 px-2.5 py-1.5 text-xs font-semibold text-slate-700 hover:bg-slate-100 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-200 dark:hover:bg-slate-800"
            >
              {copiedPin ? (
                <>
                  <Check className="h-3.5 w-3.5 text-emerald-500" />
                  <span>Copied</span>
                </>
              ) : (
                <>
                  <Copy className="h-3.5 w-3.5" />
                  <span>Copy PIN</span>
                </>
              )}
            </button>
          </div>

          <div className="flex flex-wrap items-center justify-between gap-2">
            <span className="text-[11px] text-slate-600 dark:text-slate-300">
              Waiting for remote device to enter PIN...
            </span>
            <div className="flex items-center gap-2">
              <button
                type="button"
                disabled={busyAction !== null}
                onClick={() => handleAction("accept_direct")}
                className="inline-flex items-center gap-1 rounded-xl bg-emerald-500 px-3 py-1.5 text-xs font-bold text-slate-950 hover:bg-emerald-400 disabled:opacity-50"
              >
                <ShieldCheck className="h-3.5 w-3.5" />
                Accept Without PIN
              </button>
              <button
                type="button"
                disabled={busyAction !== null}
                onClick={() => handleAction("reject")}
                className="inline-flex items-center gap-1 rounded-xl bg-slate-200 px-3 py-1.5 text-xs font-semibold text-slate-700 hover:bg-slate-300 disabled:opacity-50 dark:bg-slate-800 dark:text-slate-200 dark:hover:bg-slate-700"
              >
                <X className="h-3.5 w-3.5" />
                Reject
              </button>
            </div>
          </div>
        </div>
      ) : (
        <div className="mt-3 flex flex-wrap items-center justify-between gap-2">
          <span className="text-[11px] text-slate-600 dark:text-slate-300">
            Accept immediately or issue a 6-digit PIN to verify.
          </span>
          <div className="flex flex-wrap items-center gap-2">
            <button
              type="button"
              disabled={busyAction !== null}
              onClick={() => handleAction("accept_direct")}
              className="inline-flex items-center gap-1 rounded-xl bg-emerald-500 px-3 py-1.5 text-xs font-bold text-slate-950 hover:bg-emerald-400 disabled:opacity-50"
            >
              <Check className="h-3.5 w-3.5" />
              Accept Now
            </button>
            <button
              type="button"
              disabled={busyAction !== null}
              onClick={() => handleAction("require_pin")}
              className="inline-flex items-center gap-1 rounded-xl bg-cyan-500 px-3 py-1.5 text-xs font-bold text-slate-950 hover:bg-cyan-400 disabled:opacity-50"
            >
              <KeyRound className="h-3.5 w-3.5" />
              Require PIN
            </button>
            <button
              type="button"
              disabled={busyAction !== null}
              onClick={() => handleAction("reject")}
              className="inline-flex items-center gap-1 rounded-xl bg-slate-200 px-3 py-1.5 text-xs font-semibold text-slate-700 hover:bg-slate-300 disabled:opacity-50 dark:bg-slate-800 dark:text-slate-200 dark:hover:bg-slate-700"
            >
              <X className="h-3.5 w-3.5" />
              Reject
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

interface DropdownProps {
  pairs: IncomingPairUi[];
  onRespond: (requestId: string, action: PairResponseAction) => Promise<void>;
  onGoToDevices: () => void;
  onClose: () => void;
}

export function PendingPairsDropdown({
  pairs,
  onRespond,
  onGoToDevices,
  onClose,
}: DropdownProps) {
  return (
    <div className="absolute right-0 z-40 mt-2 w-[420px] max-w-[92vw] rounded-2xl border border-slate-200 bg-white p-4 shadow-2xl dark:border-slate-700 dark:bg-slate-900">
      <div className="mb-3 flex items-center justify-between">
        <div className="flex items-center gap-2">
          <KeyRound className="h-4 w-4 text-cyan-500" />
          <span className="text-xs font-bold text-slate-900 dark:text-white">
            Pending Pair Requests ({pairs.length})
          </span>
        </div>
        <button
          type="button"
          onClick={onClose}
          className="rounded-lg p-1 text-slate-400 hover:bg-slate-100 hover:text-slate-700 dark:hover:bg-slate-800 dark:hover:text-slate-200"
          title="Close menu"
        >
          <X className="h-4 w-4" />
        </button>
      </div>

      {pairs.length === 0 ? (
        <div className="rounded-xl border border-dashed border-slate-200 py-6 text-center text-xs text-slate-500 dark:border-slate-800 dark:text-slate-400">
          No pending pairing requests right now.
        </div>
      ) : (
        <div className="max-h-[65vh] space-y-2.5 overflow-y-auto pr-0.5">
          {pairs.map((req) => (
            <IncomingPairCard
              key={req.request_id}
              request={req}
              compact
              onRespond={onRespond}
            />
          ))}
        </div>
      )}

      <div className="mt-3 flex items-center justify-between border-t border-slate-100 pt-2.5 dark:border-slate-800">
        <button
          type="button"
          onClick={() => {
            onGoToDevices();
            onClose();
          }}
          className="text-xs font-semibold text-cyan-600 hover:underline dark:text-cyan-400"
        >
          Open Devices Tab
        </button>
        <button
          type="button"
          onClick={onClose}
          className="text-xs text-slate-500 hover:text-slate-700 dark:hover:text-slate-300"
        >
          Close
        </button>
      </div>
    </div>
  );
}
