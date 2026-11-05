import { readFileSync } from "node:fs";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// The authenticated local stack serves the panel over HTTPS so the session cookie and passkey origin match production.
const certificate = process.env.MIDAS_DEV_TLS_CERT;
const key = process.env.MIDAS_DEV_TLS_KEY;
const api = process.env.MIDAS_DEV_API ?? "http://127.0.0.1:7443";
const proxy = { target: api, secure: false, ws: true };

export default defineConfig({
  plugins: [react()],
  server: {
    host: certificate ? "localhost" : "127.0.0.1",
    port: 5173,
    strictPort: true,
    https:
      certificate && key
        ? { cert: readFileSync(certificate), key: readFileSync(key) }
        : undefined,
    proxy: { "/health": proxy, "/api": proxy },
  },
});
