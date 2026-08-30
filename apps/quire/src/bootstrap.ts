import "@fontsource/inter/latin-400.css";
import "@fontsource/inter/latin-500.css";

async function start() {
  if (import.meta.env.DEV && new URLSearchParams(window.location.search).has("e2e")) {
    await import("./e2e-harness.ts");
  }
  await import("./main.ts");
}

void start();
