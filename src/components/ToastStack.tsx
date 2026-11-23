import { AlertCircle, CheckCircle2, Info, X } from "lucide-react";
import type { ToastNotification } from "../types";

interface Props {
  toasts: ToastNotification[];
  onDismiss: (id: string) => void;
}

export function ToastStack({ toasts, onDismiss }: Props) {
  if (toasts.length === 0) return null;

  return (
    <div className="pointer-events-none fixed bottom-4 right-4 z-50 flex w-80 flex-col gap-2">
      {toasts.map((t) => {
        const Icon =
          t.level === "success"
            ? CheckCircle2
            : t.level === "error"
            ? AlertCircle
            : Info;
        const accent =
          t.level === "success"
            ? "border-emerald-500/40 text-emerald-500"
            : t.level === "error"
            ? "border-rose-500/40 text-rose-500"
            : "border-cyan-500/40 text-cyan-500";

        return (
          <div
            key={t.id}
            className={`pointer-events-auto flex items-start gap-2.5 rounded-xl border bg-white/95 p-3 shadow-lg backdrop-blur dark:bg-slate-900/95 ${accent}`}
          >
            <Icon className="mt-0.5 h-4 w-4 shrink-0" />
            <div className="min-w-0 flex-1">
              <div className="text-xs font-bold text-slate-900 dark:text-white">
                {t.title}
              </div>
              <div className="mt-0.5 line-clamp-2 text-[11px] text-slate-600 dark:text-slate-300">
                {t.message}
              </div>
            </div>
            <button
              type="button"
              onClick={() => onDismiss(t.id)}
              className="rounded p-0.5 text-slate-400 hover:text-slate-600 dark:hover:text-slate-200"
            >
              <X className="h-3.5 w-3.5" />
            </button>
          </div>
        );
      })}
    </div>
  );
}
