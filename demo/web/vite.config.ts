import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { defineConfig, type Plugin } from "vite";
import { PeerServer } from "peer";
import stun from "stun";

// Peer mode needs two things from a server, and both run locally beside the dev
// server so development depends on no external service (the client points at
// them in dev — see src/remote.ts):
//
//   • signaling — a PeerJS broker that introduces the two peers and relays
//     their SDP/ICE; it never sees the connection itself.
//   • NAT traversal — a STUN server that tells a peer its own reachable
//     address, so it can offer a candidate the other side can connect to.
//
// No TURN relay: same-machine / same-LAN peers connect directly on host
// candidates. (On Firefox that needs `media.peerconnection.ice.obfuscate_host_
// addresses = false` in about:config, or a running mDNS responder — otherwise
// host candidates are masked as unresolvable `.local` names.)
//
// Both are dev-only. The broker runs standalone rather than mounted on Vite's
// HTTP server, to stay clear of the HMR WebSocket upgrade handling.
const DEV_PEER_PORT = 9000; // keep in sync with src/remote.ts
const DEV_STUN_PORT = 3478; // keep in sync with src/remote.ts

const peerBroker = (): Plugin => ({
  name: "peer-broker",
  apply: "serve",
  configureServer(vite) {
    PeerServer({ port: DEV_PEER_PORT, path: "/" }, (server) => {
      vite.config.logger.info(`  ➜  PeerJS broker:  ws://localhost:${DEV_PEER_PORT}/`);
      // Drop the broker when the dev server stops (e.g. a config-change restart),
      // so the port is free for the next start.
      vite.httpServer?.once("close", () => server.close());
    });
  },
});

const stunServer = (): Plugin => ({
  name: "stun-server",
  apply: "serve",
  configureServer(vite) {
    const srv = stun.createServer({ type: "udp4" });
    // Reply to each binding request with the source address we saw, so the peer
    // learns its own reflexive candidate. That's all browsers need at gathering
    // time; the peer-to-peer connectivity checks they run themselves.
    srv.on("bindingRequest", (req, rinfo) => {
      const msg = stun.createMessage(stun.constants.STUN_BINDING_RESPONSE);
      msg.setTransactionId(req.transactionId);
      msg.addXorAddress(rinfo.address, rinfo.port);
      srv.send(msg, rinfo.port, rinfo.address);
    });
    srv.listen(DEV_STUN_PORT, () =>
      vite.config.logger.info(`  ➜  STUN server:    udp://localhost:${DEV_STUN_PORT}/`),
    );
    vite.httpServer?.once("close", () => srv.close());
  },
});

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
    for (const f of ["speakup_wasm.js", "speakup_wasm_bg.wasm"]) {
      h.update(readFileSync(new URL(`./public/pkg/${f}`, import.meta.url)));
    }
    return h.digest("hex").slice(0, 8);
  } catch {
    return "dev"; // pkg not built yet (e.g. type-check on a fresh checkout)
  }
})();

export default defineConfig({
  // Served at https://privacy-ethereum.github.io/speakup/demo/ (the docs
  // site owns the root; the Pages workflow nests the demo under /demo/).
  base: "/speakup/demo/",
  define: { __PKG_VERSION__: JSON.stringify(pkgVersion) },
  plugins: [coiServiceworker(), peerBroker(), stunServer()],
  server: { headers: coiHeaders },
  preview: { headers: coiHeaders },
  // The AssemblyScript compiler (lazy-loaded for the custom tab) uses top-level
  // await; the threaded-wasm app already targets modern browsers, so build for
  // esnext (covers both the production build and dev pre-bundling).
  build: { target: "esnext" },
  optimizeDeps: { esbuildOptions: { target: "esnext" } },
});
