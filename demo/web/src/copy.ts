// Central UI copy: user-facing strings that appear in more than one place live
// here so the wording stays in sync. One-off labels stay inline at their site.

/// The "view script" disclosure toggle each example/run carries (mounted in
/// examples.ts, flipped in guest-panel.ts).
export const SCRIPT_TOGGLE = {
  show: "view script ▸",
  hide: "hide script ▾",
} as const;

/// Prover-side run-button detail while the remote verifier decides on a run.
export const AWAITING_VERIFIER = "waiting for the verifier to accept…";

/// The verifier device view (waiting/connecting states and the custom-module
/// descriptions shown for a proposal vs. an in-progress run).
export const VERIFIER = {
  waiting: "Waiting for the prover to initiate a proof…",
  connecting: "Connecting to the prover's device…",
  customTitle: "custom module",
  proposeSource:
    "A custom AssemblyScript guest. On accept, the verifier compiles the source itself and checks it against the prover's binary.",
  proposeWasm: "A custom guest module supplied on the prover's device.",
  runningSource:
    "Compiled from the prover's AssemblyScript source on this device — the binary hash matches, so the verifier runs only code it can read.",
  runningWasm:
    "A guest module supplied on the prover's device. Only its public output is revealed; the prover's input never leaves their device.",
} as const;
