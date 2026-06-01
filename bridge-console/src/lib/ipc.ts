// Typed wrapper around the Tauri `daemon_call` command, which is itself
// a thin shim over `mcp_bridged::ipc::call_local`. Every renderer call
// to the daemon goes through here.

import { invoke } from "@tauri-apps/api/core";

/** Mirrors `bridge-console/src-tauri/src/main.rs::DaemonCallResult`. */
export type DaemonCallResult =
  | { kind: "ok"; result: unknown }
  | { kind: "daemon_error"; code: number; message: string }
  | { kind: "transport"; message: string };

/** Method-name constants kept in lockstep with `ipc::method_names`. */
export const Method = {
  DaemonStatus: "daemon.status",
  ServersList: "servers.list",
  ServersDetail: "servers.detail",
  ServersRevoke: "servers.revoke",
  PairInviteStart: "pair.invite_start",
  PairInviteCancel: "pair.invite_cancel",
  IdentityShow: "identity.show",
  IdentityRotate: "identity.rotate",
  LogRecent: "log.recent",
  DiagnosticsBundle: "diagnostics.bundle",
  UpdateCheck: "update.check",
} as const;

export type MethodName = (typeof Method)[keyof typeof Method];

/** Issue one JSON-RPC call to the daemon. Throws on transport failure
 *  (daemon unreachable) and on daemon-returned JSON-RPC errors; pass
 *  through whatever the renderer wants to do with success. */
export async function daemonCall<T = unknown>(
  method: MethodName,
  params?: Record<string, unknown>,
): Promise<T> {
  const r = await invoke<DaemonCallResult>("daemon_call", {
    method,
    params: params ?? null,
  });
  if (r.kind === "ok") {
    return r.result as T;
  }
  if (r.kind === "daemon_error") {
    throw new DaemonError(r.code, r.message);
  }
  throw new TransportError(r.message);
}

export class DaemonError extends Error {
  constructor(
    public code: number,
    message: string,
  ) {
    super(`daemon returned error ${code}: ${message}`);
    this.name = "DaemonError";
  }
}

export class TransportError extends Error {
  constructor(message: string) {
    super(`could not reach daemon: ${message}`);
    this.name = "TransportError";
  }
}

// --- Shape of method responses the Console needs today --------------

export interface ServerListEntry {
  pin_id: string;
  name: string;
  state: "Reachable" | "Unreachable" | "Revoked";
  created_at: number;
  backend_url: string;
  consumers: string[];
  last_activity_at: number | null;
}

export interface DaemonStatus {
  version: string;
  uptime_seconds: number;
  pin_count: number;
  pair_endpoint: string;
}

export interface IdentityInfo {
  pubkey: string;
  display_name: string;
}

/** Shape returned by `pair.invite_start`. Mirrors the SPEC §4.2
 *  Invite JSON; the phone scans this (or its QR rendering). */
export interface PairInvite {
  spec: "mcp-pair/v0.1";
  direction: "resolver_offered" | "origin_offered";
  resolver: {
    pubkey: string;
    display_name: string;
    sas: string;
    lan_addr: string;
  };
  nonce: string;
  uri?: string;
}
