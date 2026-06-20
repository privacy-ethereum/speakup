// Minimalist line icons (16×16, `currentColor` so they track the tab's text
// colour and the "soon" dimming) for the tab strips, keyed by each button's
// `data-tab` / `data-mode` value. `decorateTabs` prepends the matching icon to
// every tab button in a strip — one place for both the static mode tabs and the
// dynamically built example tabs.

const ICONS: Record<string, string> = {
  // local — a monitor: prove and verify on this machine
  local: `<svg viewBox="0 0 16 16" aria-hidden="true" focusable="false"><rect x="2" y="3" width="12" height="8" rx="1.2" fill="none" stroke="currentColor" stroke-width="1.2"/><path d="M6 13.5h4M8 11v2.5" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/></svg>`,
  // peer — two linked nodes: a second device over the network
  peer: `<svg viewBox="0 0 16 16" aria-hidden="true" focusable="false"><circle cx="4" cy="4" r="2.1" fill="none" stroke="currentColor" stroke-width="1.2"/><circle cx="12" cy="12" r="2.1" fill="none" stroke="currentColor" stroke-width="1.2"/><path d="M5.6 5.6l4.8 4.8" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/></svg>`,
  // attest — a shield with a check: a notary's attestation
  attest: `<svg viewBox="0 0 16 16" aria-hidden="true" focusable="false"><path d="M8 1.7l5 1.9v3.9c0 3-2.1 4.9-5 5.8-2.9-.9-5-2.8-5-5.8V3.6z" fill="none" stroke="currentColor" stroke-width="1.1" stroke-linejoin="round"/><path d="M5.7 8l1.6 1.6L10.5 6.4" fill="none" stroke="currentColor" stroke-width="1.1" stroke-linecap="round" stroke-linejoin="round"/></svg>`,
  // sha-256 — a hash mark
  sha256: `<svg viewBox="0 0 16 16" aria-hidden="true" focusable="false"><path d="M6.3 2.6L4.4 13.4M11.6 2.6L9.7 13.4M2.8 5.9h11M2.2 10.1h11" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/></svg>`,
  // sudoku — a 3×3 grid
  sudoku: `<svg viewBox="0 0 16 16" aria-hidden="true" focusable="false"><rect x="2.5" y="2.5" width="11" height="11" rx="1" fill="none" stroke="currentColor" stroke-width="1.1"/><path d="M6.17 2.5v11M9.83 2.5v11M2.5 6.17h11M2.5 9.83h11" fill="none" stroke="currentColor" stroke-width="1"/></svg>`,
  // custom — code brackets: write your own
  custom: `<svg viewBox="0 0 16 16" aria-hidden="true" focusable="false"><path d="M5.5 4L2 8l3.5 4M10.5 4L14 8l-3.5 4" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/></svg>`,
};

/// Prepend the matching icon to each tab button in `strip` (by `data-tab` or
/// `data-mode`). Buttons without a known key are left untouched.
export const decorateTabs = (strip: HTMLElement): void => {
  for (const btn of strip.querySelectorAll<HTMLButtonElement>("[data-tab],[data-mode]")) {
    const key = btn.dataset.tab ?? btn.dataset.mode;
    const svg = key ? ICONS[key] : undefined;
    if (svg) btn.insertAdjacentHTML("afterbegin", svg);
  }
};
