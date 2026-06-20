// Composition root. Grabs the DOM handles, constructs each component in
// dependency order, wires the cross-component edges as callbacks, and kicks off
// the initial loads. All state and behaviour live in the components; this file
// is just the wiring diagram.
//
// The two-party protocol runs one worker per party; the page is the single
// crossing point between them (the run controller). Remote mode rewires that
// crossing point to a WebRTC link instead of a second local worker — see
// run-controller.ts and remote-session.ts.

import "./style.css";

import { $ } from "./dom";
import { EXAMPLES } from "./examples";
import { AWAITING_VERIFIER } from "./copy";
import { FEATURES } from "./config";
import { initTooltips } from "./tooltip";
import { initStatsView } from "./stats-view";
import { initRunButton } from "./run-button";
import { initEditors } from "./editors";
import { RunController } from "./run-controller";
import { initInspector, fetchWasm, type Inspector } from "./guest-module";
import { initGuestPanel, DEFAULT_ASC, type GuestPanel } from "./guest-panel";
import { initVerifierView } from "./verifier-view";
import { initControlPane, type ControlPane } from "./control-pane";
import { initRemoteSession, type RemoteSession } from "./remote-session";
import { decorateTabs } from "./tab-icons";
import type { PartyRequest } from "./party.worker";

const runBtn = $<HTMLButtonElement>("run");

// Components referenced by callbacks before they're constructed (the wiring is a
// small graph with a few back-edges); assigned below, read only once everything
// is live.
let inspector: Inspector;
let panel: GuestPanel;
let controlPane: ControlPane;
let remoteSession: RemoteSession;

const statsView = initStatsView({
  statsPane: $("stats"),
  statPv: $("stat-pv"),
  statVp: $("stat-vp"),
  statTime: $("stat-time"),
  resultBox: $("result-box"),
  resultEl: $("result"),
});

const runButton = initRunButton(runBtn, {
  isPeerWaiting: () =>
    controlPane.mode() === "peer" && remoteSession.remoteMode().kind === "local",
});
runButton.set("loading"); // workers are spawning

const editors = initEditors({
  orchestrationParent: $("editor"),
  ascParent: $("asc-editor"),
  initialScript: EXAMPLES[0].script,
  initialAsc: DEFAULT_ASC,
  onAscChange: () => panel.onAscChange(),
});

const controller = new RunController({
  onReady: (n) => {
    if (n === 2) {
      runButton.set("idle");
      remoteSession.onWorkersReady();
    }
  },
  onTraffic: (pv, vp) => statsView.setTraffic(pv, vp),
  onFinished: (ok, result, ms) => {
    if (ok) {
      statsView.showResult(ms, result);
      runButton.set("done");
    } else {
      runButton.set("error", "the parties disagree — the proof did not match");
    }
  },
  onFailed: (msg) => runButton.set("error", `proof rejected — ${msg}`),
  onIdleError: (msg) => runButton.set("error", msg),
  onAborted: () => runButton.set("loading"),
  onExports: (e) => inspector.onExports(e),
  onExportsError: (m) => inspector.onExportsError(m),
  remoteMode: () => remoteSession.remoteMode(),
});

inspector = initInspector({
  host: controller.inspectHost,
  onResolved: (m) => panel.onModuleResolved(m),
  onError: (msg) => panel.onInspectError(msg),
});

panel = initGuestPanel({
  els: {
    guestTabs: $("guest-tabs"),
    guestInfoTab: $<HTMLButtonElement>("guest-info-tab"),
    aboutPane: $("guest-about"),
    aboutAnim: $("about-anim"),
    guestBody: $("guest-body"),
    customConfig: $("custom-config"),
    customModeEl: $("custom-mode"),
    ascPane: $("asc-pane"),
    uploadPane: $("upload-pane"),
    dropzone: $("dropzone"),
    wasmFileInput: $<HTMLInputElement>("wasm-file"),
    editorWrap: $("editor-wrap"),
    moduleInfo: $("module-info"),
  },
  editors,
  inspector,
  onTabChange: (tab) => {
    // The info ("about") tab has no program to prove — hide the Prove button.
    document.body.classList.toggle("on-about", tab === "about");
    if (!controller.isRunning()) {
      runButton.set(controller.readyCount() === 2 ? "idle" : "loading");
    }
  },
});

const verifierView = initVerifierView({
  els: {
    controlSection: $("control"),
    guestSection: $("guest-section"),
    runBtn,
    verifierView: $("verifier-view"),
    verifierGate: $("verifier-gate"),
    verifierConnect: $<HTMLButtonElement>("verifier-connect"),
    verifierCancel: $<HTMLButtonElement>("verifier-cancel"),
    verifierWaiting: $("verifier-waiting"),
    verifierSub: $("verifier-sub"),
    verifierRun: $("verifier-run"),
    verifierRunLabel: $("verifier-run-label"),
    verifierTitle: $("verifier-title"),
    verifierDesc: $("verifier-desc"),
    verifierParams: $("verifier-params"),
    verifierModule: $("verifier-module"),
    verifierActions: $("verifier-actions"),
    verifierAccept: $<HTMLButtonElement>("verifier-accept"),
    verifierDecline: $<HTMLButtonElement>("verifier-decline"),
  },
  onConnect: () => remoteSession.confirmJoin(),
  onCancel: () => remoteSession.cancelJoin(),
  onAccept: () => remoteSession.acceptRun(),
  onDecline: () => remoteSession.declineRun(),
});

controlPane = initControlPane({
  els: {
    controlTabs: $("control-tabs"),
    peerControls: $("peer-controls"),
    localExplainer: $("local-explainer"),
    inviteBtn: $<HTMLButtonElement>("invite"),
    invitePanel: $("invite-panel"),
    inviteQr: $<HTMLCanvasElement>("invite-qr"),
    inviteLink: $<HTMLAnchorElement>("invite-link"),
    inviteStatus: $("invite-status"),
    inviteCancelBtn: $<HTMLButtonElement>("invite-cancel"),
    remoteConnected: $("remote-connected"),
    remoteStatusEl: $("remote-status"),
    remoteDisconnectBtn: $<HTMLButtonElement>("remote-disconnect"),
    peerIp: $("peer-ip"),
    peerConn: $("peer-conn"),
    peerPingDot: $("peer-ping-dot"),
    peerPingVal: $("peer-ping-val"),
  },
  featureRemote: FEATURES.remote,
  events: () => remoteSession.events,
  onHostConnected: (link) => remoteSession.setHost(link),
  onModeChanged: () =>
    runButton.set(controller.readyCount() === 2 ? "idle" : "loading"),
  requestDisconnect: (reason) => remoteSession.disconnect(reason),
});

remoteSession = initRemoteSession({
  controller,
  verifierView,
  statsView,
  runButton,
  controlPane,
  version: __PKG_VERSION__,
});

// Tab icons: the mode strip is static; the example tabs were just mounted by the
// guest panel, so both strips are in the DOM now.
decorateTabs($("control-tabs"));
decorateTabs($("guest-tabs"));

initTooltips();

// Load and inspect each example's built-in module (keeps its example script).
for (const ex of EXAMPLES) {
  fetchWasm(ex.wasmUrl)
    .then((wasm) =>
      panel.registerExampleModule(ex.id, {
        name: ex.moduleName,
        wasm,
        builtin: true,
        exports: null,
      }),
    )
    .catch((e) => runButton.set("error", e instanceof Error ? e.message : String(e)));
}

// A `?join=<id>` link: this device joins the prover as the remote verifier.
const joinParam = new URLSearchParams(location.search).get("join");
if (FEATURES.remote && joinParam) remoteSession.bootstrapJoin(joinParam);

// --- the run button ---

runBtn.addEventListener("click", () => {
  if (controller.isRunning()) {
    controller.abort(true);
    return;
  }
  const m = panel.currentModule();
  if (!m) {
    runButton.set(
      "error",
      panel.customMode() === "asc" ? "fix the AssemblyScript first" : "drop a .wasm module first",
    );
    return;
  }
  // Examples derive their inputs from their form; the custom tab supplies its own.
  const { priv, pub } = panel.runInputs();

  const privBytes = Object.values(priv).reduce((n, a) => n + a.length, 0);
  if (privBytes > 128 * 1024) {
    runButton.set("error", "private input too long (max 128 KB)");
    return;
  }
  // The verifier sees private inputs blinded to their length (all zeros).
  const blinded = Object.fromEntries(
    Object.entries(priv).map(([k, v]) => [k, new Uint8Array(v.length)]),
  );

  const script = editors.getScript();
  const proverReq: PartyRequest = {
    type: "run", role: "prover", script, wasm: m.wasm, pub, priv, args: [],
  };
  const verifierReq: PartyRequest = {
    type: "run", role: "verifier", script, wasm: m.wasm, pub, priv: blinded, args: [],
  };

  // Remote verifier: ship the verifier request to the other device and prove
  // against the link instead of a second local worker. For a custom
  // AssemblyScript guest, send the source (not the binary) for the verifier to
  // compile and hash-check itself.
  const rm = remoteSession.remoteMode();
  if (rm.kind === "host") {
    const ascSource = panel.ascSourceIfCustomAsc();
    runButton.set("running", AWAITING_VERIFIER);
    statsView.reset();
    void controller.startRemoteRun(proverReq, verifierReq, panel.currentTab(), ascSource, rm.link);
    return;
  }

  runButton.set("running");
  statsView.reset();
  controller.startLocalRun(proverReq, verifierReq);
});
