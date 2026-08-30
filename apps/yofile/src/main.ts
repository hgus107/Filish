import { invoke } from "@tauri-apps/api/core";
import "./styles.css";

const status = document.getElementById("status") as HTMLParagraphElement;
const labels: Record<string, string> = {
  convert: "Convert",
  rename: "Rename",
  pdf: "PDF",
};

for (const button of document.querySelectorAll<HTMLButtonElement>(".tool")) {
  button.addEventListener("click", async () => {
    const tool = button.dataset.tool ?? "";
    status.textContent = `Opening ${labels[tool] ?? tool}…`;
    try {
      await invoke("launch_tool", { tool });
      status.textContent = `${labels[tool] ?? tool} opened.`;
    } catch (error) {
      status.textContent = String(error);
    }
  });
}
