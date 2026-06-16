// A tiny shared tooltip. Markup stays declarative: add `data-tip="..."` to any
// element (and make it focusable for keyboard users), then call `initTooltips`
// once. A single bubble is reused by every trigger and positioned with JS, so
// it floats above the panels instead of clipping inside them.

let bubble: HTMLDivElement | null = null;

function ensureBubble(): HTMLDivElement {
  if (!bubble) {
    bubble = document.createElement("div");
    bubble.className = "tooltip";
    bubble.setAttribute("role", "tooltip");
    document.body.appendChild(bubble);
  }
  return bubble;
}

function show(trigger: HTMLElement) {
  const tip = trigger.dataset.tip;
  if (!tip) return;
  const el = ensureBubble();
  el.textContent = tip;
  el.classList.add("visible");

  // Centre over the trigger, clamped to the viewport; flip below if the bubble
  // would spill off the top. getBoundingClientRect is viewport-relative, so the
  // bubble is positioned with `fixed`.
  const t = trigger.getBoundingClientRect();
  const b = el.getBoundingClientRect();
  const margin = 8;
  const left = Math.min(
    Math.max(margin, t.left + t.width / 2 - b.width / 2),
    window.innerWidth - b.width - margin,
  );
  const above = t.top - b.height - margin;
  el.style.left = `${left}px`;
  el.style.top = `${above < margin ? t.bottom + margin : above}px`;
}

function hide() {
  bubble?.classList.remove("visible");
}

/** Wire up every `[data-tip]` element under `root` to the shared tooltip. */
export function initTooltips(root: ParentNode = document) {
  for (const trigger of root.querySelectorAll<HTMLElement>("[data-tip]")) {
    trigger.addEventListener("mouseenter", () => show(trigger));
    trigger.addEventListener("focus", () => show(trigger));
    trigger.addEventListener("mouseleave", hide);
    trigger.addEventListener("blur", hide);
  }
}
