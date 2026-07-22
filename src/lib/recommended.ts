// Curated recommended models — manual list (refresh periodically).
// GGUF: unsloth; files verified on HF. Novice picks by purpose, not by repo name.

export type RecCategory = "light" | "chat" | "smart" | "code" | "power";

export interface RecommendedModel {
  id: string;
  /** i18n title key */
  titleKey: string;
  /** i18n short description key */
  blurbKey: string;
  category: RecCategory;
  /** Display family: «Qwen 3.6», «Gemma 4» */
  family: string;
  /** Size label: «27B», «12B» */
  sizeLabel: string;
  hfRepo: string;
  /** Concrete .gguf at repo root */
  file: string;
  /** File size (bytes) for UI and disk checks */
  fileBytes: number;
  /** Comfortable VRAM estimate for this quant (bytes) */
  vramHintBytes: number;
  /** Editorial highlight (still re-ranked by hardware fit) */
  featured?: boolean;
}

const GB = 1024 ** 3;

/**
 * Mid-2026 slice: Qwen 3.6 + Gemma 4 + light Phi/Qwen3.5.
 * Avoid promoting outdated lines as primary picks.
 */
export const RECOMMENDED_MODELS: RecommendedModel[] = [
  {
    id: "phi4-mini",
    titleKey: "rec.phi4_mini.title",
    blurbKey: "rec.phi4_mini.blurb",
    category: "light",
    family: "Phi-4",
    sizeLabel: "3.8B",
    hfRepo: "unsloth/Phi-4-mini-instruct-GGUF",
    file: "Phi-4-mini-instruct-Q4_K_M.gguf",
    fileBytes: Math.round(2.376 * GB),
    vramHintBytes: Math.round(4 * GB),
  },
  {
    id: "qwen35-4b",
    titleKey: "rec.qwen35_4b.title",
    blurbKey: "rec.qwen35_4b.blurb",
    category: "light",
    family: "Qwen 3.5",
    sizeLabel: "4B",
    hfRepo: "unsloth/Qwen3.5-4B-GGUF",
    file: "Qwen3.5-4B-Q4_K_M.gguf",
    fileBytes: Math.round(2.614 * GB),
    vramHintBytes: Math.round(5 * GB),
  },
  {
    id: "gemma4-e4b",
    titleKey: "rec.gemma4_e4b.title",
    blurbKey: "rec.gemma4_e4b.blurb",
    category: "chat",
    family: "Gemma 4",
    sizeLabel: "E4B",
    hfRepo: "unsloth/gemma-4-E4B-it-GGUF",
    file: "gemma-4-E4B-it-Q4_K_M.gguf",
    fileBytes: Math.round(4.747 * GB),
    vramHintBytes: Math.round(7 * GB),
  },
  {
    id: "gemma4-12b",
    titleKey: "rec.gemma4_12b.title",
    blurbKey: "rec.gemma4_12b.blurb",
    category: "chat",
    family: "Gemma 4",
    sizeLabel: "12B",
    hfRepo: "unsloth/gemma-4-12b-it-GGUF",
    file: "gemma-4-12b-it-Q4_K_M.gguf",
    fileBytes: Math.round(6.792 * GB),
    vramHintBytes: Math.round(10 * GB),
  },
  {
    id: "qwen36-27b",
    titleKey: "rec.qwen36_27b.title",
    blurbKey: "rec.qwen36_27b.blurb",
    category: "smart",
    family: "Qwen 3.6",
    sizeLabel: "27B",
    hfRepo: "unsloth/Qwen3.6-27B-GGUF",
    file: "Qwen3.6-27B-Q4_K_M.gguf",
    fileBytes: Math.round(16.038 * GB),
    vramHintBytes: Math.round(18 * GB),
    featured: true,
  },
  {
    id: "qwen36-27b-code",
    titleKey: "rec.qwen36_code.title",
    blurbKey: "rec.qwen36_code.blurb",
    category: "code",
    family: "Qwen 3.6",
    sizeLabel: "27B",
    hfRepo: "unsloth/Qwen3.6-27B-GGUF",
    file: "Qwen3.6-27B-Q4_K_M.gguf",
    fileBytes: Math.round(16.038 * GB),
    vramHintBytes: Math.round(18 * GB),
  },
  {
    id: "qwen36-35b-a3b",
    titleKey: "rec.qwen36_35b.title",
    blurbKey: "rec.qwen36_35b.blurb",
    category: "power",
    family: "Qwen 3.6",
    sizeLabel: "35B-A3B",
    hfRepo: "unsloth/Qwen3.6-35B-A3B-GGUF",
    file: "Qwen3.6-35B-A3B-UD-Q4_K_M.gguf",
    fileBytes: Math.round(21.109 * GB),
    vramHintBytes: Math.round(20 * GB),
  },
];

export const REC_CATEGORIES: { id: RecCategory | "all"; labelKey: string }[] = [
  { id: "all", labelKey: "rec.cat.all" },
  { id: "light", labelKey: "rec.cat.light" },
  { id: "chat", labelKey: "rec.cat.chat" },
  { id: "smart", labelKey: "rec.cat.smart" },
  { id: "code", labelKey: "rec.cat.code" },
  { id: "power", labelKey: "rec.cat.power" },
];

/** How well a model fits available memory (GPU VRAM or system RAM). */
export type FitLevel = "ok" | "tight" | "no" | "unknown";

export type FitResource = "vram" | "ram" | "unknown";

export interface FitInfo {
  level: FitLevel;
  /** Which pool the estimate used. */
  resource: FitResource;
  /** Usable budget after OS reserve (bytes), if known. */
  budgetBytes: number | null;
  /** Estimated need (bytes). */
  needBytes: number;
}

/** Leave headroom for OS / desktop / browser when running CPU-only. */
const RAM_RESERVE = 4 * GB;
/** Leave headroom for desktop compositor / other GPU clients. */
const VRAM_RESERVE = Math.round(0.75 * GB);

/**
 * Usable memory budget for model weights + modest context.
 * Prefer GPU VRAM; fall back to system RAM when no discrete GPU.
 */
export function memoryBudget(
  vramBytes: number | null,
  ramBytes: number | null,
): { budget: number; resource: FitResource } | null {
  if (vramBytes != null && vramBytes > 512 * 1024 * 1024) {
    return {
      budget: Math.max(0, vramBytes - VRAM_RESERVE),
      resource: "vram",
    };
  }
  if (ramBytes != null && ramBytes > 0) {
    return {
      budget: Math.max(0, ramBytes - RAM_RESERVE),
      resource: "ram",
    };
  }
  return null;
}

/** Need ≈ declared VRAM hint (already includes comfort margin for that quant). */
export function fitForNeed(
  needBytes: number,
  vramBytes: number | null,
  ramBytes: number | null,
): FitInfo {
  const pool = memoryBudget(vramBytes, ramBytes);
  if (!pool) {
    return { level: "unknown", resource: "unknown", budgetBytes: null, needBytes };
  }
  const { budget, resource } = pool;
  if (budget >= needBytes) {
    return { level: "ok", resource, budgetBytes: budget, needBytes };
  }
  // Tight: at least ~70% of need — may work with lower ctx / partial offload.
  if (budget >= needBytes * 0.7) {
    return { level: "tight", resource, budgetBytes: budget, needBytes };
  }
  return { level: "no", resource, budgetBytes: budget, needBytes };
}

export function fitInfo(
  model: RecommendedModel,
  vramBytes: number | null,
  ramBytes: number | null,
): FitInfo {
  return fitForNeed(model.vramHintBytes, vramBytes, ramBytes);
}

/** @deprecated use fitInfo — kept for call-site simplicity */
export function fitLevel(
  model: RecommendedModel,
  vramBytes: number | null,
  ramBytes: number | null,
): FitLevel {
  return fitInfo(model, vramBytes, ramBytes).level;
}

/**
 * Estimate fit for an arbitrary GGUF by file size.
 * Weights ≈ file size; add overhead for KV / runtime (min 1.5 GiB or 15%).
 */
export function fitFromFileBytes(
  fileBytes: number,
  vramBytes: number | null,
  ramBytes: number | null,
): FitInfo {
  if (!fileBytes || fileBytes <= 0) {
    return { level: "unknown", resource: "unknown", budgetBytes: null, needBytes: 0 };
  }
  const overhead = Math.max(1.5 * GB, fileBytes * 0.15);
  return fitForNeed(fileBytes + overhead, vramBytes, ramBytes);
}

const FIT_RANK: Record<FitLevel, number> = {
  ok: 0,
  tight: 1,
  unknown: 2,
  no: 3,
};

export function fitRank(level: FitLevel): number {
  return FIT_RANK[level];
}

/** Sort: better fit first, then editorial featured, then smaller need. */
export function sortByFit(
  models: RecommendedModel[],
  vramBytes: number | null,
  ramBytes: number | null,
): RecommendedModel[] {
  return [...models].sort((a, b) => {
    const fa = fitInfo(a, vramBytes, ramBytes);
    const fb = fitInfo(b, vramBytes, ramBytes);
    const dr = fitRank(fa.level) - fitRank(fb.level);
    if (dr !== 0) return dr;
    if (!!b.featured !== !!a.featured) return a.featured ? -1 : 1;
    return a.vramHintBytes - b.vramHintBytes;
  });
}

/**
 * Best starting pick for this machine: highest comfort among "ok",
 * else best "tight". Prefers editorial featured when fits are equal.
 */
export function bestForHardware(
  vramBytes: number | null,
  ramBytes: number | null,
  models: RecommendedModel[] = RECOMMENDED_MODELS,
): RecommendedModel | null {
  const sorted = sortByFit(models, vramBytes, ramBytes);
  const ok = sorted.filter((m) => fitInfo(m, vramBytes, ramBytes).level === "ok");
  if (ok.length) {
    // Prefer featured among ok; else largest that still fits (more capable).
    const featured = ok.find((m) => m.featured);
    if (featured) return featured;
    return ok[ok.length - 1];
  }
  const tight = sorted.find((m) => fitInfo(m, vramBytes, ramBytes).level === "tight");
  return tight ?? sorted[0] ?? null;
}
