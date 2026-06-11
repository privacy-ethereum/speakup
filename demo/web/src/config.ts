// Feature flags for parts of the demo that aren't (yet) part of the story
// we definitely want to tell. Flip the defaults here, or override them
// without a rebuild via URL params: ?cheat=1&wat=1
const DEFAULTS = {
  /// "Tamper with a message" button: corrupt one relayed protocol message
  /// and watch the verifier reject the proof.
  cheat: false,
  /// "Custom (WAT)" tab: write your own guest in WebAssembly text format.
  watEditor: false,
};

const params = new URLSearchParams(location.search);
const flag = (name: string, fallback: boolean) => {
  const v = params.get(name);
  if (v === null) return fallback;
  return v === "1" || v === "true";
};

export const FEATURES = {
  cheat: flag("cheat", DEFAULTS.cheat),
  watEditor: flag("wat", DEFAULTS.watEditor),
};
