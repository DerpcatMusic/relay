import type { RelayWebSessionOptions, RelayWebSessionState } from "./types.js";

/** Framework-independent placeholder for the future Relay WebRTC session. */
export class RelayWebSession {
  readonly state: RelayWebSessionState = "idle";

  constructor(readonly options: RelayWebSessionOptions = {}) {}
}
