<script lang="ts">
  // Шрифты — самохостинг через @fontsource (офлайн-desktop, без CDN).
  // Space Grotesk (латиница) — акцентные заголовки/бренд; кириллица падает на Inter.
  import "@fontsource/space-grotesk/latin-500.css";
  import "@fontsource/space-grotesk/latin-600.css";
  import "@fontsource/space-grotesk/latin-700.css";
  // Inter — тело интерфейса (латиница + кириллица).
  import "@fontsource/inter/latin-400.css";
  import "@fontsource/inter/latin-500.css";
  import "@fontsource/inter/latin-600.css";
  import "@fontsource/inter/cyrillic-400.css";
  import "@fontsource/inter/cyrillic-500.css";
  import "@fontsource/inter/cyrillic-600.css";
  // JetBrains Mono — цифры, параметры, пути, логи (латиница + кириллица).
  import "@fontsource/jetbrains-mono/latin-400.css";
  import "@fontsource/jetbrains-mono/latin-500.css";
  import "@fontsource/jetbrains-mono/cyrillic-400.css";

  import "$lib/styles/theme.css";
  let { children } = $props();
</script>

{@render children()}
