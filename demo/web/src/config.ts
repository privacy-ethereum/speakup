// Feature flags for parts of the demo that aren't (yet) part of the story we
// definitely want to tell. Flip the defaults here, or override them without a
// rebuild via URL params: ?remote=0
const DEFAULTS = {
  /// "Verify from another device": run the verifier on a second device over a
  /// WebRTC DataChannel, joined by scanning a QR code.
  remote: true,
};

const params = new URLSearchParams(location.search);
const flag = (name: string, fallback: boolean) => {
  const v = params.get(name);
  if (v === null) return fallback;
  return v === "1" || v === "true";
};

export const FEATURES = {
  remote: flag("remote", DEFAULTS.remote),
};
