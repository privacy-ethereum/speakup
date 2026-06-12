import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { defineConfig, type Plugin } from "vite";

// SharedArrayBuffer (threaded wasm) needs cross-origin isolation. The dev and
// preview servers send the headers; on GitHub Pages (which can't set headers)
// the coi-serviceworker shim injects them instead.
const coiHeaders = {
  "Cross-Origin-Opener-Policy": "same-origin",
  "Cross-Origin-Embedder-Policy": "require-corp",
};

// coi-serviceworker must be a standalone same-origin file (it registers
// itself as a service worker), so it can't go through the bundle: serve it
// from node_modules in dev, copy it into dist root at build.
const coiPath = createRequire(import.meta.url).resolve(
  "coi-serviceworker/coi-serviceworker.min.js",
);
const coiServiceworker = (): Plugin => ({
  name: "coi-serviceworker",
  configureServer(server) {
    server.middlewares.use((req, res, next) => {
      if (req.url?.split("?")[0].endsWith("/coi-serviceworker.min.js")) {
        res.setHeader("Content-Type", "text/javascript");
        res.end(readFileSync(coiPath));
      } else next();
    });
  },
  generateBundle() {
    this.emitFile({
      type: "asset",
      fileName: "coi-serviceworker.min.js",
      source: readFileSync(coiPath, "utf8"),
    });
  },
});

// Content hash of the wasm pkg, baked into the bundle as __PKG_VERSION__.
// The pkg files keep stable names (public/ assets are copied verbatim, not
// hashed like the app bundles), and GitHub Pages serves everything with
// max-age=600 — so after a deploy a warm browser would pair the NEW page
// code with the PREVIOUS deploy's glue/wasm ("… is not a function"). The
// worker appends ?v=<version> to both pkg fetches, busting the cache
// exactly when the pkg actually changed.
const pkgVersion = (() => {
  try {
    const h = createHash("sha256");
    for (const f of ["zkvm_demo.js", "zkvm_demo_bg.wasm"]) {
      h.update(readFileSync(new URL(`./public/pkg/${f}`, import.meta.url)));
    }
    return h.digest("hex").slice(0, 8);
  } catch {
    return "dev"; // pkg not built yet (e.g. type-check on a fresh checkout)
  }
})();

export default defineConfig({
  // Served at https://<org>.github.io/speakup-demo/
  base: "/speakup-demo/",
  define: { __PKG_VERSION__: JSON.stringify(pkgVersion) },
  plugins: [coiServiceworker()],
  // The guest sources are ?raw-imported from ../guests for the
  // "view full source" modal.
  server: { fs: { allow: [".."] }, headers: coiHeaders },
  preview: { headers: coiHeaders },
});
