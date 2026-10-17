// Reproduce password-manager onboarding opening a tab outside the agent's active page.
chrome.runtime.onInstalled.addListener(() => {
  chrome.tabs.create({ url: chrome.runtime.getURL("welcome.html") });
});
