// SPDX-License-Identifier: GPL-3.0-or-later
//
// The tests run inside workerd (the Workers runtime) through the Cloudflare
// Vitest plugin, against the real wrangler.jsonc bindings: the Durable
// Objects, hibernatable WebSockets and storage are the runtime's own, not
// mocks. Only outbound `fetch` (to Cloudflare's TURN API and to
// retroachievements.org) is replaced, per test, so nothing leaves the machine.

import { cloudflareTest } from "@cloudflare/vitest-plugin";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [
    cloudflareTest({
      wrangler: { configPath: "./wrangler.jsonc" },
      miniflare: {
        bindings: {
          // Test-only secrets: placeholders, never real values.
          TURN_KEY_ID: "test-key-id",
          TURN_KEY_API_TOKEN: "test-api-token",
          // Small spend-guard limits so the tests can reach them; the deployed
          // values are the ones in wrangler.jsonc.
          TURN_DAILY_CAP: "5",
          TURN_PER_IP_PER_HOUR: "3",
          RA_PER_IP_PER_HOUR: "10",
          RA_LOGINS_PER_IP_PER_HOUR: "3",
          RA_DAILY_CAP: "15",
          // The deployed proxy is off until RetroAchievements answers the
          // integration request (wrangler.jsonc, v3.4.1); the proxy itself is
          // still under test, so the tests switch it on. ra.test.ts's
          // kill-switch case sets "false" itself.
          RA_ENABLED: "true",
        },
      },
    }),
  ],
});
