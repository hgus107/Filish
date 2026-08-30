async function start() {
  if (import.meta.env.DEV && new URLSearchParams(window.location.search).has("e2e")) {
    await import("./e2e-harness.ts");
  }
  await import("./main.ts");
}

void start();
