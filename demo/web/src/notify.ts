// Tab notifications for peer mode: a short chime plus a visual cue on the
// browser tab — a flashing title and a badged favicon — so the user notices a
// peer connecting, or an incoming request, while this tab is backgrounded. The
// sound always plays; the tab cue only shows when the tab is hidden (when it's
// visible the UI change is already on screen).

// --- sound ---

let audioCtx: AudioContext | null = null;

const ctx = (): AudioContext | null => {
  if (!audioCtx) {
    const AC = window.AudioContext ?? (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
    if (!AC) return null;
    audioCtx = new AC();
  }
  return audioCtx;
};

// Browsers block audio until the page has seen a user gesture; unlock on the
// first one (peer mode always involves a click — invite / connect — before any
// notification fires, so the context is running by the time we chime).
const unlock = () => {
  const ac = ctx();
  if (ac && ac.state === "suspended") void ac.resume();
};
window.addEventListener("pointerdown", unlock, { once: true });
window.addEventListener("keydown", unlock, { once: true });

/// Play a short two-note chime. `up` rises (a welcoming "connected"); otherwise
/// it's a steadier double note for an incoming request.
const chime = (up: boolean) => {
  const ac = ctx();
  if (!ac) return;
  if (ac.state === "suspended") void ac.resume();
  const start = ac.currentTime + 0.01;
  const notes = up ? [659.25, 987.77] : [880, 880]; // E5→B5, or A5·A5
  notes.forEach((freq, i) => {
    const t = start + i * 0.12;
    const osc = ac.createOscillator();
    const gain = ac.createGain();
    osc.type = "sine";
    osc.frequency.value = freq;
    gain.gain.setValueAtTime(0, t);
    gain.gain.linearRampToValueAtTime(0.15, t + 0.01);
    gain.gain.exponentialRampToValueAtTime(0.0001, t + 0.18);
    osc.connect(gain).connect(ac.destination);
    osc.start(t);
    osc.stop(t + 0.2);
  });
};

// --- tab cue (title + favicon badge) ---

const originalTitle = document.title;

const faviconLink = (): HTMLLinkElement | null =>
  document.querySelector<HTMLLinkElement>('link[rel="icon"][type="image/svg+xml"]');

/// The favicon's href captured before swapping, so the original can be restored.
let originalIconHref: string | null = null;

/// Swap the favicon for the red-lined alert variant (same path, alert mark).
const setAlertFavicon = () => {
  const link = faviconLink();
  if (!link) return;
  if (originalIconHref === null) originalIconHref = link.getAttribute("href");
  link.setAttribute(
    "href",
    (originalIconHref ?? link.href).replace("favicon.svg", "favicon-alert.svg"),
  );
};

const restoreFavicon = () => {
  if (originalIconHref === null) return;
  const link = faviconLink();
  if (link) link.setAttribute("href", originalIconHref);
};

/// Clear any active tab cue and restore the title and favicon.
const stopAlert = () => {
  document.title = originalTitle;
  restoreFavicon();
};

document.addEventListener("visibilitychange", () => {
  if (!document.hidden) stopAlert();
});
window.addEventListener("focus", stopAlert);

const alert = () => {
  // Only worth a tab cue when the tab is in the background; if it's visible the
  // user already sees the change. The title gets a "! " prefix (set once, no
  // flashing) and the favicon turns red; both clear when the tab regains focus.
  if (document.hidden) {
    document.title = `! ${originalTitle}`;
    setAlertFavicon();
  }
};

// --- public events ---

/// A peer connected to our invite.
export const notifyPeerConnected = () => {
  chime(true);
  alert();
};

/// The prover sent a request to prove a program.
export const notifyProverRequest = () => {
  chime(false);
  alert();
};
