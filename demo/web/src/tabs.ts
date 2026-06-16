// A small, reusable tab strip. It owns behaviour only — selection state, ARIA
// roles, click and arrow-key navigation; the seam between the strip and its
// panel is pure CSS (see `.tabs` in style.css). Mark the initially-active
// button with `class="active"` and give each a `data-tab`; pass `onChange` to
// react to selection. The returned `select` switches tabs programmatically.

export interface Tabs {
  select(tab: string): void;
}

export function initTabs(strip: HTMLElement, onChange: (tab: string) => void): Tabs {
  const buttons = [...strip.querySelectorAll<HTMLButtonElement>("[data-tab]")];

  const sync = (tab: string) => {
    for (const b of buttons) {
      const active = b.dataset.tab === tab;
      b.classList.toggle("active", active);
      b.setAttribute("aria-selected", String(active));
      b.tabIndex = active ? 0 : -1;
    }
  };

  const select = (tab: string) => {
    sync(tab);
    onChange(tab);
  };

  strip.setAttribute("role", "tablist");
  for (const b of buttons) b.setAttribute("role", "tab");
  // Adopt the initial selection from markup (the `.active` button, else first),
  // without firing onChange — the page sets up its own initial state.
  const initial = buttons.find((b) => b.classList.contains("active")) ?? buttons[0];
  if (initial?.dataset.tab) sync(initial.dataset.tab);

  strip.addEventListener("click", (ev) => {
    const b = (ev.target as HTMLElement).closest<HTMLButtonElement>("[data-tab]");
    if (b?.dataset.tab) select(b.dataset.tab);
  });

  strip.addEventListener("keydown", (ev) => {
    if (ev.key !== "ArrowRight" && ev.key !== "ArrowLeft") return;
    const i = buttons.findIndex((b) => b.classList.contains("active"));
    if (i < 0) return;
    ev.preventDefault();
    const step = ev.key === "ArrowRight" ? 1 : -1;
    const next = buttons[(i + step + buttons.length) % buttons.length];
    if (next.dataset.tab) {
      select(next.dataset.tab);
      next.focus();
    }
  });

  return { select };
}
