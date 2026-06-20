// The control pane: the run-mode tabs (Local / Peer / Attest) and the entire
// invite UI (QR + link) for hosting a remote verifier. Owns the selected mode
// and the pending invite handle. The connection-status row lives in this pane's
// DOM too, but it's driven by the remote session through the small interface
// returned here — the pane renders, the session decides.

import { hostInvite, type ConnType, type RemoteEvents, type RemoteLink } from "./remote";
import QRCode from "qrcode";

export type Mode = "local" | "peer";

const CONN_LABEL: Record<ConnType, string> = {
  local: "LAN",
  internet: "WAN",
  relayed: "WAN (relayed)",
  unknown: "—",
};

export interface ControlPane {
  mode(): Mode;
  /// Set the connection-status text (driven by the session's address probe).
  setPeerStatus(text: string): void;
  /// Set the peer's IP in the info table ("unknown" if undeterminable).
  setPeerIp(ip: string): void;
  /// Set how the peer is reached (same network / internet / relayed).
  setConnType(type: ConnType): void;
  /// Update the live ping readout — round-trip ms, or null for no response.
  setPing(ms: number | null): void;
  /// Host connected: reveal the connection row.
  markConnected(): void;
  /// Guest connected (join flow): reveal the disconnect button.
  markGuestConnected(): void;
  /// Re-offer the invite button (version mismatch, or after a closed invite).
  reShowInvite(): void;
  /// The link closed: hide the connection row, re-offer the invite in peer mode.
  onRemoteClosed(): void;
  /// Join-link bootstrap: force peer mode and show the "connecting…" state.
  enterJoinMode(): void;
}

export interface ControlPaneEls {
  controlTabs: HTMLElement;
  peerControls: HTMLElement;
  localExplainer: HTMLElement;
  inviteBtn: HTMLButtonElement;
  invitePanel: HTMLElement;
  inviteQr: HTMLCanvasElement;
  inviteLink: HTMLAnchorElement;
  inviteStatus: HTMLElement;
  inviteCancelBtn: HTMLButtonElement;
  remoteConnected: HTMLElement;
  remoteStatusEl: HTMLElement;
  remoteDisconnectBtn: HTMLButtonElement;
  peerIp: HTMLElement;
  peerConn: HTMLElement;
  peerPingDot: HTMLElement;
  peerPingVal: HTMLElement;
}

export const initControlPane = (opts: {
  els: ControlPaneEls;
  featureRemote: boolean;
  /// The link event handlers, obtained lazily — the remote session that owns
  /// them is constructed after this pane (they're only needed at invite time).
  events: () => RemoteEvents;
  onHostConnected(link: RemoteLink): void;
  onModeChanged(mode: Mode): void;
  requestDisconnect(reason: string): void;
}): ControlPane => {
  const { els } = opts;
  let mode: Mode = "local";
  let inviteHandle: { cancel(): void } | null = null;

  const joinUrlFor = (id: string) => {
    const url = new URL(location.href);
    url.searchParams.set("join", id);
    url.hash = "";
    return url.toString();
  };

  /// Switch the control-pane mode. Leaving peer mode drops any active link and
  /// cancels a pending invite (so nothing can connect while in local mode).
  const selectMode = (m: Mode) => {
    if (m === mode) return;
    if (mode === "peer") {
      opts.requestDisconnect("switched to local mode"); // no-ops if not connected
      inviteHandle?.cancel();
      inviteHandle = null;
    }
    mode = m;
    for (const b of els.controlTabs.querySelectorAll<HTMLButtonElement>("[data-mode]")) {
      b.classList.toggle("active", b.dataset.mode === m);
    }
    els.peerControls.hidden = m !== "peer";
    els.localExplainer.hidden = m !== "local";
    if (m === "peer") {
      // Fresh peer mode: offer the invite, nothing connected yet.
      els.inviteBtn.hidden = false;
      els.invitePanel.hidden = true;
      els.remoteConnected.hidden = true;
    }
    opts.onModeChanged(m);
  };

  els.controlTabs.addEventListener("click", (ev) => {
    const m = (ev.target as HTMLElement).closest<HTMLButtonElement>("[data-mode]")?.dataset.mode;
    // "attest" is not selectable yet (the tab only carries its coming-soon tip).
    if (m === "local" || m === "peer") selectMode(m);
  });

  els.inviteBtn.addEventListener("click", () => {
    els.inviteBtn.hidden = true;
    els.invitePanel.hidden = false;
    els.inviteQr.hidden = true;
    els.inviteLink.textContent = "";
    els.inviteStatus.textContent = "creating invite…";
    inviteHandle = hostInvite({
      joinUrl: joinUrlFor,
      onWaiting: (url) => {
        void QRCode.toCanvas(els.inviteQr, url, { width: 200, margin: 2 });
        els.inviteQr.hidden = false;
        els.inviteLink.href = url;
        els.inviteLink.textContent = url;
        els.inviteStatus.textContent =
          "scan with the verifier's device\n(both devices need this page open)";
      },
      onConnected: (link) => {
        els.invitePanel.hidden = true;
        opts.onHostConnected(link);
      },
      onError: (message) => {
        els.inviteStatus.textContent = `invite failed: ${message}`;
      },
      events: opts.events(),
    });
  });

  els.inviteCancelBtn.addEventListener("click", () => {
    inviteHandle?.cancel();
    inviteHandle = null;
    els.invitePanel.hidden = true;
    els.inviteBtn.hidden = false;
  });

  els.remoteDisconnectBtn.addEventListener("click", () =>
    opts.requestDisconnect("disconnected"),
  );

  // The Peer tab (and the whole remote feature) is gated by the feature flag.
  if (!opts.featureRemote) {
    els.controlTabs.querySelector<HTMLButtonElement>('[data-mode="peer"]')!.hidden = true;
  }

  const setPing = (ms: number | null) => {
    if (ms === null) {
      els.peerPingVal.textContent = "—";
      els.peerPingDot.classList.add("offline");
    } else {
      els.peerPingVal.textContent =
        ms < 10 ? `${ms.toFixed(1)} ms` : `${Math.round(ms)} ms`;
      els.peerPingDot.classList.remove("offline");
    }
  };

  /// Reset the info table to placeholders; the session fills IP, type, and ping.
  const resetInfo = () => {
    els.peerIp.textContent = "unknown";
    els.peerConn.textContent = CONN_LABEL.unknown;
    setPing(null);
  };

  return {
    mode: () => mode,
    setPeerStatus: (text) => {
      els.remoteStatusEl.textContent = text;
    },
    setPeerIp: (ip) => {
      els.peerIp.textContent = ip;
    },
    setConnType: (type) => {
      els.peerConn.textContent = CONN_LABEL[type];
    },
    setPing,
    markConnected: () => {
      els.remoteConnected.hidden = false;
      resetInfo();
    },
    markGuestConnected: () => {
      els.remoteDisconnectBtn.hidden = false;
      resetInfo();
    },
    reShowInvite: () => {
      els.inviteBtn.hidden = false;
    },
    onRemoteClosed: () => {
      els.remoteConnected.hidden = true;
      setPing(null);
      if (mode === "peer") els.inviteBtn.hidden = false; // ready to invite again
    },
    enterJoinMode: () => {
      selectMode("peer"); // this device joins as the verifier
      els.inviteBtn.hidden = true;
      els.remoteConnected.hidden = false;
      els.remoteDisconnectBtn.hidden = true;
      els.remoteStatusEl.textContent = "connecting to the prover's device…";
    },
  };
};
