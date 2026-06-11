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

export default defineConfig({
  // Served at https://<org>.github.io/speakup-demo/
  base: "/speakup-demo/",
  plugins: [coiServiceworker()],
  // The guest sources are ?raw-imported from ../guests for the
  // "view full source" modal.
  server: { fs: { allow: [".."] }, headers: coiHeaders },
  preview: { headers: coiHeaders },
});
