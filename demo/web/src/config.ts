// Feature flags for parts of the demo that aren't (yet) part of the story
// we definitely want to tell. Flip the defaults here, or try them without a
// rebuild via URL params: ?slow=1&cheat=1&wat=1
const DEFAULTS = {
  /// Relay-speed slider + step-through-messages mode.
  slowMotion: false,
  /// "Tamper with a message" button: corrupt one relayed protocol message
  /// and watch the verifier reject the proof.
  cheat: false,
  /// "Custom (WAT)" tab: write your own guest in WebAssembly text format.
  watEditor: false,
};

const params = new URLSearchParams(location.search);
const on = (v: string | null) => v === "1" || v === "true";

export const FEATURES = {
  slowMotion: DEFAULTS.slowMotion || on(params.get("slow")),
  cheat: DEFAULTS.cheat || on(params.get("cheat")),
  watEditor: DEFAULTS.watEditor || on(params.get("wat")),
};
