import { defineConfig } from "vite";

export default defineConfig({
  // Served at https://<org>.github.io/speakup-demo/
  base: "/speakup-demo/",
  // The guest sources are ?raw-imported from ../guests for the
  // "view full source" modal.
  server: { fs: { allow: [".."] } },
});
