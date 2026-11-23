import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { UploadCloud } from "lucide-react";
import { api } from "./api";
import type {
  AppConfig,
  HistoryEntry,
  IncomingPairUi,
  LocalInterfaceInfo,
  OutboundPairInfo,
  SyncDirection,
  TelemetrySnapshot,
  ToastNotification,
  TransferInfo,
} from "./types";
import { TopTelemetryBar } from "./components/TopTelemetryBar";
import { NavTab, SidebarNav } from "./components/SidebarNav";
import { ClipboardView } from "./components/ClipboardView";
import { DevicesView } from "./components/DevicesView";
import { ConnectionsView } from "./components/ConnectionsView";
import { SettingsView } from "./components/SettingsView";
import { QrCodeModal } from "./components/QrCodeModal";
import { ToastStack } from "./components/ToastStack";

export function App() {
  const [tab, setTab] = useState<NavTab>("clipboard");
  const [snapshot, setSnapshot] = useState<TelemetrySnapshot | null>(null);
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [transfers, setTransfers] = useState<TransferInfo[]>([]);
  const [incomingPairs, setIncomingPairs] = useState<IncomingPairUi[]>([]);
  const [localIps, setLocalIps] = useState<LocalInterfaceInfo[]>([]);
  const [toasts, setToasts] = useState<ToastNotification[]>([]);
  const [qrOpen, setQrOpen] = useState(false);
  const [pairsMenuOpen, setPairsMenuOpen] = useState(false);
  const [outboundPair, setOutboundPair] = useState<OutboundPairInfo | null>(null);
  const [shareUri, setShareUri] = useState("");
  const [draggingFiles, setDraggingFiles] = useState(false);

  const pushToast = useCallback(
    (
      level: string,
      title: string,
      message: string,
      options?: { actionLabel?: string; onAction?: () => void; durationMs?: number }
    ) => {
      const id = crypto.randomUUID();
      setToasts((prev) => [
        ...prev.slice(-3),
        {
          id,
          level,
          title,
          message,
          actionLabel: options?.actionLabel,
          onAction: options?.onAction,
        },
      ]);
      setTimeout(() => {
        setToasts((prev) => prev.filter((t) => t.id !== id));
      }, options?.durationMs ?? 4500);
    },
    []
  );

  const refreshAll = useCallback(async () => {
    const [snap, cfg, hist, xfers, pairs, ips] = await Promise.all([
      api.getSnapshot(),
      api.getConfig(),
      api.getHistory(),
      api.getTransfers(),
      api.getIncomingPairs(),
      api.getLocalIps(),
    ]);
    setSnapshot(snap);
    setConfig(cfg);
    setHistory(hist);
    setTransfers(xfers);
    setIncomingPairs(pairs);
    setLocalIps(ips);
  }, []);

  useEffect(() => {
    refreshAll().catch((e) => pushToast("error", "Init Failed", String(e)));

    const unlistenFns: (() => void)[] = [];
    let cancelled = false;

    const attach = async () => {
      const u1 = await listen<TelemetrySnapshot>("clipnet://snapshot", (e) => {
        if (!cancelled) setSnapshot(e.payload);
      });
      const u2 = await listen<HistoryEntry[]>("clipnet://history", (e) => {
        if (!cancelled) setHistory(e.payload);
      });
      const u3 = await listen<TransferInfo[]>("clipnet://transfers", (e) => {
        if (!cancelled) setTransfers(e.payload);
      });
      const u4 = await listen<IncomingPairUi[]>("clipnet://pairing", (e) => {
        if (!cancelled) setIncomingPairs(e.payload);
      });
      const u5 = await listen<{
        level: string;
        title: string;
        message: string;
      }>("clipnet://toast", (e) => {
        if (!cancelled) {
          if (e.payload.title === "Pairing Request") {
            pushToast(e.payload.level, e.payload.title, e.payload.message, {
              actionLabel: "Review Request",
              onAction: () => setPairsMenuOpen(true),
              durationMs: 8000,
            });
          } else {
            pushToast(e.payload.level, e.payload.title, e.payload.message);
          }
        }
      });
      const u7 = await listen<OutboundPairInfo>("clipnet://outbound-pair", (e) => {
        if (cancelled) return;
        const info = e.payload;
        if (info.state === "completed") {
          setOutboundPair(null);
          refreshAll().catch(() => {});
          pushToast(
            "success",
            "Device Paired",
            `${info.target_name} added to trusted peers`
          );
        } else if (info.state === "cancelled") {
          setOutboundPair(null);
        } else {
          setOutboundPair(info);
        }
      });
      const u8 = await listen("clipnet://open-pair-requests", () => {
        if (!cancelled) setPairsMenuOpen(true);
      });
      const u6 = await getCurrentWebview().onDragDropEvent(async (event) => {
        if (cancelled) return;
        if (event.payload.type === "over" || event.payload.type === "enter") {
          setDraggingFiles(true);
        } else if (event.payload.type === "leave") {
          setDraggingFiles(false);
        } else if (event.payload.type === "drop") {
          setDraggingFiles(false);
          const paths = event.payload.paths;
          if (paths && paths.length > 0) {
            try {
              await api.sendFiles(paths, null);
              pushToast(
                "info",
                "Sending Files",
                `Queued ${paths.length} item(s) to connected peers`
              );
            } catch (err) {
              pushToast("error", "Send Files Failed", String(err));
            }
          }
        }
      });
      unlistenFns.push(u1, u2, u3, u4, u5, u6, u7, u8);
    };

    attach();
    return () => {
      cancelled = true;
      unlistenFns.forEach((fn) => fn());
    };
  }, [pushToast, refreshAll]);

  // Apply Dark / Light / System theme
  useEffect(() => {
    if (!config) return;
    const apply = () => {
      const prefersDark = window.matchMedia(
        "(prefers-color-scheme: dark)"
      ).matches;
      const isDark =
        config.ui.theme === "dark" ||
        (config.ui.theme === "system" && prefersDark);
      document.documentElement.classList.toggle("dark", isDark);
    };
    apply();
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
  }, [config]);

  const handleChangeDirection = async (dir: SyncDirection) => {
    const nextSnap = await api.setSyncDirection(dir);
    setSnapshot(nextSnap);
  };

  const handleSendFiles = async () => {
    try {
      const paths = await api.pickFiles();
      if (paths.length === 0) return;
      await api.sendFiles(paths, null);
      pushToast(
        "info",
        "Sending Files",
        `Offering ${paths.length} file(s) to connected peers`
      );
    } catch (e) {
      pushToast("error", "Send Files", String(e));
    }
  };

  const handleShowQr = async () => {
    const uri = await api.getQrShareUri();
    setShareUri(uri);
    setQrOpen(true);
  };

  if (!snapshot || !config) {
    return (
      <div className="flex h-full items-center justify-center bg-slate-950 text-sm text-slate-300">
        Initializing ClipboardNet engine...
      </div>
    );
  }

  return (
    <div className="relative flex h-screen flex-col overflow-hidden bg-slate-100 text-slate-900 dark:bg-slate-950 dark:text-slate-100">
      <TopTelemetryBar
        snapshot={snapshot}
        incomingPairs={incomingPairs}
        pairsMenuOpen={pairsMenuOpen}
        onTogglePairsMenu={setPairsMenuOpen}
        onRespondIncomingPair={async (id, action) => {
          await api.respondIncomingPair(id, action);
          const updated = await api.getIncomingPairs();
          setIncomingPairs(updated);
          if (action !== "require_pin" && updated.length === 0) {
            setPairsMenuOpen(false);
          }
        }}
        onGoToDevices={() => setTab("devices")}
        onChangeDirection={handleChangeDirection}
        onOpenSettingsDiagnostics={() => setTab("settings")}
      />

      <div className="flex min-h-0 flex-1">
        <SidebarNav
          activeTab={tab}
          onSelectTab={setTab}
          snapshot={snapshot}
          onSendFilesClick={handleSendFiles}
          onShowQrClick={handleShowQr}
          onOpenPairRequests={() => setPairsMenuOpen(true)}
        />

        <main className="min-w-0 flex-1 overflow-y-auto p-5">
          <div className="mx-auto max-w-5xl">
            {tab === "clipboard" && (
              <ClipboardView
                history={history}
                transfers={transfers}
                onCopy={async (id) => {
                  await api.copyHistoryItem(id);
                  pushToast("success", "Copied", "Item placed onto local clipboard");
                }}
                onResend={async (id) => {
                  await api.resendHistoryItem(id, null);
                  pushToast("info", "Re-synced", "Broadcast to connected peers");
                }}
                onTogglePin={async (id) => setHistory(await api.togglePinHistory(id))}
                onDelete={async (id) => setHistory(await api.deleteHistoryItem(id))}
                onClearUnpinned={async () =>
                  setHistory(await api.clearUnpinnedHistory())
                }
                onSendFilesClick={handleSendFiles}
                onApproveTransfer={(id) => api.approveTransfer(id)}
                onRejectTransfer={(id) => api.rejectTransfer(id)}
                onCancelTransfer={(id) => api.cancelTransfer(id)}
                onOpenFolder={(p) => api.openPathInOs(p)}
              />
            )}

            {tab === "devices" && (
              <DevicesView
                snapshot={snapshot}
                incomingPairs={incomingPairs}
                outboundPair={outboundPair}
                onRespondIncomingPair={async (id, action) => {
                  await api.respondIncomingPair(id, action);
                  setIncomingPairs(await api.getIncomingPairs());
                }}
                onStartPair={async (addr) => {
                  const info = await api.startPairing(addr);
                  setOutboundPair(info);
                  return info;
                }}
                onSubmitPin={async (sid, pin) => {
                  await api.submitPairPin(sid, pin);
                  setOutboundPair(null);
                  await refreshAll();
                }}
                onCancelPair={async (sid) => {
                  await api.cancelPairing(sid);
                  setOutboundPair(null);
                }}
                onDismissOutboundPair={() => setOutboundPair(null)}
                onUpdatePeerSettings={async (id, muted, dir) => {
                  setSnapshot(await api.updatePeerSettings(id, muted, dir));
                  setConfig(await api.getConfig());
                }}
                onRevokePeer={async (id) => {
                  setSnapshot(await api.revokePeer(id));
                  setConfig(await api.getConfig());
                }}
                onPingPeers={() => api.pingPeers()}
              />
            )}

            {tab === "connections" && (
              <ConnectionsView
                config={config}
                snapshot={snapshot}
                localIps={localIps}
                onUpdateLan={async (lan) => {
                  setSnapshot(await api.updateLanConfig(lan));
                  setConfig(await api.getConfig());
                }}
                onUpdateEmbeddedHub={async (hub) => {
                  setSnapshot(await api.updateEmbeddedHub(hub));
                  setConfig(await api.getConfig());
                }}
                onSaveProfile={async (profile) => {
                  setSnapshot(await api.saveProfile(profile));
                  setConfig(await api.getConfig());
                }}
                onDeleteProfile={async (id) => {
                  setSnapshot(await api.deleteProfile(id));
                  setConfig(await api.getConfig());
                }}
                onToggleProfile={async (id, enabled) => {
                  setSnapshot(await api.toggleProfile(id, enabled));
                  setConfig(await api.getConfig());
                }}
                onTestProfile={(p) => api.testProfile(p)}
                onGenerateRoomCode={() => api.generateRoomCode()}
                onShowQr={handleShowQr}
              />
            )}

            {tab === "settings" && (
              <SettingsView
                config={config}
                snapshot={snapshot}
                onSaveConfig={async (cfg) => {
                  setSnapshot(await api.updateConfig(cfg));
                  setConfig(await api.getConfig());
                }}
                onPickDownloadDir={() => api.pickFolder()}
                onRegenerateIdentity={async () => {
                  setSnapshot(await api.regenerateIdentity());
                  setConfig(await api.getConfig());
                  pushToast("info", "Identity Reset", "Generated new E2EE device keypair");
                }}
                onExportDiagnostics={() => api.exportDiagnostics()}
              />
            )}
          </div>
        </main>
      </div>

      {/* Drag & Drop File Send Overlay */}
      {draggingFiles && (
        <div className="pointer-events-none fixed inset-0 z-40 flex items-center justify-center bg-slate-950/75 backdrop-blur-xs">
          <div className="flex flex-col items-center rounded-3xl border-2 border-dashed border-cyan-400 bg-slate-900/90 px-12 py-10 text-center shadow-2xl">
            <UploadCloud className="mb-3 h-12 w-12 text-cyan-400" />
            <div className="text-lg font-bold text-white">
              Drop files or folders to send
            </div>
            <div className="mt-1 text-xs text-slate-400">
              Encrypted end-to-end & synced to connected peers
            </div>
          </div>
        </div>
      )}

      <QrCodeModal
        open={qrOpen}
        shareUri={shareUri}
        onClose={() => setQrOpen(false)}
        onImportUri={async (uri) => {
          try {
            const msg = await api.importInviteUri(uri);
            await refreshAll();
            pushToast("success", "Invite Imported", msg);
            setTab("connections");
          } catch (e) {
            pushToast("error", "Import Failed", String(e));
          }
        }}
      />

      <ToastStack
        toasts={toasts}
        onDismiss={(id) => setToasts((prev) => prev.filter((t) => t.id !== id))}
      />
    </div>
  );
}

export default App;
