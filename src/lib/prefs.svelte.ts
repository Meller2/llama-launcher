// Реактивные предпочтения UI: язык + уровень сложности.
// Синхронизируются из Settings при загрузке / сохранении.
import {
  translate,
  normalizeLocale,
  normalizeExpertise,
  type Locale,
  type Expertise,
} from "$lib/i18n";
import type { Settings } from "$lib/api";

class Prefs {
  locale = $state<Locale>("ru");
  expertise = $state<Expertise>("beginner");
  openUiOnReady = $state(true);

  constructor() {
    // Синхронизация data-locale / lang с DOM (шрифты, a11y).
    $effect.root(() => {
      $effect(() => {
        this.#syncDomLocale(this.locale);
      });
    });
  }

  #syncDomLocale(locale: Locale) {
    if (typeof document === "undefined") return;
    document.documentElement.lang = locale;
    document.documentElement.dataset.locale = locale;
  }

  /** Применить из загруженных/сохранённых настроек. */
  apply(s: Settings) {
    this.locale = normalizeLocale(s.locale);
    this.expertise = normalizeExpertise(s.expertise);
    this.openUiOnReady = s.open_ui_on_ready !== false;
    this.#syncDomLocale(this.locale);
  }

  t(key: string, params?: Record<string, string | number>): string {
    return translate(this.locale, key, params);
  }

  get isBeginner() {
    return this.expertise === "beginner";
  }
  get isIntermediate() {
    return this.expertise === "intermediate";
  }
  get isExpert() {
    return this.expertise === "expert";
  }
  /** Сырые GGUF-поля, -ngl, KV, tools… */
  get showAdvanced() {
    return this.expertise === "expert";
  }
  /** Сетка авто-параметров (без флажков CLI). */
  get showAutoDetails() {
    return this.expertise !== "beginner";
  }
  /** Можно выключить авто-конфиг. */
  get canDisableAuto() {
    return this.expertise !== "beginner";
  }
  /** Ручной путь к llama.cpp, несколько папок, etc. */
  get showPowerPaths() {
    return this.expertise !== "beginner";
  }
  /** Лог сервера по умолчанию развёрнут. */
  get logExpandedByDefault() {
    return this.expertise === "expert";
  }
}

export const prefs = new Prefs();
