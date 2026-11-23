import { useEffect, useState } from "react";
import {
  AlertTriangle,
  Check,
  Copy,
  Globe,
  HardDrive,
  Key,
  Network,
  Pencil,
  Play,
  Plus,
  QrCode,
  RefreshCw,
  Server,
  ShieldCheck,
  Terminal,
  Trash2,
  Wifi,
  X,
} from "lucide-react";
import type {
  AppConfig,
  ConnectionProfile,
  ConnectionProfileKind,
  ConnectionTestResult,
  EmbeddedHubConfig,
  LanDiscoveryConfig,
  LocalInterfaceInfo,
  TelemetrySnapshot,
} from "../types";

interface Props {
  config: AppConfig;
  snapshot: TelemetrySnapshot;
  localIps: LocalInterfaceInfo[];
  onUpdateLan: (lan: LanDiscoveryConfig) => Promise<void>;
  onUpdateEmbeddedHub: (hub: EmbeddedHubConfig) => Promise<void>;
  onSaveProfile: (profile: ConnectionProfile) => Promise<void>;
  onDeleteProfile: (id: string) => Promise<void>;
  onToggleProfile: (id: string, enabled: boolean) => Promise<void>;
  onTestProfile: (profile: ConnectionProfile) => Promise<ConnectionTestResult>;
  onGenerateRoomCode: () => Promise<string>;
  onShowQr: () => void;
}

type ModalMode = "direct_peer" | "external_hub" | "wan_relay" | "ssh_tunnel";

export function ConnectionsView({
  config,
  snapshot,
  localIps,
  onUpdateLan,
  onUpdateEmbeddedHub,
  onSaveProfile,
  onDeleteProfile,
  onToggleProfile,
  onTestProfile,
  onGenerateRoomCode,
  onShowQr,
}: Props) {
  const [lanDraft, setLanDraft] = useState<LanDiscoveryConfig>(config.lan);
  const [hubDraft, setHubDraft] = useState<EmbeddedHubConfig>(config.embedded_hub);

  useEffect(() => {
    setLanDraft(config.lan);
  }, [config.lan]);

  useEffect(() => {
    setHubDraft(config.embedded_hub);
  }, [config.embedded_hub]);
  const [copiedKey, setCopiedKey] = useState<string | null>(null);
  const [testResults, setTestResults] = useState<Record<string, ConnectionTestResult>>({});
  const [testingId, setTestingId] = useState<string | null>(null);

  // Add / Edit Profile Modal State
  const [modalOpen, setModalOpen] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [mode, setMode] = useState<ModalMode>("external_hub");
  const [profName, setProfName] = useState("");

  // Fields per mode
  const [dpHost, setDpHost] = useState("");
  const [dpPort, setDpPort] = useState(48215);
  const [dpTls, setDpTls] = useState(false);
  const [dpFp, setDpFp] = useState("");
  const [dpSecret, setDpSecret] = useState("");

  const [hubUrl, setHubUrl] = useState("wss://192.168.1.10:48220");
  const [hubFp, setHubFp] = useState("");
  const [hubSecret, setHubSecret] = useState("");

  const [relayUrl, setRelayUrl] = useState("wss://relay.example.com");
  const [roomCode, setRoomCode] = useState("");

  const [sshHost, setSshHost] = useState("");
  const [sshPort, setSshPort] = useState(22);
  const [sshUser, setSshUser] = useState("");
  const [sshKeyPath, setSshKeyPath] = useState("");
  const [sshRemoteHost, setSshRemoteHost] = useState("127.0.0.1");
  const [sshRemotePort, setSshRemotePort] = useState(48220);
  const [sshSecret, setSshSecret] = useState("");

  const [modalTestResult, setModalTestResult] = useState<ConnectionTestResult | null>(null);
  const [modalTesting, setModalTesting] = useState(false);

  const copyText = async (tag: string, text: string) => {
    await navigator.clipboard.writeText(text);
    setCopiedKey(tag);
    setTimeout(() => setCopiedKey(null), 1500);
  };

  const openCreateModal = async () => {
    setEditingId(null);
    setMode("wan_relay");
    setProfName("Cross-Network Room");
    setDpHost("");
    setDpPort(48215);
    setDpTls(false);
    setDpFp("");
    setDpSecret("");
    setHubUrl("wss://192.168.1.10:48220");
    setHubFp("");
    setHubSecret("");
    setRelayUrl("ws://127.0.0.1:48220");
    setRoomCode(await onGenerateRoomCode());
    setSshHost("");
    setSshPort(22);
    setSshUser("");
    setSshKeyPath("");
    setSshRemoteHost("127.0.0.1");
    setSshRemotePort(48220);
    setSshSecret("");
    setModalTestResult(null);
    setModalOpen(true);
  };

  const openEditModal = (p: ConnectionProfile) => {
    setEditingId(p.id);
    setProfName(p.name);
    setModalTestResult(null);
    const k = p.kind;
    setMode(k.kind);
    if (k.kind === "direct_peer") {
      setDpHost(k.host);
      setDpPort(k.port);
      setDpTls(k.use_tls);
      setDpFp(k.cert_fingerprint ?? "");
      setDpSecret(k.shared_secret);
    } else if (k.kind === "external_hub") {
      setHubUrl(k.url);
      setHubFp(k.cert_fingerprint ?? "");
      setHubSecret(k.room_secret);
    } else if (k.kind === "wan_relay") {
      setRelayUrl(k.relay_url);
      setRoomCode(k.room_code);
    } else if (k.kind === "ssh_tunnel") {
      setSshHost(k.ssh_host);
      setSshPort(k.ssh_port);
      setSshUser(k.ssh_user ?? "");
      setSshKeyPath(k.identity_file ?? "");
      setSshRemoteHost(k.remote_host);
      setSshRemotePort(k.remote_port);
      setSshSecret(k.room_secret);
    }
    setModalOpen(true);
  };

  const buildProfileFromModal = (): ConnectionProfile => {
    let kind: ConnectionProfileKind;
    if (mode === "direct_peer") {
      kind = {
        kind: "direct_peer",
        host: dpHost.trim(),
        port: Number(dpPort) || 48215,
        use_tls: dpTls,
        cert_fingerprint: dpFp.trim() ? dpFp.trim() : null,
        shared_secret: dpSecret.trim(),
      };
    } else if (mode === "external_hub") {
      kind = {
        kind: "external_hub",
        url: hubUrl.trim(),
        cert_fingerprint: hubFp.trim() ? hubFp.trim() : null,
        room_secret: hubSecret.trim(),
      };
    } else if (mode === "wan_relay") {
      kind = {
        kind: "wan_relay",
        relay_url: relayUrl.trim(),
        room_code: roomCode.trim(),
      };
    } else {
      kind = {
        kind: "ssh_tunnel",
        ssh_host: sshHost.trim(),
        ssh_port: Number(sshPort) || 22,
        ssh_user: sshUser.trim() ? sshUser.trim() : null,
        identity_file: sshKeyPath.trim() ? sshKeyPath.trim() : null,
        remote_host: sshRemoteHost.trim() || "127.0.0.1",
        remote_port: Number(sshRemotePort) || 48220,
        room_secret: sshSecret.trim(),
      };
    }

    const existing = editingId
      ? config.profiles.find((p) => p.id === editingId)
      : undefined;

    return {
      id: editingId ?? crypto.randomUUID(),
      name: profName.trim() || "Connection Profile",
      enabled: existing ? existing.enabled : true,
      kind,
    };
  };

  const handleSaveModal = async () => {
    const prof = buildProfileFromModal();
    await onSaveProfile(prof);
    setModalOpen(false);
  };

  const handleTestModal = async () => {
    setModalTesting(true);
    setModalTestResult(null);
    try {
      const res = await onTestProfile(buildProfileFromModal());
      setModalTestResult(res);
    } finally {
      setModalTesting(false);
    }
  };

  const handlePinDetectedCert = async (profileId: string, fp: string) => {
    const prof = config.profiles.find((p) => p.id === profileId);
    if (!prof) return;
    const updated: ConnectionProfile = {
      ...prof,
      kind:
        prof.kind.kind === "external_hub"
          ? { ...prof.kind, cert_fingerprint: fp }
          : prof.kind.kind === "direct_peer"
          ? { ...prof.kind, cert_fingerprint: fp }
          : prof.kind,
    };
    await onSaveProfile(updated);
  };

  const stateBadge = (state: string) => {
    switch (state) {
      case "connected":
        return "bg-emerald-500/15 text-emerald-700 dark:text-emerald-300";
      case "connecting":
      case "reconnecting":
        return "bg-amber-500/15 text-amber-700 dark:text-amber-300";
      case "error":
        return "bg-rose-500/15 text-rose-700 dark:text-rose-300";
      default:
        return "bg-slate-200 text-slate-600 dark:bg-slate-800 dark:text-slate-400";
    }
  };

  const modeCards: {
    id: ModalMode;
    title: string;
    subtitle: string;
    icon: typeof Wifi;
  }[] = [
    {
      id: "direct_peer",
      title: "Direct / VPN Mesh",
      subtitle: "Static IP, Tailscale, WireGuard, ZeroTier",
      icon: Wifi,
    },
    {
      id: "external_hub",
      title: "Self-Hosted Hub",
      subtitle: "Clipboardwire-style WebSocket/TLS Hub",
      icon: Server,
    },
    {
      id: "wan_relay",
      title: "WAN Relay Room",
      subtitle: "E2EE cross-network sync (Office ↔ 4G)",
      icon: Globe,
    },
    {
      id: "ssh_tunnel",
      title: "SSH Tunnel",
      subtitle: "Bypass strict firewall via SSH -L",
      icon: Terminal,
    },
  ];

  const emb = snapshot.embedded_hub;

  return (
    <div className="space-y-5">
      {/* Row 1: LAN Auto-Discovery + Embedded Hub */}
      <div className="grid gap-4 lg:grid-cols-2">
        {/* LAN Discovery Card */}
        <section className="rounded-2xl border border-slate-200 bg-white p-4 shadow-xs dark:border-slate-800 dark:bg-slate-900/80">
          <div className="mb-3 flex items-center justify-between">
            <div className="flex items-center gap-2">
              <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-cyan-500/15 text-cyan-600 dark:text-cyan-300">
                <Network className="h-4 w-4" />
              </div>
              <div>
                <h3 className="text-sm font-bold text-slate-900 dark:text-white">
                  LAN Auto-Discovery (P2P)
                </h3>
                <p className="text-[11px] text-slate-500 dark:text-slate-400">
                  Zero-config mDNS + UDP beacon on local Wi-Fi/Ethernet
                </p>
              </div>
            </div>
            <button
              type="button"
              onClick={() => {
                const next = { ...lanDraft, enabled: !lanDraft.enabled };
                setLanDraft(next);
                onUpdateLan(next);
              }}
              className={`rounded-full px-3 py-1 text-xs font-bold transition ${
                lanDraft.enabled
                  ? "bg-emerald-500 text-slate-950"
                  : "bg-slate-200 text-slate-600 dark:bg-slate-800 dark:text-slate-400"
              }`}
            >
              {lanDraft.enabled ? "Enabled" : "Disabled"}
            </button>
          </div>

          <div className="mb-3 flex flex-wrap gap-2 text-[11px]">
            <span
              className={`inline-flex items-center gap-1 rounded-full px-2 py-0.5 font-medium ${
                snapshot.discovery_status.mdns_active
                  ? "bg-emerald-500/15 text-emerald-700 dark:text-emerald-300"
                  : "bg-slate-200 text-slate-500 dark:bg-slate-800 dark:text-slate-400"
              }`}
            >
              <span className="h-1.5 w-1.5 rounded-full bg-current" />
              mDNS
            </span>
            <span
              className={`inline-flex items-center gap-1 rounded-full px-2 py-0.5 font-medium ${
                snapshot.discovery_status.udp_active
                  ? "bg-emerald-500/15 text-emerald-700 dark:text-emerald-300"
                  : "bg-slate-200 text-slate-500 dark:bg-slate-800 dark:text-slate-400"
              }`}
            >
              <span className="h-1.5 w-1.5 rounded-full bg-current" />
              UDP :{lanDraft.udp_beacon_port}
            </span>
            <span className="rounded-full bg-slate-100 px-2 py-0.5 font-mono text-slate-600 dark:bg-slate-800 dark:text-slate-300">
              P2P :{lanDraft.listen_port}
            </span>
          </div>

          {/* Local IP Interfaces */}
          <div className="space-y-1.5">
            <div className="text-[11px] font-semibold uppercase tracking-wider text-slate-400">
              This Machine IPs (click to copy)
            </div>
            <div className="flex flex-wrap gap-1.5">
              {localIps.map((iface) => (
                <button
                  key={`${iface.name}-${iface.ip}`}
                  type="button"
                  onClick={() => copyText(iface.ip, iface.ip)}
                  className="inline-flex items-center gap-1.5 rounded-lg border border-slate-200 bg-slate-50 px-2 py-1 text-[11px] font-mono text-slate-700 hover:border-cyan-500/50 dark:border-slate-700 dark:bg-slate-950 dark:text-slate-200"
                >
                  <span
                    className={`rounded px-1 text-[9px] font-bold uppercase ${
                      iface.category === "vpn"
                        ? "bg-violet-500/20 text-violet-600 dark:text-violet-300"
                        : "bg-cyan-500/15 text-cyan-700 dark:text-cyan-300"
                    }`}
                  >
                    {iface.category}
                  </span>
                  <span>{iface.ip}</span>
                  {copiedKey === iface.ip ? (
                    <Check className="h-3 w-3 text-emerald-500" />
                  ) : (
                    <Copy className="h-3 w-3 text-slate-400" />
                  )}
                </button>
              ))}
              {localIps.length === 0 && (
                <span className="text-xs text-slate-400">No non-loopback IPv4 found</span>
              )}
            </div>
          </div>
        </section>

        {/* Embedded Hub Card (Clipboardwire mode) */}
        <section className="rounded-2xl border border-slate-200 bg-white p-4 shadow-xs dark:border-slate-800 dark:bg-slate-900/80">
          <div className="mb-3 flex items-center justify-between">
            <div className="flex items-center gap-2">
              <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-violet-500/15 text-violet-600 dark:text-violet-300">
                <HardDrive className="h-4 w-4" />
              </div>
              <div>
                <h3 className="text-sm font-bold text-slate-900 dark:text-white">
                  Embedded Hub Mode
                </h3>
                <p className="text-[11px] text-slate-500 dark:text-slate-400">
                  Run a TLS Hub on this PC while participating as a client
                </p>
              </div>
            </div>
            <button
              type="button"
              onClick={() => {
                const next = { ...hubDraft, enabled: !hubDraft.enabled };
                setHubDraft(next);
                onUpdateEmbeddedHub(next);
              }}
              className={`rounded-full px-3 py-1 text-xs font-bold transition ${
                emb.running
                  ? "bg-emerald-500 text-slate-950"
                  : "bg-slate-200 text-slate-600 dark:bg-slate-800 dark:text-slate-400"
              }`}
            >
              {emb.running ? "Running" : "Start Hub"}
            </button>
          </div>

          <div className="grid grid-cols-2 gap-2 text-xs">
            <label className="block">
              <span className="text-[11px] text-slate-500 dark:text-slate-400">Port</span>
              <input
                type="number"
                value={hubDraft.port}
                onChange={(e) =>
                  setHubDraft({ ...hubDraft, port: Number(e.target.value) || 48220 })
                }
                className="mt-0.5 w-full rounded-lg border border-slate-300 bg-white px-2.5 py-1 font-mono text-xs dark:border-slate-700 dark:bg-slate-950"
              />
            </label>
            <label className="flex items-end gap-2 pb-1">
              <input
                type="checkbox"
                checked={hubDraft.use_tls}
                onChange={(e) =>
                  setHubDraft({ ...hubDraft, use_tls: e.target.checked })
                }
                className="h-4 w-4 accent-cyan-500"
              />
              <span className="text-xs font-medium text-slate-700 dark:text-slate-200">
                Auto-TLS (wss://)
              </span>
            </label>
          </div>

          <div className="mt-2.5 flex items-center gap-2">
            <input
              readOnly
              value={hubDraft.room_secret}
              className="flex-1 rounded-lg border border-slate-200 bg-slate-50 px-2.5 py-1 font-mono text-[11px] text-slate-600 dark:border-slate-800 dark:bg-slate-950 dark:text-slate-300"
            />
            <button
              type="button"
              onClick={() => copyText("hub_sec", hubDraft.room_secret)}
              className="rounded-lg border border-slate-300 px-2 py-1 text-xs hover:bg-slate-100 dark:border-slate-700 dark:hover:bg-slate-800"
              title="Copy Hub Secret"
            >
              {copiedKey === "hub_sec" ? (
                <Check className="h-3.5 w-3.5 text-emerald-500" />
              ) : (
                <Copy className="h-3.5 w-3.5" />
              )}
            </button>
            <button
              type="button"
              onClick={() => onUpdateEmbeddedHub(hubDraft)}
              className="rounded-lg bg-slate-900 px-2.5 py-1 text-xs font-semibold text-white hover:bg-slate-800 dark:bg-slate-100 dark:text-slate-950"
            >
              Apply
            </button>
          </div>

          {emb.running && (
            <div className="mt-2.5 flex flex-wrap items-center justify-between gap-2 rounded-xl bg-emerald-500/10 px-3 py-2 text-[11px]">
              <span className="font-semibold text-emerald-700 dark:text-emerald-300">
                {emb.connected_clients} connected client(s) · :{emb.port}
              </span>
              <button
                type="button"
                onClick={onShowQr}
                className="inline-flex items-center gap-1 font-semibold text-cyan-600 hover:underline dark:text-cyan-300"
              >
                <QrCode className="h-3.5 w-3.5" />
                Share Hub QR / URI
              </button>
            </div>
          )}
          {emb.error && (
            <div className="mt-2 rounded-lg bg-rose-500/10 px-2.5 py-1.5 text-xs text-rose-500">
              {emb.error}
            </div>
          )}
        </section>
      </div>

      {/* Saved Multi-Transport Connection Profiles */}
      <section className="space-y-3">
        <div className="flex items-center justify-between">
          <div>
            <h2 className="text-sm font-bold text-slate-900 dark:text-white">
              Connection Profiles (Hub · Direct/VPN · WAN Relay · SSH Tunnel)
            </h2>
            <p className="text-xs text-slate-500 dark:text-slate-400">
              Run multiple transports concurrently across home, office, Tailscale, or 4G networks.
            </p>
          </div>
          <button
            type="button"
            onClick={openCreateModal}
            className="inline-flex items-center gap-1.5 rounded-xl bg-cyan-500 px-3.5 py-2 text-xs font-bold text-slate-950 shadow-sm hover:bg-cyan-400"
          >
            <Plus className="h-4 w-4" />
            Add Connection
          </button>
        </div>

        <div className="space-y-2.5">
          {config.profiles.map((prof) => {
            const st = snapshot.profile_statuses.find((s) => s.profile_id === prof.id);
            const testRes = testResults[prof.id];
            const state = st?.state ?? (prof.enabled ? "connecting" : "disabled");

            return (
              <div
                key={prof.id}
                className="rounded-2xl border border-slate-200 bg-white p-4 shadow-xs dark:border-slate-800 dark:bg-slate-900/80"
              >
                <div className="flex flex-wrap items-center justify-between gap-3">
                  <div className="min-w-0">
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="text-sm font-bold text-slate-900 dark:text-white">
                        {prof.name}
                      </span>
                      <span className="rounded bg-slate-100 px-1.5 py-0.5 text-[10px] font-semibold uppercase text-slate-600 dark:bg-slate-800 dark:text-slate-300">
                        {prof.kind.kind.replace("_", " ")}
                      </span>
                      <span
                        className={`rounded-full px-2 py-0.5 text-[10px] font-bold uppercase ${stateBadge(
                          state
                        )}`}
                      >
                        {state}
                        {st && st.attempt > 1 && state === "reconnecting"
                          ? ` #${st.attempt}`
                          : ""}
                      </span>
                      {st?.rtt_ms !== null && st?.rtt_ms !== undefined && (
                        <span className="font-mono text-[11px] text-slate-500 dark:text-slate-400">
                          {st.rtt_ms} ms · {st.peer_count} peers
                        </span>
                      )}
                    </div>
                    <div className="mt-1 text-xs text-slate-500 dark:text-slate-400">
                      {st?.detail ?? "Idle"}
                    </div>
                  </div>

                  <div className="flex items-center gap-1.5">
                    <button
                      type="button"
                      disabled={testingId === prof.id}
                      onClick={async () => {
                        setTestingId(prof.id);
                        try {
                          const r = await onTestProfile(prof);
                          setTestResults((prev) => ({ ...prev, [prof.id]: r }));
                        } finally {
                          setTestingId(null);
                        }
                      }}
                      className="inline-flex items-center gap-1 rounded-lg border border-slate-300 px-2.5 py-1 text-xs font-medium text-slate-700 hover:bg-slate-100 dark:border-slate-700 dark:text-slate-200 dark:hover:bg-slate-800"
                    >
                      <Play className="h-3 w-3" />
                      {testingId === prof.id ? "Testing..." : "Test"}
                    </button>
                    <button
                      type="button"
                      onClick={() => openEditModal(prof)}
                      className="rounded-lg p-1.5 text-slate-400 hover:bg-slate-100 hover:text-slate-700 dark:hover:bg-slate-800"
                    >
                      <Pencil className="h-4 w-4" />
                    </button>
                    <button
                      type="button"
                      onClick={() => onToggleProfile(prof.id, !prof.enabled)}
                      className={`rounded-lg px-2.5 py-1 text-xs font-semibold ${
                        prof.enabled
                          ? "bg-emerald-500/15 text-emerald-700 dark:text-emerald-300"
                          : "bg-slate-200 text-slate-600 dark:bg-slate-800 dark:text-slate-400"
                      }`}
                    >
                      {prof.enabled ? "On" : "Off"}
                    </button>
                    <button
                      type="button"
                      onClick={() => onDeleteProfile(prof.id)}
                      className="rounded-lg p-1.5 text-slate-400 hover:bg-slate-100 hover:text-rose-500 dark:hover:bg-slate-800"
                    >
                      <Trash2 className="h-4 w-4" />
                    </button>
                  </div>
                </div>

                {/* Live error + 1-click self-signed cert pin */}
                {(st?.last_error || testRes) && (
                  <div className="mt-2.5 space-y-1.5">
                    {testRes && (
                      <div
                        className={`flex flex-wrap items-center justify-between gap-2 rounded-lg px-3 py-1.5 text-xs ${
                          testRes.ok
                            ? "bg-emerald-500/10 text-emerald-700 dark:text-emerald-300"
                            : "bg-rose-500/10 text-rose-600 dark:text-rose-300"
                        }`}
                      >
                        <span>
                          {testRes.message}
                          {testRes.rtt_ms !== null ? ` (${testRes.rtt_ms} ms)` : ""}
                        </span>
                        {testRes.detected_cert_fingerprint && (
                          <button
                            type="button"
                            onClick={() =>
                              handlePinDetectedCert(
                                prof.id,
                                testRes.detected_cert_fingerprint!
                              )
                            }
                            className="inline-flex items-center gap-1 rounded bg-cyan-500 px-2 py-0.5 text-[11px] font-bold text-slate-950"
                          >
                            <ShieldCheck className="h-3 w-3" />
                            Trust & Pin Cert
                          </button>
                        )}
                      </div>
                    )}
                    {!testRes && st?.last_error && (
                      <div className="flex flex-wrap items-center justify-between gap-2 rounded-lg bg-amber-500/10 px-3 py-1.5 text-xs text-amber-700 dark:text-amber-300">
                        <span className="flex items-center gap-1.5">
                          <AlertTriangle className="h-3.5 w-3.5 shrink-0" />
                          {st.last_error}
                        </span>
                        {st.detected_cert_fingerprint && (
                          <button
                            type="button"
                            onClick={() =>
                              handlePinDetectedCert(
                                prof.id,
                                st.detected_cert_fingerprint!
                              )
                            }
                            className="inline-flex items-center gap-1 rounded bg-cyan-500 px-2 py-0.5 text-[11px] font-bold text-slate-950"
                          >
                            <ShieldCheck className="h-3 w-3" />
                            Trust Self-Signed Cert
                          </button>
                        )}
                      </div>
                    )}
                  </div>
                )}
              </div>
            );
          })}

          {config.profiles.length === 0 && (
            <div className="rounded-2xl border border-dashed border-slate-300 py-8 text-center text-xs text-slate-500 dark:border-slate-700 dark:text-slate-400">
              No custom connection profiles yet. Click &ldquo;Add Connection&rdquo; to add a Hub, Direct VPN peer, WAN Relay room, or SSH tunnel.
            </div>
          )}
        </div>
      </section>

      {/* Add / Edit Connection Modal */}
      {modalOpen && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-950/75 p-4 backdrop-blur-xs">
          <div className="max-h-[90vh] w-full max-w-xl overflow-y-auto rounded-2xl border border-slate-200 bg-white p-5 shadow-2xl dark:border-slate-800 dark:bg-slate-900">
            <div className="mb-4 flex items-center justify-between">
              <h3 className="text-base font-bold text-slate-900 dark:text-white">
                {editingId ? "Edit Connection Profile" : "Add Connection Method"}
              </h3>
              <button
                type="button"
                onClick={() => setModalOpen(false)}
                className="rounded-lg p-1 text-slate-400 hover:bg-slate-100 dark:hover:bg-slate-800"
              >
                <X className="h-4 w-4" />
              </button>
            </div>

            {/* 4-Card Context Selector */}
            <div className="mb-4 grid grid-cols-2 gap-2">
              {modeCards.map((c) => {
                const Icon = c.icon;
                const active = mode === c.id;
                return (
                  <button
                    key={c.id}
                    type="button"
                    onClick={() => {
                      setMode(c.id);
                      setModalTestResult(null);
                    }}
                    className={`rounded-xl border p-2.5 text-left transition ${
                      active
                        ? "border-cyan-500 bg-cyan-500/10"
                        : "border-slate-200 hover:bg-slate-50 dark:border-slate-800 dark:hover:bg-slate-800/50"
                    }`}
                  >
                    <div className="flex items-center gap-1.5 text-xs font-bold text-slate-900 dark:text-white">
                      <Icon className="h-3.5 w-3.5 text-cyan-500" />
                      {c.title}
                    </div>
                    <div className="mt-0.5 text-[11px] text-slate-500 dark:text-slate-400">
                      {c.subtitle}
                    </div>
                  </button>
                );
              })}
            </div>

            <div className="space-y-3 text-xs">
              <label className="block">
                <span className="font-semibold text-slate-700 dark:text-slate-300">
                  Profile Name
                </span>
                <input
                  value={profName}
                  onChange={(e) => setProfName(e.target.value)}
                  className="mt-1 w-full rounded-lg border border-slate-300 bg-white px-3 py-1.5 dark:border-slate-700 dark:bg-slate-950"
                />
              </label>

              {mode === "direct_peer" && (
                <>
                  <div className="grid grid-cols-3 gap-2">
                    <label className="col-span-2 block">
                      <span className="font-semibold">Peer IP / Tailscale Host</span>
                      <input
                        value={dpHost}
                        onChange={(e) => setDpHost(e.target.value)}
                        placeholder="100.64.0.12 or 192.168.1.50"
                        className="mt-1 w-full rounded-lg border border-slate-300 bg-white px-3 py-1.5 font-mono dark:border-slate-700 dark:bg-slate-950"
                      />
                    </label>
                    <label className="block">
                      <span className="font-semibold">Port</span>
                      <input
                        type="number"
                        value={dpPort}
                        onChange={(e) => setDpPort(Number(e.target.value))}
                        className="mt-1 w-full rounded-lg border border-slate-300 bg-white px-3 py-1.5 font-mono dark:border-slate-700 dark:bg-slate-950"
                      />
                    </label>
                  </div>
                  <label className="block">
                    <span className="font-semibold">Shared Secret</span>
                    <input
                      value={dpSecret}
                      onChange={(e) => setDpSecret(e.target.value)}
                      className="mt-1 w-full rounded-lg border border-slate-300 bg-white px-3 py-1.5 font-mono dark:border-slate-700 dark:bg-slate-950"
                    />
                  </label>
                  <label className="flex items-center gap-2">
                    <input
                      type="checkbox"
                      checked={dpTls}
                      onChange={(e) => setDpTls(e.target.checked)}
                      className="accent-cyan-500"
                    />
                    <span>Use TLS (wss://)</span>
                  </label>
                </>
              )}

              {mode === "external_hub" && (
                <>
                  <label className="block">
                    <span className="font-semibold">Hub WebSocket URL</span>
                    <input
                      value={hubUrl}
                      onChange={(e) => setHubUrl(e.target.value)}
                      placeholder="wss://hub.example.com:48220"
                      className="mt-1 w-full rounded-lg border border-slate-300 bg-white px-3 py-1.5 font-mono dark:border-slate-700 dark:bg-slate-950"
                    />
                  </label>
                  <label className="block">
                    <span className="font-semibold">Hub / Room E2EE Secret</span>
                    <input
                      value={hubSecret}
                      onChange={(e) => setHubSecret(e.target.value)}
                      placeholder="Shared secret for encryption & hub auth"
                      className="mt-1 w-full rounded-lg border border-slate-300 bg-white px-3 py-1.5 font-mono dark:border-slate-700 dark:bg-slate-950"
                    />
                  </label>
                  <label className="block">
                    <span className="font-semibold">
                      Self-Signed TLS Fingerprint (optional)
                    </span>
                    <input
                      value={hubFp}
                      onChange={(e) => setHubFp(e.target.value)}
                      placeholder="SHA-256 hex (click Test to auto-detect)"
                      className="mt-1 w-full rounded-lg border border-slate-300 bg-white px-3 py-1.5 font-mono dark:border-slate-700 dark:bg-slate-950"
                    />
                  </label>
                </>
              )}

              {mode === "wan_relay" && (
                <>
                  <label className="block">
                    <span className="font-semibold">Relay Server URL</span>
                    <input
                      value={relayUrl}
                      onChange={(e) => setRelayUrl(e.target.value)}
                      placeholder="wss://your-relay-host:48220"
                      className="mt-1 w-full rounded-lg border border-slate-300 bg-white px-3 py-1.5 font-mono dark:border-slate-700 dark:bg-slate-950"
                    />
                  </label>
                  <label className="block">
                    <span className="font-semibold">
                      Zero-Knowledge E2EE Room Code
                    </span>
                    <div className="mt-1 flex gap-2">
                      <input
                        value={roomCode}
                        onChange={(e) => setRoomCode(e.target.value)}
                        className="flex-1 rounded-lg border border-slate-300 bg-white px-3 py-1.5 font-mono dark:border-slate-700 dark:bg-slate-950"
                      />
                      <button
                        type="button"
                        onClick={async () => setRoomCode(await onGenerateRoomCode())}
                        className="inline-flex items-center gap-1 rounded-lg border border-slate-300 px-2.5 py-1.5 font-medium hover:bg-slate-100 dark:border-slate-700 dark:hover:bg-slate-800"
                      >
                        <RefreshCw className="h-3.5 w-3.5" />
                        Generate
                      </button>
                    </div>
                  </label>
                </>
              )}

              {mode === "ssh_tunnel" && (
                <>
                  <div className="grid grid-cols-3 gap-2">
                    <label className="col-span-2 block">
                      <span className="font-semibold">SSH Bastion / Server Host</span>
                      <input
                        value={sshHost}
                        onChange={(e) => setSshHost(e.target.value)}
                        placeholder="bastion.company.com"
                        className="mt-1 w-full rounded-lg border border-slate-300 bg-white px-3 py-1.5 font-mono dark:border-slate-700 dark:bg-slate-950"
                      />
                    </label>
                    <label className="block">
                      <span className="font-semibold">SSH Port</span>
                      <input
                        type="number"
                        value={sshPort}
                        onChange={(e) => setSshPort(Number(e.target.value))}
                        className="mt-1 w-full rounded-lg border border-slate-300 bg-white px-3 py-1.5 font-mono dark:border-slate-700 dark:bg-slate-950"
                      />
                    </label>
                  </div>
                  <div className="grid grid-cols-2 gap-2">
                    <label className="block">
                      <span className="font-semibold">SSH User (optional)</span>
                      <input
                        value={sshUser}
                        onChange={(e) => setSshUser(e.target.value)}
                        placeholder="ubuntu"
                        className="mt-1 w-full rounded-lg border border-slate-300 bg-white px-3 py-1.5 font-mono dark:border-slate-700 dark:bg-slate-950"
                      />
                    </label>
                    <label className="block">
                      <span className="font-semibold">Identity Key Path (optional)</span>
                      <input
                        value={sshKeyPath}
                        onChange={(e) => setSshKeyPath(e.target.value)}
                        placeholder="~/.ssh/id_ed25519"
                        className="mt-1 w-full rounded-lg border border-slate-300 bg-white px-3 py-1.5 font-mono dark:border-slate-700 dark:bg-slate-950"
                      />
                    </label>
                  </div>
                  <div className="grid grid-cols-3 gap-2">
                    <label className="col-span-2 block">
                      <span className="font-semibold">Remote Hub Host</span>
                      <input
                        value={sshRemoteHost}
                        onChange={(e) => setSshRemoteHost(e.target.value)}
                        className="mt-1 w-full rounded-lg border border-slate-300 bg-white px-3 py-1.5 font-mono dark:border-slate-700 dark:bg-slate-950"
                      />
                    </label>
                    <label className="block">
                      <span className="font-semibold">Remote Port</span>
                      <input
                        type="number"
                        value={sshRemotePort}
                        onChange={(e) => setSshRemotePort(Number(e.target.value))}
                        className="mt-1 w-full rounded-lg border border-slate-300 bg-white px-3 py-1.5 font-mono dark:border-slate-700 dark:bg-slate-950"
                      />
                    </label>
                  </div>
                  <label className="block">
                    <span className="font-semibold">Room Secret</span>
                    <input
                      value={sshSecret}
                      onChange={(e) => setSshSecret(e.target.value)}
                      className="mt-1 w-full rounded-lg border border-slate-300 bg-white px-3 py-1.5 font-mono dark:border-slate-700 dark:bg-slate-950"
                    />
                  </label>
                </>
              )}

              {modalTestResult && (
                <div
                  className={`flex flex-wrap items-center justify-between gap-2 rounded-lg p-2.5 ${
                    modalTestResult.ok
                      ? "bg-emerald-500/10 text-emerald-700 dark:text-emerald-300"
                      : "bg-rose-500/10 text-rose-600 dark:text-rose-300"
                  }`}
                >
                  <span>{modalTestResult.message}</span>
                  {modalTestResult.detected_cert_fingerprint && (
                    <button
                      type="button"
                      onClick={() => {
                        if (mode === "external_hub") {
                          setHubFp(modalTestResult.detected_cert_fingerprint!);
                        } else if (mode === "direct_peer") {
                          setDpFp(modalTestResult.detected_cert_fingerprint!);
                        }
                      }}
                      className="inline-flex items-center gap-1 rounded bg-cyan-500 px-2 py-0.5 text-[11px] font-bold text-slate-950"
                    >
                      <Key className="h-3 w-3" />
                      Use Detected Cert Fingerprint
                    </button>
                  )}
                </div>
              )}
            </div>

            <div className="mt-5 flex items-center justify-between">
              <button
                type="button"
                disabled={modalTesting}
                onClick={handleTestModal}
                className="inline-flex items-center gap-1.5 rounded-lg border border-slate-300 px-3 py-1.5 text-xs font-semibold text-slate-700 hover:bg-slate-100 dark:border-slate-700 dark:text-slate-200 dark:hover:bg-slate-800"
              >
                <Play className="h-3.5 w-3.5" />
                {modalTesting ? "Testing..." : "Test Connection"}
              </button>

              <div className="flex gap-2">
                <button
                  type="button"
                  onClick={() => setModalOpen(false)}
                  className="rounded-lg px-3 py-1.5 text-xs font-medium text-slate-500 hover:bg-slate-100 dark:hover:bg-slate-800"
                >
                  Cancel
                </button>
                <button
                  type="button"
                  onClick={handleSaveModal}
                  className="rounded-lg bg-cyan-500 px-4 py-1.5 text-xs font-bold text-slate-950 hover:bg-cyan-400"
                >
                  Save Profile
                </button>
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
