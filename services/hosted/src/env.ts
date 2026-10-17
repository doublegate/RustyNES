// SPDX-License-Identifier: GPL-3.0-or-later
//
// The Worker's bindings, mirroring wrangler.jsonc. Every var is a string (that
// is how Workers delivers them); `limits.ts`'s `intVar` parses and clamps the
// numeric ones so a typo in the dashboard degrades to the default rather than
// to "no limit".

import type { Budget } from "./budget";
import type { Lobby } from "./lobby";

export interface Env {
  LOBBY: DurableObjectNamespace<Lobby>;
  BUDGET: DurableObjectNamespace<Budget>;

  ALLOWED_ORIGINS?: string;
  ALLOW_LOCALHOST?: string;

  TURN_ENABLED?: string;
  TURN_TTL_SECONDS?: string;
  TURN_DAILY_CAP?: string;
  TURN_PER_IP_PER_HOUR?: string;
  /** Secret: the Realtime TURN key id. */
  TURN_KEY_ID?: string;
  /** Secret: the Realtime TURN key's API token. */
  TURN_KEY_API_TOKEN?: string;

  LOBBY_MAX_CONNECTIONS?: string;
  LOBBY_MAX_CONNECTIONS_PER_IP?: string;
  LOBBY_CONNECTS_PER_IP_PER_MINUTE?: string;
  LOBBY_ROOMS_PER_IP_PER_MINUTE?: string;

  RA_ENABLED?: string;
  RA_UPSTREAM?: string;
  RA_USER_AGENT?: string;
}

/** Internal header carrying the client IP from the Worker to a Durable Object. */
export const CLIENT_IP_HEADER = "x-rustynes-client-ip";
