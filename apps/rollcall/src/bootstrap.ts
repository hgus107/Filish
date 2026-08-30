import "@fontsource/inter/latin-400.css";
import "@fontsource/inter/latin-500.css";
import "@fontsource/inter/latin-600.css";
import "@fontsource/inter/latin-700.css";
import "@fontsource/inter/latin-800.css";

async function start() {
  if (import.meta.env.DEV && new URLSearchParams(window.location.search).has("e2e")) {
    await import("./e2e-harness.ts");
  }
  await import("./main.ts");
}

void start();
