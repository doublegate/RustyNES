// SPDX-License-Identifier: GPL-3.0-or-later
//
// Types for the tests: `env` and `exports` from `cloudflare:workers` carry
// this Worker's bindings and main-module exports.

import type { Env as HostedEnv } from "../src/env";

declare global {
  namespace Cloudflare {
    interface Env extends HostedEnv {}
    interface GlobalProps {
      mainModule: typeof import("../src/index");
      durableNamespaces: "Lobby" | "Budget";
    }
  }
}

export {};
