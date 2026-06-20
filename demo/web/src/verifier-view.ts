// The verifier device's view. When this device joins as the remote verifier the
// prover-authoring UI makes no sense — the prover drives everything — so it's
// swapped for this dedicated pane: a "waiting" state, then the proposal to
// accept/decline, then the description of what's being verified (the statistics
// pane below carries the live progress). This is a pure view; the proposal state
// and the accept/decline logic live in the remote session.

import { EXAMPLES } from "./examples";
import { fmtBytes, sha256Hex } from "./dom";
import { VERIFIER } from "./copy";
import type { PartyRequest } from "./party.worker";
import type { ControlMsg } from "./remote";

type ProposeMsg = Extract<ControlMsg, { kind: "propose" }>;

export interface VerifierView {
  /// Arrived via a join link: take over the UI and show the connect gate
  /// (nothing connects until the user opts in — connecting reveals their IP).
  enterGate(): void;
  /// User opted in: show the "connecting…" state while the link is dialled.
  showConnecting(): void;
  /// The link is open: show the idle "waiting for a run" state.
  connected(): void;
  /// Leave verifier mode (on disconnect / cancel): restore the authoring UI.
  exit(): void;
  /// Back to the idle "waiting" state (after a decline, or a withdrawn run).
  resetToWaiting(): void;
  /// Show an incoming proposal with accept/decline actions.
  showProposal(msg: ProposeMsg): void;
  /// Mark the proposal accepted (hide actions, label "verifying").
  beginAccept(): void;
  /// Set progress text while accepting (compiling, loading…). `module` clears or
  /// sets the module line when provided.
  setStatus(desc: string, module?: string): void;
  /// Fill the pane with what's being verified once a run begins.
  showRunning(tab: string, wasm: Uint8Array, fromSource: boolean): void;
}

export interface VerifierViewEls {
  controlSection: HTMLElement;
  guestSection: HTMLElement;
  runBtn: HTMLButtonElement;
  verifierView: HTMLElement;
  verifierGate: HTMLElement;
  verifierConnect: HTMLButtonElement;
  verifierCancel: HTMLButtonElement;
  verifierWaiting: HTMLElement;
  verifierSub: HTMLElement;
  verifierRun: HTMLElement;
  verifierRunLabel: HTMLElement;
  verifierTitle: HTMLElement;
  verifierDesc: HTMLElement;
  verifierParams: HTMLElement;
  verifierModule: HTMLElement;
  verifierActions: HTMLElement;
  verifierAccept: HTMLButtonElement;
  verifierDecline: HTMLButtonElement;
}

/// A public-input summary the verifier can weigh before accepting.
const fmtPub = (request: PartyRequest): string => {
  if (request.type !== "run") return "(none)";
  const entries = Object.entries(request.pub);
  if (!entries.length) return "(none)";
  return entries
    .map(([k, v]) => (v instanceof Uint8Array ? `${k}: ${v.length} bytes` : `${k}: ${String(v)}`))
    .join(" · ");
};

export const initVerifierView = (opts: {
  els: VerifierViewEls;
  onConnect(): void;
  onCancel(): void;
  onAccept(): void;
  onDecline(): void;
}): VerifierView => {
  const { els } = opts;

  els.verifierConnect.addEventListener("click", opts.onConnect);
  els.verifierCancel.addEventListener("click", opts.onCancel);
  els.verifierAccept.addEventListener("click", opts.onAccept);
  els.verifierDecline.addEventListener("click", opts.onDecline);

  /// Show one of the verifier view's three top-level panels, hiding the rest.
  const showPanel = (panel: "gate" | "waiting" | "run") => {
    els.verifierGate.hidden = panel !== "gate";
    els.verifierWaiting.hidden = panel !== "waiting";
    els.verifierRun.hidden = panel !== "run";
  };

  return {
    enterGate() {
      // The gate stands alone — hide the authoring UI and the control pane
      // (its connection box only makes sense once we're actually connected).
      els.controlSection.hidden = true;
      els.guestSection.hidden = true;
      els.runBtn.hidden = true;
      els.verifierView.hidden = false;
      showPanel("gate");
    },
    showConnecting() {
      els.controlSection.hidden = false; // the connection box now tracks the dial
      els.verifierSub.textContent = VERIFIER.connecting;
      showPanel("waiting");
    },
    connected() {
      els.verifierSub.textContent = VERIFIER.waiting;
      showPanel("waiting");
    },
    exit() {
      els.verifierView.hidden = true;
      els.verifierActions.hidden = true;
      els.controlSection.hidden = false;
      els.guestSection.hidden = false;
      els.runBtn.hidden = false;
    },
    resetToWaiting() {
      els.verifierActions.hidden = true;
      els.verifierSub.textContent = VERIFIER.waiting;
      showPanel("waiting");
    },
    showProposal(msg) {
      els.verifierRunLabel.textContent = "incoming request";
      const ex = EXAMPLES.find((e) => e.id === msg.tab);
      if (ex) {
        els.verifierTitle.textContent = ex.label;
        els.verifierDesc.textContent = ex.description;
        els.verifierModule.textContent = ex.moduleName;
      } else {
        els.verifierTitle.textContent = VERIFIER.customTitle;
        els.verifierDesc.textContent = msg.source ? VERIFIER.proposeSource : VERIFIER.proposeWasm;
        els.verifierModule.textContent = msg.source ? "AssemblyScript source" : "uploaded WASM module";
      }
      els.verifierParams.textContent = fmtPub(msg.request);
      els.verifierActions.hidden = false;
      showPanel("run");
    },
    beginAccept() {
      els.verifierActions.hidden = true;
      els.verifierRunLabel.textContent = "verifying";
    },
    setStatus(desc, module) {
      els.verifierDesc.textContent = desc;
      if (module !== undefined) els.verifierModule.textContent = module;
    },
    showRunning(tab, wasm, fromSource) {
      showPanel("run");
      const ex = EXAMPLES.find((e) => e.id === tab);
      if (ex) {
        els.verifierTitle.textContent = ex.label;
        els.verifierDesc.textContent = ex.description;
        els.verifierModule.textContent = ex.moduleName;
        return;
      }
      els.verifierTitle.textContent = VERIFIER.customTitle;
      els.verifierDesc.textContent = fromSource ? VERIFIER.runningSource : VERIFIER.runningWasm;
      void sha256Hex(wasm).then((h) => {
        els.verifierModule.textContent = `module · ${fmtBytes(wasm.length)} · sha256 ${h.slice(0, 8)}…`;
      });
    },
  };
};
