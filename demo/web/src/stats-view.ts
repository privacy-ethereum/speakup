// The statistics + result pane as a view. Owns no state: the run controller
// pushes traffic and the final result in.

import { fmtBytes, fmtMs } from "./dom";

export interface StatsView {
  /// Reveal the pane and zero it for a fresh run (hides the previous result).
  reset(): void;
  setTraffic(pvBytes: number, vpBytes: number): void;
  showResult(elapsedMs: number, result: string): void;
}

export interface StatsEls {
  statsPane: HTMLElement;
  statPv: HTMLElement;
  statVp: HTMLElement;
  statTime: HTMLElement;
  resultBox: HTMLElement;
  resultEl: HTMLElement;
}

export const initStatsView = (els: StatsEls): StatsView => ({
  reset() {
    els.statsPane.hidden = false;
    els.resultBox.hidden = true;
    els.statTime.textContent = "—";
    els.statPv.textContent = "0 B";
    els.statVp.textContent = "0 B";
  },
  setTraffic(pvBytes, vpBytes) {
    els.statPv.textContent = fmtBytes(pvBytes);
    els.statVp.textContent = fmtBytes(vpBytes);
  },
  showResult(elapsedMs, result) {
    els.statTime.textContent = fmtMs(elapsedMs);
    els.resultEl.textContent = result;
    els.resultBox.hidden = false;
  },
});
