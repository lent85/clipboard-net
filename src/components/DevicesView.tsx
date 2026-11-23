import { useEffect, useState } from "react";
import {
  Activity,
  Clock,
  Laptop,
  Loader2,
  Monitor,
  Radar,
  ShieldCheck,
  Trash2,
  Volume2,
  VolumeX,
} from "lucide-react";
import type {
  IncomingPairUi,
  OutboundPairInfo,
  PairResponseAction,
  SyncDirection,
  TelemetrySnapshot,
} from "../types";
import { formatBytes, osLabel } from "../api";
import { IncomingPairCard } from "./PendingPairsPanel";

interface Props {
  snapshot: TelemetrySnapshot;
  incomingPairs: IncomingPairUi[];
  outboundPair: OutboundPairInfo | null;
  onRespondIncomingPair: (requestId: string, action: PairResponseAction) => Promise<void>;
  onStartPair: (addr: string) => Promise<OutboundPairInfo>;
  onSubmitPin: (sessionId: string, pin: string) => Promise<void>;
  onCancelPair: (sessionId: string) => Promise<void>;
  onDismissOutboundPair: () => void;
  onUpdatePeerSettings: (
    deviceId: string,
    muted: boolean,
    direction: SyncDirection | null
  ) => Promise<void>;
  onRevokePeer: (deviceId: string) => Promise<void>;
  onPingPeers: () => Promise<void>;
}

function formatCountdown(expiresAtMs: number, nowMs: number): string {
  const secs = Math.max(0, Math.ceil((expiresAtMs - nowMs) / 1000));
  const mins = Math.floor(secs / 60);
  const rem = secs % 60;
  return `${mins}:${rem.toString().padStart(2, "0")}`;
}

export function DevicesView({
  snapshot,
  incomingPairs,
  outboundPair,
  onRespondIncomingPair,
  onStartPair,
  onSubmitPin,
  onCancelPair,
  onDismissOutboundPair,
  onUpdatePeerSettings,
  onRevokePeer,
  onPingPeers,
}: Props) {
  const [manualAddr, setManualAddr] = useState("");
  const [pinInput, setPinInput] = useState("");
  const [dialError, setDialError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [nowMs, setNowMs] = useState(() => Date.now());

  useEffect(() => {
    setPinInput("");
  }, [outboundPair?.session_id]);

  useEffect(() => {
    if (!outboundPair) return;
    setNowMs(Date.now());
    const id = setInterval(() => setNowMs(Date.now()), 1000);
    return () => clearInterval(id);
  }, [outboundPair]);

  const handleDialPair = async (addr: string) => {
    setDialError(null);
    setBusy(true);
    try {
      await onStartPair(addr);
      setPinInput("");
    } catch (e) {
      setDialError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const handleConfirmPin = async () => {
    if (!outboundPair || pinInput.trim().length < 6) return;
    setDialError(null);
    setBusy(true);
    try {
      await onSubmitPin(outboundPair.session_id, pinInput.trim());
      setPinInput("");
    } catch (e) {
      setDialError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const handleCancelOutbound = async () => {
    if (!outboundPair) return;
    setDialError(null);
    setBusy(true);
    try {
      await onCancelPair(outboundPair.session_id);
      setPinInput("");
    } finally {
      setBusy(false);
    }
  };

  const OsIcon = ({ os }: { os: string }) =>
    os.toLowerCase().includes("win") ? (
      <Monitor className="h-4 w-4 text-sky-500" />
    ) : (
      <Laptop className="h-4 w-4 text-orange-400" />
    );

  // Merge active peers with trusted peers
  const activeById = new Map(snapshot.active_peers.map((p) => [p.device_id, p]));
  const trustedIds = new Set(snapshot.trusted_peers.map((p) => p.device_id));
  const transientPeers = snapshot.active_peers.filter(
    (p) => !trustedIds.has(p.device_id)
  );

  const modalError = outboundPair?.message ?? dialError;

  return (
    <div className="space-y-5">
      {/* Incoming Pairing Approval Banners */}
      {incomingPairs.length > 0 && (
        <section className="space-y-2.5">
          <div className="flex items-center justify-between">
            <h2 className="text-sm font-bold text-slate-900 dark:text-white">
              Pending Pair Requests ({incomingPairs.length})
            </h2>
            <span className="text-[11px] text-slate-500 dark:text-slate-400">
              Accept directly or issue a 6-digit PIN
            </span>
          </div>
          {incomingPairs.map((req) => (
            <IncomingPairCard
              key={req.request_id}
              request={req}
              onRespond={onRespondIncomingPair}
            />
          ))}
        </section>
      )}

      {/* Active & Paired Devices */}
      <section className="space-y-3">
        <div className="flex items-center justify-between">
          <div>
            <h2 className="text-sm font-bold text-slate-900 dark:text-white">
              Connected & Paired Devices
            </h2>
            <p className="text-xs text-slate-500 dark:text-slate-400">
              Live transport route, latency RTT, traffic counters, and per-device sync controls.
            </p>
          </div>
          <button
            type="button"
            onClick={() => onPingPeers()}
            className="inline-flex items-center gap-1.5 rounded-lg border border-slate-300 bg-white px-2.5 py-1.5 text-xs font-medium text-slate-700 hover:bg-slate-100 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-200 dark:hover:bg-slate-700"
          >
            <Activity className="h-3.5 w-3.5 text-cyan-500" />
            Ping All
          </button>
        </div>

        <div className="grid gap-3 md:grid-cols-2">
          {/* Active peers discovered via Hub/Relay/Direct even if not in LAN pair list */}
          {transientPeers.map((ap) => (
            <div
              key={ap.device_id}
              className="rounded-2xl border border-emerald-500/30 bg-white p-4 shadow-xs dark:bg-slate-900/80"
            >
              <div className="flex items-start justify-between gap-2">
                <div className="flex items-center gap-2.5">
                  <div className="flex h-9 w-9 items-center justify-center rounded-xl bg-slate-100 dark:bg-slate-800">
                    <OsIcon os={ap.os} />
                  </div>
                  <div>
                    <div className="flex items-center gap-2">
                      <span className="h-2 w-2 rounded-full bg-emerald-500" />
                      <span className="text-sm font-bold text-slate-900 dark:text-white">
                        {ap.device_name}
                      </span>
                      <span className="rounded bg-slate-100 px-1.5 py-0.5 text-[10px] font-medium text-slate-600 dark:bg-slate-800 dark:text-slate-300">
                        {osLabel(ap.os)}
                      </span>
                    </div>
                    <div className="mt-0.5 flex flex-wrap items-center gap-2 text-[11px] text-slate-500 dark:text-slate-400">
                      <span className="rounded bg-cyan-500/15 px-1.5 py-0.5 font-semibold text-cyan-700 dark:text-cyan-300">
                        {ap.transport_kind}
                      </span>
                      <span>{ap.channel_label}</span>
                      {ap.rtt_ms !== null && (
                        <span className="font-mono">{ap.rtt_ms} ms</span>
                      )}
                    </div>
                  </div>
                </div>
              </div>
              <div className="mt-3 flex items-center justify-between border-t border-slate-100 pt-2.5 text-[11px] text-slate-500 dark:border-slate-800 dark:text-slate-400">
                <span className="font-mono">FP: {ap.fingerprint || "Hub Session"}</span>
                <span>
                  ↑ {formatBytes(ap.tx_bytes)} · ↓ {formatBytes(ap.rx_bytes)}
                </span>
              </div>
            </div>
          ))}

          {/* Paired Trusted Devices */}
          {snapshot.trusted_peers.map((tp) => {
            const active = activeById.get(tp.device_id);
            return (
              <div
                key={tp.device_id}
                className={`rounded-2xl border bg-white p-4 shadow-xs dark:bg-slate-900/80 ${
                  active
                    ? "border-emerald-500/35"
                    : "border-slate-200 dark:border-slate-800"
                }`}
              >
                <div className="flex items-start justify-between gap-2">
                  <div className="flex items-center gap-2.5">
                    <div className="flex h-9 w-9 items-center justify-center rounded-xl bg-slate-100 dark:bg-slate-800">
                      <OsIcon os={tp.os} />
                    </div>
                    <div>
                      <div className="flex items-center gap-2">
                        <span
                          className={`h-2 w-2 rounded-full ${
                            active ? "bg-emerald-500" : "bg-slate-400"
                          }`}
                        />
                        <span className="text-sm font-bold text-slate-900 dark:text-white">
                          {tp.device_name}
                        </span>
                        <span className="rounded bg-slate-100 px-1.5 py-0.5 text-[10px] font-medium text-slate-600 dark:bg-slate-800 dark:text-slate-300">
                          {osLabel(tp.os)}
                        </span>
                      </div>
                      <div className="mt-0.5 flex flex-wrap items-center gap-2 text-[11px] text-slate-500 dark:text-slate-400">
                        {active ? (
                          <>
                            <span className="rounded bg-emerald-500/15 px-1.5 py-0.5 font-semibold text-emerald-700 dark:text-emerald-300">
                              {active.transport_kind}
                            </span>
                            {active.rtt_ms !== null && (
                              <span className="font-mono">{active.rtt_ms} ms</span>
                            )}
                          </>
                        ) : (
                          <span>Offline · Last IP {tp.last_addr ?? "unknown"}</span>
                        )}
                      </div>
                    </div>
                  </div>

                  <div className="flex items-center gap-1">
                    <button
                      type="button"
                      title={tp.muted ? "Unmute peer" : "Mute peer"}
                      onClick={() =>
                        onUpdatePeerSettings(
                          tp.device_id,
                          !tp.muted,
                          tp.direction_override
                        )
                      }
                      className={`rounded-lg p-1.5 ${
                        tp.muted
                          ? "bg-amber-500/15 text-amber-500"
                          : "text-slate-400 hover:bg-slate-100 dark:hover:bg-slate-800"
                      }`}
                    >
                      {tp.muted ? (
                        <VolumeX className="h-4 w-4" />
                      ) : (
                        <Volume2 className="h-4 w-4" />
                      )}
                    </button>
                    <button
                      type="button"
                      title="Unpair device"
                      onClick={() => onRevokePeer(tp.device_id)}
                      className="rounded-lg p-1.5 text-slate-400 hover:bg-slate-100 hover:text-rose-500 dark:hover:bg-slate-800"
                    >
                      <Trash2 className="h-4 w-4" />
                    </button>
                  </div>
                </div>

                <div className="mt-3 flex flex-wrap items-center justify-between gap-2 border-t border-slate-100 pt-2.5 text-[11px] dark:border-slate-800">
                  <span className="inline-flex items-center gap-1 font-mono text-slate-400">
                    <ShieldCheck className="h-3.5 w-3.5 text-emerald-500" />
                    {tp.fingerprint}
                  </span>

                  <select
                    value={tp.direction_override ?? "inherit"}
                    onChange={(e) => {
                      const val = e.target.value;
                      onUpdatePeerSettings(
                        tp.device_id,
                        tp.muted,
                        val === "inherit" ? null : (val as SyncDirection)
                      );
                    }}
                    className="rounded-md border border-slate-200 bg-slate-50 px-2 py-0.5 text-[11px] text-slate-700 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-200"
                  >
                    <option value="inherit">Global Mode</option>
                    <option value="bidirectional">Bidirectional</option>
                    <option value="send_only">Send Only</option>
                    <option value="receive_only">Receive Only</option>
                    <option value="paused">Paused</option>
                  </select>
                </div>
              </div>
            );
          })}
        </div>

        {transientPeers.length === 0 && snapshot.trusted_peers.length === 0 && (
          <div className="rounded-2xl border border-dashed border-slate-300 py-8 text-center text-xs text-slate-500 dark:border-slate-700 dark:text-slate-400">
            No devices paired or connected yet. Pair a nearby LAN device below or add a Hub / Relay profile in Connections.
          </div>
        )}
      </section>

      {/* Nearby LAN Radar */}
      <section className="rounded-2xl border border-slate-200 bg-white p-4 shadow-xs dark:border-slate-800 dark:bg-slate-900/80">
        <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
          <div className="flex items-center gap-2">
            <Radar className="h-4 w-4 text-cyan-500" />
            <h3 className="text-sm font-bold text-slate-900 dark:text-white">
              Nearby LAN Radar (mDNS + UDP Broadcast)
            </h3>
          </div>
          <div className="flex items-center gap-2">
            <input
              value={manualAddr}
              onChange={(e) => setManualAddr(e.target.value)}
              placeholder="Manual IP:Port (e.g. 192.168.1.50:48215)"
              className="w-60 rounded-lg border border-slate-300 bg-white px-2.5 py-1 text-xs text-slate-800 dark:border-slate-700 dark:bg-slate-950 dark:text-slate-100"
            />
            <button
              type="button"
              disabled={busy || !manualAddr.trim()}
              onClick={() => handleDialPair(manualAddr)}
              className="rounded-lg bg-cyan-500 px-3 py-1 text-xs font-semibold text-slate-950 hover:bg-cyan-400 disabled:opacity-50"
            >
              Pair IP
            </button>
          </div>
        </div>

        {dialError && !outboundPair && (
          <div className="mb-3 rounded-lg border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-600 dark:text-rose-300">
            {dialError}
          </div>
        )}

        <div className="grid gap-2.5 sm:grid-cols-2">
          {snapshot.discovered_peers.map((dp) => {
            const alreadyTrusted = trustedIds.has(dp.device_id);
            const primaryAddr = `${dp.addrs[0] ?? ""}:${dp.port}`;
            return (
              <div
                key={dp.device_id}
                className="flex items-center justify-between gap-3 rounded-xl border border-slate-200 bg-slate-50/70 p-3 dark:border-slate-800 dark:bg-slate-950/60"
              >
                <div className="flex min-w-0 items-center gap-2.5">
                  <OsIcon os={dp.os} />
                  <div className="min-w-0">
                    <div className="truncate text-xs font-bold text-slate-900 dark:text-slate-100">
                      {dp.device_name}
                    </div>
                    <div className="truncate text-[11px] text-slate-500 dark:text-slate-400">
                      {primaryAddr} · via {dp.via.join("+").toUpperCase()}
                    </div>
                  </div>
                </div>
                {alreadyTrusted ? (
                  <span className="rounded-full bg-emerald-500/15 px-2.5 py-1 text-[11px] font-semibold text-emerald-600 dark:text-emerald-300">
                    Paired
                  </span>
                ) : (
                  <button
                    type="button"
                    disabled={busy}
                    onClick={() => handleDialPair(primaryAddr)}
                    className="shrink-0 rounded-lg bg-slate-900 px-3 py-1.5 text-xs font-semibold text-white hover:bg-slate-800 dark:bg-cyan-500 dark:text-slate-950 dark:hover:bg-cyan-400"
                  >
                    Pair
                  </button>
                )}
              </div>
            );
          })}
          {snapshot.discovered_peers.length === 0 && (
            <div className="col-span-full py-4 text-center text-xs text-slate-500 dark:text-slate-400">
              {snapshot.discovery_status.enabled
                ? "Scanning local network via mDNS & UDP broadcast..."
                : "LAN Auto-Discovery is disabled in Connections."}
            </div>
          )}
        </div>
      </section>

      {/* Live Outbound Pairing Modal */}
      {outboundPair && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-950/75 p-4 backdrop-blur-xs">
          <div className="w-full max-w-sm rounded-2xl border border-slate-200 bg-white p-5 shadow-2xl dark:border-slate-800 dark:bg-slate-900">
            <div className="flex items-start justify-between gap-2">
              <h3 className="text-base font-bold text-slate-900 dark:text-white">
                {outboundPair.state === "waiting_approval"
                  ? "Waiting for Remote Approval"
                  : outboundPair.state === "failed"
                  ? "Pairing Failed"
                  : "Enter 6-Digit Pairing PIN"}
              </h3>
              <span className="inline-flex items-center gap-1 rounded-full bg-slate-100 px-2 py-0.5 font-mono text-[11px] text-slate-600 dark:bg-slate-800 dark:text-slate-300">
                <Clock className="h-3 w-3" />
                {formatCountdown(outboundPair.expires_at_ms, nowMs)}
              </span>
            </div>

            <div className="mt-2 rounded-lg bg-slate-100 px-2.5 py-1.5 font-mono text-[11px] text-slate-600 dark:bg-slate-800 dark:text-slate-300">
              {outboundPair.target_name} ({osLabel(outboundPair.target_os)}) · FP:{" "}
              {outboundPair.target_fingerprint}
            </div>

            {outboundPair.state === "waiting_approval" && (
              <>
                <div className="mt-4 flex flex-col items-center rounded-xl border border-cyan-500/30 bg-cyan-500/10 px-4 py-5 text-center">
                  <Loader2 className="mb-2.5 h-7 w-7 animate-spin text-cyan-500" />
                  <p className="text-xs font-semibold text-slate-800 dark:text-slate-100">
                    Request sent to {outboundPair.target_name}
                  </p>
                  <p className="mt-1 text-[11px] leading-relaxed text-slate-600 dark:text-slate-300">
                    Open <strong>Pair Requests</strong> on {outboundPair.target_name} and choose{" "}
                    <strong>Accept Now</strong> (instant) or <strong>Require PIN</strong>.
                  </p>
                </div>
                <div className="mt-4 flex justify-end">
                  <button
                    type="button"
                    disabled={busy}
                    onClick={handleCancelOutbound}
                    className="rounded-lg px-3 py-1.5 text-xs font-medium text-slate-500 hover:bg-slate-100 dark:hover:bg-slate-800"
                  >
                    Cancel Request
                  </button>
                </div>
              </>
            )}

            {(outboundPair.state === "pin_required" ||
              outboundPair.state === "verifying") && (
              <>
                <p className="mt-2 text-xs text-slate-500 dark:text-slate-400">
                  {outboundPair.target_name} requested PIN verification. Enter the 6-digit PIN shown
                  in its <strong>Pair Requests</strong> menu:
                </p>
                <input
                  autoFocus
                  maxLength={6}
                  value={pinInput}
                  onChange={(e) => setPinInput(e.target.value.replace(/\D/g, ""))}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") handleConfirmPin();
                  }}
                  placeholder="000000"
                  className="mt-3 w-full rounded-xl border border-slate-300 bg-white py-2.5 text-center font-mono text-2xl font-black tracking-[0.35em] text-slate-900 focus:border-cyan-500 focus:outline-none dark:border-slate-700 dark:bg-slate-950 dark:text-cyan-300"
                />
                {modalError && (
                  <p className="mt-2 text-xs text-rose-500">{modalError}</p>
                )}
                <div className="mt-4 flex justify-end gap-2">
                  <button
                    type="button"
                    disabled={busy}
                    onClick={handleCancelOutbound}
                    className="rounded-lg px-3 py-1.5 text-xs font-medium text-slate-500 hover:bg-slate-100 dark:hover:bg-slate-800"
                  >
                    Cancel
                  </button>
                  <button
                    type="button"
                    disabled={
                      busy ||
                      outboundPair.state === "verifying" ||
                      pinInput.trim().length < 6
                    }
                    onClick={handleConfirmPin}
                    className="rounded-lg bg-cyan-500 px-4 py-1.5 text-xs font-bold text-slate-950 hover:bg-cyan-400 disabled:opacity-50"
                  >
                    {outboundPair.state === "verifying" || busy
                      ? "Verifying..."
                      : "Verify & Pair"}
                  </button>
                </div>
              </>
            )}

            {outboundPair.state === "failed" && (
              <>
                <p className="mt-3 rounded-lg border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-600 dark:text-rose-300">
                  {modalError ?? "Pairing failed or expired."}
                </p>
                <div className="mt-4 flex justify-end">
                  <button
                    type="button"
                    onClick={() => {
                      setDialError(null);
                      onDismissOutboundPair();
                    }}
                    className="rounded-lg bg-slate-900 px-4 py-1.5 text-xs font-semibold text-white hover:bg-slate-800 dark:bg-slate-800 dark:hover:bg-slate-700"
                  >
                    Close
                  </button>
                </div>
              </>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
