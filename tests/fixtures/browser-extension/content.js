// Fill only the fixture's dummy username; no credential store or external service is involved.
const button = document.createElement("button");
button.id = "extension-fill";
button.textContent = "Fill demo username";
button.addEventListener("click", () => {
  document.querySelector("#name").value = "extension supplied";
});
document.body.append(button);
