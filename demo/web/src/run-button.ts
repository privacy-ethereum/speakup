// The Prove button as a tiny view: all run feedback lives on the button itself —
// a spinner while loading or proving, a green ✓ on success, a red ✗ (message on
// hover) on failure. The one piece of outside state it needs — whether peer mode
// is selected but no verifier is connected yet — is injected as a predicate so
// the button stays decoupled from the mode/remote machinery.

export type RunButtonState = "loading" | "idle" | "running" | "done" | "error";

export interface RunButton {
  /// `detail` is the hover title for the error state (and an optional override
  /// for the running state, e.g. the AWAITING_VERIFIER note in copy.ts).
  set(state: RunButtonState, detail?: string): void;
}

const SPINNER = '<span class="spinner" aria-hidden="true"></span>';

export const initRunButton = (
  btn: HTMLButtonElement,
  opts: { isPeerWaiting(): boolean },
): RunButton => {
  const set = (state: RunButtonState, detail = "") => {
    btn.disabled = state === "loading";
    btn.classList.toggle("done", state === "done");
    btn.classList.toggle("error", state === "error");
    if (state === "loading" || state === "running") {
      btn.innerHTML = SPINNER;
      btn.title = state === "running" ? detail || "click to abort" : "loading…";
    } else if (state === "idle" && opts.isPeerWaiting()) {
      // Peer mode needs a connected verifier before there's anything to prove to.
      btn.disabled = true;
      btn.textContent = "Prove";
      btn.title = "invite a verifier to prove to";
    } else {
      btn.textContent = state === "done" ? "✓" : state === "error" ? "✗" : "Prove";
      btn.title = state === "error" ? detail : "";
    }
  };
  return { set };
};
