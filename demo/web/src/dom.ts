// Shared DOM lookup and formatting primitives. This module imports nothing from
// the app, so any component can use it without risking an import cycle.

/// Typed `document.getElementById`.
export const $ = <T extends HTMLElement>(id: string): T =>
  document.getElementById(id) as T;

export const fmtBytes = (n: number): string =>
  n < 1024
    ? `${n} B`
    : n < 1 << 20
      ? `${(n / 1024).toFixed(1)} KB`
      : `${(n / (1 << 20)).toFixed(1)} MB`;

export const fmtMs = (ms: number): string =>
  ms < 1000 ? `${ms.toFixed(0)} ms` : `${(ms / 1000).toFixed(2)} s`;

/// Lowercase hex SHA-256 of some bytes.
export const sha256Hex = async (bytes: Uint8Array): Promise<string> => {
  const digest = await crypto.subtle.digest("SHA-256", bytes as BufferSource);
  return [...new Uint8Array(digest)]
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
};
