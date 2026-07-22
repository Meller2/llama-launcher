// Launch profiles — user-facing presets on top of AutoConfig / settings defaults.
// Beginner/intermediate pick a profile; expert may still use raw defaults.

import type { AutoConfig, LaunchDefaults } from "$lib/api";

export type LaunchProfileId =
  | "balanced"
  | "speed"
  | "quality"
  | "long_ctx"
  | "cpu";

export interface LaunchParams {
  ctx: number;
  kv_quant: string;
  threads: number;
  ngl: number;
}

export const LAUNCH_PROFILES: {
  id: LaunchProfileId;
  labelKey: string;
  descKey: string;
}[] = [
  {
    id: "balanced",
    labelKey: "prof.balanced",
    descKey: "prof.balanced.desc",
  },
  {
    id: "speed",
    labelKey: "prof.speed",
    descKey: "prof.speed.desc",
  },
  {
    id: "quality",
    labelKey: "prof.quality",
    descKey: "prof.quality.desc",
  },
  {
    id: "long_ctx",
    labelKey: "prof.long_ctx",
    descKey: "prof.long_ctx.desc",
  },
  {
    id: "cpu",
    labelKey: "prof.cpu",
    descKey: "prof.cpu.desc",
  },
];

const CTX_SPEED_CAP = 4096;
const CTX_CPU_CAP = 8192;
const CTX_LONG_MIN = 16384;
const CTX_LONG_CAP = 32768;

function baseParams(
  auto: AutoConfig | null,
  defaults: LaunchDefaults,
): LaunchParams {
  if (auto) {
    return {
      ctx: auto.ctx,
      kv_quant: auto.kv_quant,
      threads: auto.threads,
      ngl: auto.ngl,
    };
  }
  return {
    ctx: defaults.ctx,
    kv_quant: defaults.kv_quant,
    threads: defaults.threads,
    ngl: defaults.ngl,
  };
}

function bumpKvQuality(kv: string): string {
  if (kv === "q4_0") return "q8_0";
  if (kv === "q8_0") return "f16";
  return kv;
}

/**
 * Apply a named profile to hardware auto-config (or settings defaults).
 * Profiles only reshape known-safe knobs; they do not re-run autoconfig.
 */
export function applyLaunchProfile(
  id: LaunchProfileId,
  auto: AutoConfig | null,
  defaults: LaunchDefaults,
): LaunchParams {
  const b = baseParams(auto, defaults);

  switch (id) {
    case "balanced":
      return b;

    case "speed":
      return {
        ...b,
        ctx: Math.min(b.ctx, CTX_SPEED_CAP),
        kv_quant: "q4_0",
      };

    case "quality":
      return {
        ...b,
        kv_quant: bumpKvQuality(b.kv_quant),
      };

    case "long_ctx":
      return {
        ...b,
        ctx: Math.min(Math.max(b.ctx, CTX_LONG_MIN), CTX_LONG_CAP),
        kv_quant: "q4_0",
      };

    case "cpu":
      return {
        ...b,
        ngl: 0,
        ctx: Math.min(b.ctx, CTX_CPU_CAP),
        kv_quant: "q4_0",
      };

    default:
      return b;
  }
}

export function isLaunchProfileId(s: string): s is LaunchProfileId {
  return LAUNCH_PROFILES.some((p) => p.id === s);
}
