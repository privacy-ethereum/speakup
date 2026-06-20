// A tiny shared tooltip. Markup stays declarative: add `data-tip="..."` (plain
// text) or `data-tip-html="..."` (rich content, e.g. a link) to any element —
// make it focusable for keyboard users — then call `initTooltips` once. A single
// bubble is reused by every trigger and positioned with JS, so it floats above
// the panels instead of clipping inside them. The bubble stays open while the
// pointer is over it (with a short hide delay), so links inside it are clickable.

let bubble: HTMLDivElement | null = null;
let hideTimer: number | undefined;

function cancelHide() {
  if (hideTimer !== undefined) {
    window.clearTimeout(hideTimer);
    hideTimer = undefined;
  }
}

function scheduleHide() {
  cancelHide();
  hideTimer = window.setTimeout(() => bubble?.classList.remove("visible"), 150);
}

function ensureBubble(): HTMLDivElement {
  if (!bubble) {
    bubble = document.createElement("div");
    bubble.className = "tooltip";
    bubble.setAttribute("role", "tooltip");
    // Keep it open while the pointer is over the bubble itself, so a link inside
    // can be reached and clicked.
    bubble.addEventListener("mouseenter", cancelHide);
    bubble.addEventListener("mouseleave", scheduleHide);
    document.body.appendChild(bubble);
  }
  return bubble;
}

function show(trigger: HTMLElement) {
  const html = trigger.dataset.tipHtml;
  const text = trigger.dataset.tip;
  if (html === undefined && text === undefined) return;
  cancelHide();
  const el = ensureBubble();
  if (html !== undefined) el.innerHTML = html;
  else el.textContent = text!;
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

/** Wire up every `[data-tip]` / `[data-tip-html]` element under `root`. */
export function initTooltips(root: ParentNode = document) {
  for (const trigger of root.querySelectorAll<HTMLElement>("[data-tip],[data-tip-html]")) {
    trigger.addEventListener("mouseenter", () => show(trigger));
    trigger.addEventListener("focus", () => show(trigger));
    trigger.addEventListener("mouseleave", scheduleHide);
    trigger.addEventListener("blur", scheduleHide);
  }
}
