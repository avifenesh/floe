import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "node:path";
export default defineConfig({
    plugins: [react()],
    resolve: {
        alias: {
            "@": path.resolve(__dirname, "./src"),
        },
    },
    server: {
        port: 5173,
        // Narrow proxy: only paths vite can reliably forward from its
        // dev server before the SPA fallback intervenes. Everything else
        // goes cross-origin to the backend on :8787; session auth relies
        // on SameSite=None+Secure cookies (localhost is a secure context
        // per browser spec, so http://127.0.0.1 is accepted).
        proxy: {
            "/analyze": "http://127.0.0.1:8787",
            "/health": "http://127.0.0.1:8787",
        },
    },
});
