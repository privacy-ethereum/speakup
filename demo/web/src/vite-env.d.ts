/// <reference types="vite/client" />

/// Content hash of the wasm pkg, injected at build time (vite.config.ts):
/// cache-busts the pkg's stable-named files across deploys.
declare const __PKG_VERSION__: string;
