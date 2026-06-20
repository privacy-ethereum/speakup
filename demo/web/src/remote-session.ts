// The remote-verifier glue: the brain of peer mode. Owns the remote mode
// (local / host / guest) and the pending proposal, builds the link event
// handlers, and coordinates the run controller, verifier view, stats, button,
// and control pane. Everything below the page (workers, bindings, the protocol)
// is unchanged — remote mode is purely this page-level coordination: the
// inviting device is the prover (host); the device that scans the QR becomes the
// verifier (guest).

import { joinInvite, type ControlMsg, type RemoteEvents, type RemoteLink } from "./remote";
import { compileAscSource } from "./guest-module";
import { sha256Hex } from "./dom";
import type { RemoteMode, RunController } from "./run-controller";
import type { VerifierView } from "./verifier-view";
import type { StatsView } from "./stats-view";
import type { RunButton } from "./run-button";
import type { ControlPane } from "./control-pane";

type ProposeMsg = Extract<ControlMsg, { kind: "propose" }>;

export interface RemoteSession {
  remoteMode(): RemoteMode;
  events: RemoteEvents;
  /// A verifier connected to our invite: version-check and adopt as host.
  setHost(link: RemoteLink): void;
  /// Tear down the link on purpose (disconnect button, version mismatch, mode).
  disconnect(reason: string): void;
  /// Both workers are now ready — resume a proposal accepted while still loading.
  onWorkersReady(): void;
  /// Verifier accept/decline (wired from the verifier view's buttons).
  acceptRun(): void;
  declineRun(): void;
  /// Join-link bootstrap: show the connect gate (nothing connects until the
  /// user opts in — connecting reveals their IP to the prover).
  bootstrapJoin(joinParam: string): void;
  /// Gate accepted: actually dial the host as the verifier.
  confirmJoin(): void;
  /// Gate declined: abandon the join and restore the authoring UI.
  cancelJoin(): void;
}

export const initRemoteSession = (opts: {
  controller: RunController;
  verifierView: VerifierView;
  statsView: StatsView;
  runButton: RunButton;
  controlPane: ControlPane;
  version: string;
}): RemoteSession => {
  const { controller, verifierView, statsView, runButton, controlPane, version } = opts;

  let remote: RemoteMode = { kind: "local" };
  /// A proposal the verifier is showing, awaiting the user's accept/decline.
  let pendingProposal: ProposeMsg | null = null;
  /// A proposal the user accepted before this device's wasm finished loading;
  /// proceeds once both workers are up (see onWorkersReady).
  let acceptedPending: ProposeMsg | null = null;

  const remoteMode = () => remote;

  // Live ping: an interval probes the peer every 2s and a timeout marks the
  // readout offline if pongs stop arriving (see the `ping`/`pong` control cases).
  let pingTimer: number | undefined;
  let lastPongAt = 0;

  const stopPing = () => {
    if (pingTimer !== undefined) {
      window.clearInterval(pingTimer);
      pingTimer = undefined;
    }
  };

  const startPing = (link: RemoteLink) => {
    stopPing();
    lastPongAt = performance.now();
    const tick = () => {
      if (remote.kind === "local" || remote.link !== link) {
        stopPing();
        return;
      }
      if (performance.now() - lastPongAt > 6000) controlPane.setPing(null); // stale
      link.sendControl({ kind: "ping", t: performance.now() });
    };
    tick();
    pingTimer = window.setInterval(tick, 2000);
  };

  /// Show the connected peer's status, IP (best effort, "unknown" otherwise),
  /// and start the live ping. The address retries once — the ICE stats settle a
  /// beat after the channel opens.
  const beginPeerSession = (link: RemoteLink, role: string) => {
    controlPane.setPeerStatus(`✓ ${role} connected`);
    startPing(link);
    void (async () => {
      let ep = await link.peerEndpoint();
      if (!ep.address) {
        await new Promise((r) => setTimeout(r, 700));
        ep = await link.peerEndpoint();
      }
      if (remote.kind !== "local" && remote.link === link) {
        controlPane.setPeerIp(ep.address ?? "unknown");
        controlPane.setConnType(ep.type);
      }
    })();
  };

  const setHost = (link: RemoteLink) => {
    // Same-deploy check; the guest's version rides in the metadata.
    if (link.peerVersion !== version) {
      link.sendControl({
        kind: "error",
        message: `deploy mismatch: prover runs ${version}, verifier ${link.peerVersion} — reload both devices`,
      });
      setTimeout(() => link.close(), 500); // let the message flush
      controlPane.reShowInvite();
      return;
    }
    link.sendControl({ kind: "hello", version });
    remote = { kind: "host", link };
    document.body.classList.add("remote-host");
    controlPane.markConnected();
    // A verifier is now connected, so the button is no longer "waiting for a
    // peer" — refresh it (enabled once the workers are up) to prove to them.
    runButton.set(controller.readyCount() === 2 ? "idle" : "loading");
    beginPeerSession(link, "verifier");
  };

  const onClose = (_reason: string) => {
    if (remote.kind === "local") return;
    stopPing();
    remote = { kind: "local" };
    pendingProposal = null;
    acceptedPending = null;
    document.body.classList.remove("remote-guest", "remote-host");
    controlPane.onRemoteClosed();
    verifierView.exit();
    if (controller.isRunning()) controller.abort(false);
    else runButton.set(controller.readyCount() === 2 ? "idle" : "loading");
  };

  /// Tears down the link on purpose (disconnect button, version mismatch).
  const disconnect = (reason: string) => {
    if (remote.kind === "local") return;
    remote.link.close(); // close() suppresses the link's own teardown event
    onClose(reason);
  };

  /// Guest side: a proposal arrived. Show what the prover wants to prove and let
  /// the user accept or decline — nothing runs until they accept.
  const onRemotePropose = (msg: ProposeMsg) => {
    if (remote.kind !== "guest" || controller.isRunning() || pendingProposal) return;
    pendingProposal = msg;
    verifierView.showProposal(msg);
  };

  /// Resolve the guest module (compiling + hash-checking the source for a custom
  /// AssemblyScript guest), then signal `ready` and begin verifying.
  const proceedAccept = async (msg: ProposeMsg) => {
    if (remote.kind !== "guest" || controller.isRunning()) return;
    const link = remote.link;

    let request = msg.request;
    if (msg.source && request.type === "run") {
      // Compile the prover's source ourselves and refuse unless it matches their
      // binary — the verifier runs only code it can read.
      verifierView.setStatus("Compiling the prover's AssemblyScript source…", "");
      let wasm: Uint8Array;
      try {
        wasm = await compileAscSource(msg.source.asc);
      } catch (e) {
        const detail = e instanceof Error ? e.message : String(e);
        link.sendControl({ kind: "error", message: `the verifier could not compile the source: ${detail}` });
        verifierView.setStatus(`Couldn't compile the prover's source:\n${detail}`);
        return;
      }
      if ((await sha256Hex(wasm)) !== msg.source.wasmHash) {
        link.sendControl({ kind: "error", message: "source/binary hash mismatch — the verifier refused to run" });
        verifierView.setStatus("The prover's binary doesn't match the source they sent — the verifier refused to run.");
        return;
      }
      request = { ...request, wasm };
    }
    // A run may have been aborted (or the link dropped) during the compile.
    if (remote.kind !== "guest" || controller.isRunning()) return;

    verifierView.showRunning(
      msg.tab,
      request.type === "run" ? request.wasm : new Uint8Array(),
      !!msg.source,
    );
    statsView.reset();
    controller.startVerifierRun(request, link);
  };

  /// Accept the pending proposal. Defers to `proceedAccept` once both workers
  /// are ready (so the `ready` handshake — and the prover's timer — waits for
  /// setup).
  const acceptRun = () => {
    if (!pendingProposal || remote.kind !== "guest" || controller.isRunning()) return;
    const msg = pendingProposal;
    pendingProposal = null;
    verifierView.beginAccept();
    if (controller.readyCount() < 2) {
      acceptedPending = msg; // resumed by onWorkersReady when the workers are up
      verifierView.setStatus("loading the verifier…");
      return;
    }
    void proceedAccept(msg);
  };

  const declineRun = () => {
    if (!pendingProposal || remote.kind !== "guest") return;
    pendingProposal = null;
    remote.link.sendControl({ kind: "decline" });
    verifierView.resetToWaiting();
  };

  const onRemoteControl = (msg: ControlMsg) => {
    switch (msg.kind) {
      case "hello":
        // Host → guest. Different deploys embed different guest modules, so a
        // cross-version run would fail in confusing ways — refuse early.
        if (msg.version !== version) disconnect("deploy mismatch — reload both devices");
        break;
      case "propose":
        onRemotePropose(msg);
        break;
      case "ready":
        // Verifier accepted and is set up. Begin proving now — timer starts here.
        if (remote.kind === "host") controller.attachProver(remote.link);
        break;
      case "decline":
        if (remote.kind === "host" && controller.isRunning()) {
          controller.cancel();
          runButton.set("error", "the verifier declined the request");
        }
        break;
      case "done":
        controller.onPeerDone(msg.result);
        break;
      case "error":
        if (controller.isRunning()) controller.fail(`remote device: ${msg.message}`);
        break;
      case "abort":
        // The prover aborted — whether mid-run or while we were still deciding on
        // (or loading for) a proposal. Clear it all and return to waiting.
        pendingProposal = null;
        acceptedPending = null;
        if (controller.isRunning()) controller.abort(false);
        if (remote.kind === "guest") verifierView.resetToWaiting();
        break;
      case "ping":
        // Echo the sender's timestamp verbatim so they measure RTT on one clock.
        if (remote.kind !== "local") remote.link.sendControl({ kind: "pong", t: msg.t });
        break;
      case "pong":
        lastPongAt = performance.now();
        controlPane.setPing(performance.now() - msg.t);
        break;
    }
  };

  const events: RemoteEvents = {
    onControl: onRemoteControl,
    onProtocol: (data) => controller.onRemoteProtocol(data),
    onClose,
  };

  const onWorkersReady = () => {
    // A proposal the user accepted before this device's wasm was ready.
    if (acceptedPending) {
      const msg = acceptedPending;
      acceptedPending = null;
      void proceedAccept(msg);
    }
  };

  /// A join link was followed but not yet confirmed; held until the user opts
  /// in at the connect gate (or cancels).
  let pendingJoin: string | null = null;

  const bootstrapJoin = (joinParam: string) => {
    pendingJoin = joinParam;
    verifierView.enterGate(); // ask before connecting — connecting reveals our IP
  };

  const confirmJoin = () => {
    if (pendingJoin === null) return;
    const joinParam = pendingJoin;
    pendingJoin = null;
    controlPane.enterJoinMode();
    verifierView.showConnecting();
    joinInvite(joinParam, version, {
      events,
      onConnected: (link) => {
        remote = { kind: "guest", link };
        document.body.classList.add("remote-guest");
        controlPane.markGuestConnected();
        verifierView.connected();
        beginPeerSession(link, "prover");
      },
      onError: (message) => {
        verifierView.exit(); // connection failed — give the authoring UI back
        controlPane.setPeerStatus(`couldn't connect: ${message}`);
      },
    });
  };

  const cancelJoin = () => {
    pendingJoin = null;
    verifierView.exit(); // the control pane was never touched — back to local
  };

  return {
    remoteMode,
    events,
    setHost,
    disconnect,
    onWorkersReady,
    acceptRun,
    declineRun,
    bootstrapJoin,
    confirmJoin,
    cancelJoin,
  };
};
