const products = {
  kiln: {
    name: "Kiln",
    mark: "K",
    title: "Kiln — Convert Images Locally",
    description: "Kiln converts images privately and locally on your Mac. Fast, offline, and free.",
    github: "https://github.com/hgus107/kiln",
    version: "Version 0.1.2 · macOS 13+",
  },
  rollcall: {
    name: "Rollcall",
    mark: "R",
    title: "Rollcall — Preview-First Bulk Rename",
    description: "Rename large file batches locally on your Mac. Preview every name and keep your originals untouched.",
    github: "https://github.com/hgus107/rollcall",
    version: "Version 0.1.2 · macOS 12+",
  },
};

function setProduct(productName, updateHistory = true) {
  const product = products[productName] ?? products.kiln;
  const selectedName = product === products.rollcall ? "rollcall" : "kiln";
  document.body.dataset.product = selectedName;
  document.title = product.title;
  document.querySelector("#meta-description").setAttribute("content", product.description);
  document.querySelector("#og-title").setAttribute("content", product.title);
  document.querySelector("#og-description").setAttribute("content", product.description);

  document.querySelectorAll("[data-product-panel]").forEach((panel) => {
    panel.hidden = panel.dataset.productPanel !== selectedName;
  });
  document.querySelectorAll("[data-product-tab]").forEach((tab) => {
    const active = tab.dataset.productTab === selectedName;
    tab.classList.toggle("active", active);
    tab.setAttribute("aria-selected", String(active));
  });
  document.querySelectorAll("[data-current-name]").forEach((node) => { node.textContent = product.name; });
  document.querySelectorAll("[data-current-mark]").forEach((node) => { node.textContent = product.mark; });
  document.querySelectorAll("[data-current-github]").forEach((link) => { link.href = product.github; });
  document.querySelectorAll("[data-current-releases]").forEach((link) => { link.href = `${product.github}/releases`; });
  document.querySelectorAll("[data-current-license]").forEach((link) => { link.href = `${product.github}/blob/main/LICENSE`; });
  document.querySelector("[data-current-version]").textContent = product.version;

  if (updateHistory) {
    const url = new URL(window.location.href);
    if (selectedName === "kiln") url.searchParams.delete("app");
    else url.searchParams.set("app", selectedName);
    window.history.replaceState({}, "", url);
    window.scrollTo({ top: 0, behavior: "smooth" });
  }
}

document.querySelectorAll("[data-product-tab]").forEach((tab) => {
  tab.addEventListener("click", () => setProduct(tab.dataset.productTab));
});

setProduct(new URLSearchParams(window.location.search).get("app"), false);
