import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { Menu, Submenu } from "@tauri-apps/api/menu";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { openUrl } from "@tauri-apps/plugin-opener";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import * as QRCode from "qrcode";
import "@fontsource/kalam/400.css";
import "./styles.css";

type Phase =
  | "idle"
  | "certificate_setup"
  | "windows_certificate_setup"
  | "export_choice"
  | "proxy_setup"
  | "listening"
  | "siege_listening"
  | "captured"
  | "error";

type StatusSnapshot = {
  phase: Phase;
  message: string;
  certificateSetupCompleted: boolean;
  windowsCertificateSetupCompleted: boolean;
  proxySetupCompleted: boolean;
  localIp?: string;
  port?: number;
  certificateUrl?: string;
  certificateWasCreated: boolean;
  certificateTrusted: boolean;
  exportPath?: string;
  profileName?: string;
  siegeMatchupCaptured: boolean;
  siegeAttackLogCaptured: boolean;
  siegeDefenseLogCaptured: boolean;
};

type GameDevice = "ios" | "steam" | "android";

const GAME_DEVICE_STORAGE_KEY = "swagex.game-device";

const elements = {
  splash: document.querySelector<HTMLElement>("#splash-screen")!,
  app: document.querySelector<HTMLElement>("#app-screen")!,
  deviceScreen: document.querySelector<HTMLElement>("#device-screen")!,
  certificateScreen: document.querySelector<HTMLElement>("#certificate-screen")!,
  windowsCertificateScreen: document.querySelector<HTMLElement>("#windows-certificate-screen")!,
  proxyScreen: document.querySelector<HTMLElement>("#proxy-screen")!,
  waitingScreen: document.querySelector<HTMLElement>("#waiting-screen")!,
  siegeScreen: document.querySelector<HTMLElement>("#siege-screen")!,
  jsonScreen: document.querySelector<HTMLElement>("#json-screen")!,
  errorScreen: document.querySelector<HTMLElement>("#error-screen")!,
  certificateQr: document.querySelector<HTMLImageElement>("#certificate-qr")!,
  certificateDownload: document.querySelector<HTMLButtonElement>("#certificate-download")!,
  certificateDone: document.querySelector<HTMLButtonElement>("#certificate-done")!,
  windowsCertificateAction: document.querySelector<HTMLButtonElement>("#windows-certificate-action")!,
  windowsCaptureActions: document.querySelector<HTMLElement>("#windows-capture-actions")!,
  windowsCertificateStatus: document.querySelector<HTMLElement>("#windows-certificate-status")!,
  proxyHost: document.querySelector<HTMLElement>("#proxy-host")!,
  proxyPort: document.querySelector<HTMLElement>("#proxy-port")!,
  stopListening: document.querySelector<HTMLButtonElement>("#stop-listening")!,
  stopSiegeListening: document.querySelector<HTMLButtonElement>("#stop-siege-listening")!,
  captureAccount: Array.from(document.querySelectorAll<HTMLButtonElement>(".capture-account")),
  captureSiege: Array.from(document.querySelectorAll<HTMLButtonElement>(".capture-siege")),
  siegeMatchupStep: document.querySelector<HTMLElement>("[data-siege-step='matchup']")!,
  siegeAttackStep: document.querySelector<HTMLElement>("[data-siege-step='attack']")!,
  siegeDefenseStep: document.querySelector<HTMLElement>("[data-siege-step='defense']")!,
  exportName: document.querySelector<HTMLElement>("#export-name")!,
  revealExport: document.querySelector<HTMLButtonElement>("#reveal-export")!,
  quitApp: document.querySelector<HTMLButtonElement>("#quit-app")!,
  errorMessage: document.querySelector<HTMLElement>("#error-message")!,
  retryAction: document.querySelector<HTMLButtonElement>("#retry-action")!,
  updateDialog: document.querySelector<HTMLElement>("#update-dialog")!,
  updateTitle: document.querySelector<HTMLElement>("#update-title")!,
  updateMessage: document.querySelector<HTMLElement>("#update-message")!,
  updateProgress: document.querySelector<HTMLElement>("#update-progress")!,
  updateDismiss: document.querySelector<HTMLButtonElement>("#update-dismiss")!,
  updateInstall: document.querySelector<HTMLButtonElement>("#update-install")!,
  chooseApple: document.querySelector<HTMLButtonElement>("#choose-apple")!,
  chooseSteam: document.querySelector<HTMLButtonElement>("#choose-steam")!,
  chooseAndroid: document.querySelector<HTMLButtonElement>("#choose-android")!,
  appVersions: Array.from(document.querySelectorAll<HTMLElement>("[data-app-version]")),
};

let latestStatus: StatusSnapshot | null = null;
let pollTimer: number | undefined;
let actionInProgress = false;
let availableUpdate: Update | null = null;
let updateInProgress = false;

async function renderAppVersion(): Promise<void> {
  const version = await getVersion();
  elements.appVersions.forEach((element) => {
    element.textContent = version;
  });
}

function showScreen(screen: HTMLElement): void {
  [
    elements.deviceScreen,
    elements.certificateScreen,
    elements.windowsCertificateScreen,
    elements.proxyScreen,
    elements.waitingScreen,
    elements.siegeScreen,
    elements.jsonScreen,
    elements.errorScreen,
  ]
    .forEach((candidate) => { candidate.hidden = candidate !== screen; });
  elements.app.dataset.screen = screen.dataset.screen ?? "";
}

function selectedGameDevice(): GameDevice | null {
  const stored = localStorage.getItem(GAME_DEVICE_STORAGE_KEY);
  if (stored === "apple" || stored === "ios") return "ios";
  if (stored === "steam") return "steam";
  if (stored === "android") return "android";
  return null;
}

function showGameDeviceChoice(): void {
  stopPolling();
  elements.windowsCertificateAction.textContent = "J’ai terminé !";
  showScreen(elements.deviceScreen);
}

async function startIosFlow(): Promise<void> {
  const status = await invoke<StatusSnapshot>("export_status");
  const next = status.certificateSetupCompleted
    ? await invoke<StatusSnapshot>("start_proxy_setup")
    : await invoke<StatusSnapshot>("start_certificate_setup", { regenerate: false });
  updateStatus(next);
  beginPolling();
}

async function selectAppleDevice(): Promise<void> {
  localStorage.setItem(GAME_DEVICE_STORAGE_KEY, "ios");
  try {
    await startIosFlow();
  } catch (error) {
    elements.errorMessage.textContent = String(error);
    showScreen(elements.errorScreen);
  }
}

async function startSteamFlow(): Promise<void> {
  const status = await invoke<StatusSnapshot>("export_status");
  const next = status.windowsCertificateSetupCompleted
    ? await invoke<StatusSnapshot>("prepare_steam_export_choice")
    : await invoke<StatusSnapshot>("start_windows_certificate_setup", { regenerate: false });
  updateStatus(next);
  beginPolling();
}

async function selectSteamDevice(): Promise<void> {
  localStorage.setItem(GAME_DEVICE_STORAGE_KEY, "steam");
  try {
    await startSteamFlow();
  } catch (error) {
    elements.errorMessage.textContent = String(error);
    showScreen(elements.errorScreen);
  }
}

async function changeGameDevice(): Promise<void> {
  stopPolling();
  if (["certificate_setup", "windows_certificate_setup", "proxy_setup", "listening", "siege_listening"].includes(latestStatus?.phase ?? "")) {
    try {
      const command = selectedGameDevice() === "steam" ? "cancel_steam_export" : "cancel_export";
      latestStatus = await invoke<StatusSnapshot>(command);
    } catch {
      // Returning to the choice screen must remain possible if the listener
      // already stopped between the last status refresh and this menu action.
    }
  }
  localStorage.removeItem(GAME_DEVICE_STORAGE_KEY);
  showGameDeviceChoice();
}

function updateStatus(status: StatusSnapshot): void {
  latestStatus = status;

  if (status.phase === "certificate_setup") {
    showScreen(elements.certificateScreen);
  } else if (status.phase === "windows_certificate_setup") {
    showScreen(elements.windowsCertificateScreen);
  } else if (status.phase === "export_choice") {
    showScreen(elements.windowsCertificateScreen);
  } else if (status.phase === "proxy_setup" || status.phase === "idle") {
    if (selectedGameDevice() === "steam") {
      showScreen(elements.windowsCertificateScreen);
    } else {
      showScreen(elements.proxyScreen);
    }
  } else if (status.phase === "listening") {
    showScreen(elements.waitingScreen);
  } else if (status.phase === "siege_listening") {
    showScreen(elements.siegeScreen);
  } else if (status.phase === "captured") {
    showScreen(elements.jsonScreen);
  } else {
    elements.errorMessage.textContent = status.message;
    showScreen(elements.errorScreen);
  }

  const hasAddress = Boolean(status.localIp && status.port);
  if (hasAddress) {
    elements.proxyHost.textContent = status.localIp!;
    elements.proxyPort.textContent = String(status.port!);
  }

  if (status.phase === "certificate_setup" && status.certificateUrl) {
    void QRCode.toDataURL(status.certificateUrl, {
      margin: 0,
      width: 232,
      errorCorrectionLevel: "M",
      color: { dark: "#FFC156", light: "#181818" },
    }).then((url) => { elements.certificateQr.src = url; });
  }

  if (status.phase === "captured") {
    elements.exportName.textContent = status.profileName
      ? `${status.profileName}.json`
      : "Compte.json";
  }

  elements.siegeMatchupStep.classList.toggle("is-complete", status.siegeMatchupCaptured);
  elements.siegeAttackStep.classList.toggle("is-complete", status.siegeAttackLogCaptured);
  elements.siegeDefenseStep.classList.toggle("is-complete", status.siegeDefenseLogCaptured);

  const windowsExportChoice = selectedGameDevice() === "steam" && status.phase === "export_choice";
  elements.windowsCertificateAction.hidden = windowsExportChoice;
  elements.windowsCaptureActions.hidden = !windowsExportChoice;

  if (
    status.phase === "windows_certificate_setup"
    || status.phase === "export_choice"
    || (status.phase === "idle" && selectedGameDevice() === "steam")
  ) {
    const needsAttention = status.phase === "windows_certificate_setup"
      && !status.certificateTrusted
      && !status.message.startsWith("Installez le certificat");
    elements.windowsCertificateStatus.hidden = !needsAttention;
    elements.windowsCertificateStatus.textContent = needsAttention ? status.message : "";

    elements.windowsCertificateAction.textContent = "J’ai terminé !";
  }

  elements.certificateDone.disabled = actionInProgress;
  elements.windowsCertificateAction.disabled = actionInProgress;
  elements.captureAccount.forEach((button) => { button.disabled = actionInProgress; });
  elements.captureSiege.forEach((button) => { button.disabled = actionInProgress; });
}

async function refreshStatus(): Promise<void> {
  try {
    updateStatus(await invoke<StatusSnapshot>("export_status"));
  } catch (error) {
    elements.errorMessage.textContent = String(error);
    showScreen(elements.errorScreen);
  }
}

function beginPolling(): void {
  stopPolling();
  pollTimer = window.setInterval(() => void refreshStatus(), 500);
}

function stopPolling(): void {
  if (pollTimer !== undefined) {
    window.clearInterval(pollTimer);
    pollTimer = undefined;
  }
}

function closeUpdateDialog(): void {
  elements.updateDialog.hidden = true;
  elements.updateProgress.hidden = true;
  elements.updateInstall.disabled = false;
}

function cleanReleaseNote(value: string): string {
  return value
    .replace(/\[([^\]]+)]\([^)]*\)/g, "$1")
    .replace(/\*\*([^*]+)\*\*/g, "$1")
    .replace(/`([^`]+)`/g, "$1")
    .trim();
}

function releaseNoteSummary(markdown: string): string | null {
  const firstSection = markdown.split(/^##\s+Télécharger\s+SwagEx\s*$/im)[0] ?? markdown;
  const note = firstSection
    .split(/\r?\n/)
    .map((line) => line.trim())
    .find((line) => line.length > 0 && !line.startsWith("#"));
  return note ? cleanReleaseNote(note.replace(/^-\s+/, "")) : null;
}

function showUpdateMessage(title: string, message: string, canInstall: boolean): void {
  elements.updateTitle.textContent = title;
  elements.updateMessage.textContent = message;
  elements.updateProgress.hidden = true;
  elements.updateInstall.hidden = !canInstall;
  elements.updateDismiss.textContent = canInstall ? "Plus tard" : "Fermer";
  elements.updateDialog.hidden = false;
}

async function installAvailableUpdate(): Promise<void> {
  if (!availableUpdate || updateInProgress) return;
  updateInProgress = true;
  elements.updateInstall.disabled = true;
  elements.updateDismiss.disabled = true;
  elements.updateProgress.hidden = false;
  elements.updateProgress.textContent = "Téléchargement…";

  try {
    let downloaded = 0;
    let contentLength = 0;
    await availableUpdate.downloadAndInstall((event) => {
      if (event.event === "Started") {
        contentLength = event.data.contentLength ?? 0;
      } else if (event.event === "Progress") {
        downloaded += event.data.chunkLength;
        if (contentLength > 0) {
          elements.updateProgress.textContent = `Téléchargement… ${Math.round((downloaded / contentLength) * 100)} %`;
        }
      } else if (event.event === "Finished") {
        elements.updateProgress.textContent = "Installation…";
      }
    });
    await relaunch();
  } catch {
    updateInProgress = false;
    elements.updateDismiss.disabled = false;
    elements.updateInstall.disabled = false;
    elements.updateProgress.textContent = "La mise à jour a échoué. Réessayez plus tard.";
  }
}

async function checkForUpdates(manual: boolean): Promise<void> {
  if (updateInProgress) return;
  try {
    const update = await check({ timeout: 5000 });
    if (!update) {
      if (manual) {
        showUpdateMessage("SwagEx est à jour", "Vous utilisez déjà la dernière version disponible.", false);
      }
      return;
    }

    availableUpdate = update;
    const note = update.body ? releaseNoteSummary(update.body) : null;
    showUpdateMessage(
      `SwagEx ${update.version} est disponible`,
      note ?? "Une nouvelle version est prête à être installée.",
      true,
    );
  } catch {
    if (manual) {
      showUpdateMessage("Mise à jour indisponible", "Impossible de vérifier les mises à jour. Réessayez plus tard.", false);
    }
  }
}

async function installAppMenu(): Promise<void> {
  try {
    const changeDevice = changeDeviceMenuItem();
    const newCertificate = newCertificateMenuItem();
    const updates = updateMenuItem();
    const quit = quitMenuItem();
    const appSubmenu = await Submenu.new({ text: "SwagEx", items: [changeDevice, newCertificate, updates, quit] });
    const menu = await Menu.new({ items: [appSubmenu] });
    await menu.setAsAppMenu();
  } catch {
    // The native menu is optional in browser development and older runtimes.
  }
}

async function createNewCertificate(): Promise<void> {
  stopPolling();
  try {
    const command = selectedGameDevice() === "steam"
      ? "start_windows_certificate_setup"
      : "start_certificate_setup";
    const status = await invoke<StatusSnapshot>(command, { regenerate: true });
    updateStatus(status);
    beginPolling();
  } catch (error) {
    elements.errorMessage.textContent = String(error);
    showScreen(elements.errorScreen);
  }
}

function changeDeviceMenuItem() {
  return {
    id: "change-game-device",
    text: "Changer mon appareil de jeu…",
    action: () => { void changeGameDevice(); },
  } as const;
}

function newCertificateMenuItem() {
  return {
    id: "new-certificate",
    text: "Nouveau certificat…",
    action: () => { void createNewCertificate(); },
  } as const;
}

function updateMenuItem() {
  return {
    id: "check-for-updates",
    text: "Rechercher les mises à jour…",
    action: () => { void checkForUpdates(true); },
  } as const;
}

async function quitApplication(): Promise<void> {
  stopPolling();
  if (selectedGameDevice() === "steam") {
    try {
      await invoke("stop_steam_capture");
    } catch {
      // Closing the application must remain possible if the listener already
      // stopped and its Windows hosts entries were already restored.
    }
  }
  await getCurrentWindow().close().catch((error) => {
    elements.errorMessage.textContent = `Impossible de fermer SwagEx : ${String(error)}`;
    showScreen(elements.errorScreen);
  });
}

function quitMenuItem() {
  return {
    id: "quit-app",
    text: "Quitter SwagEx",
    accelerator: "CmdOrCtrl+Q",
    action: () => { void quitApplication(); },
  } as const;
}

async function runAction(command: string, args?: Record<string, unknown>): Promise<void> {
  if (actionInProgress) return;
  actionInProgress = true;
  let succeeded = false;
  updateStatus(latestStatus ?? {
    phase: "error",
    message: "Action en cours…",
    certificateSetupCompleted: false,
    windowsCertificateSetupCompleted: false,
    proxySetupCompleted: false,
    certificateWasCreated: false,
    certificateTrusted: false,
    siegeMatchupCaptured: false,
    siegeAttackLogCaptured: false,
    siegeDefenseLogCaptured: false,
  });
  try {
    updateStatus(await invoke<StatusSnapshot>(command, args));
    succeeded = true;
  } catch (error) {
    elements.errorMessage.textContent = String(error);
    showScreen(elements.errorScreen);
  } finally {
    actionInProgress = false;
    if (succeeded && latestStatus) updateStatus(latestStatus);
  }
}

async function showDevelopmentPreview(): Promise<boolean> {
  if (!import.meta.env.DEV) return false;
  const preview = new URLSearchParams(window.location.search).get("preview");
  if (!preview) return false;

  if (preview === "splash") return true;
  elements.splash.hidden = true;
  elements.app.hidden = false;
  const previews: Record<string, HTMLElement> = {
    device: elements.deviceScreen,
    ios: elements.certificateScreen,
    windows: elements.windowsCertificateScreen,
    "windows-choice": elements.windowsCertificateScreen,
    proxy: elements.proxyScreen,
    waiting: elements.waitingScreen,
    siege: elements.siegeScreen,
    json: elements.jsonScreen,
  };
  const screen = previews[preview] ?? elements.deviceScreen;
  showScreen(screen);
  elements.proxyHost.textContent = "192.168.1.24";
  elements.proxyPort.textContent = "8080";
  elements.exportName.textContent = "Berserk~~65581.json";
  if (preview === "windows-choice") {
    elements.windowsCertificateAction.hidden = true;
    elements.windowsCaptureActions.hidden = false;
  }
  if (screen === elements.siegeScreen) {
    elements.siegeMatchupStep.classList.add("is-complete");
    elements.siegeAttackStep.classList.add("is-complete");
  }
  if (screen === elements.certificateScreen) {
    elements.certificateQr.src = await QRCode.toDataURL("http://192.168.1.24:8080/SwagEx.mobileconfig", {
      margin: 0,
      width: 232,
      errorCorrectionLevel: "M",
      color: { dark: "#FFC156", light: "#181818" },
    });
  }
  return true;
}

async function initialize(): Promise<void> {
  const splashStartedAt = performance.now();
  if (await showDevelopmentPreview()) return;
  try {
    await renderAppVersion();
    await invoke("recover_steam_route");
    await invoke<StatusSnapshot>("export_status");
    const remainingSplashTime = Math.max(0, 5000 - (performance.now() - splashStartedAt));
    await new Promise((resolve) => window.setTimeout(resolve, remainingSplashTime));
    elements.splash.hidden = true;
    elements.app.hidden = false;
    if (selectedGameDevice() === "ios") {
      await startIosFlow();
    } else if (selectedGameDevice() === "steam") {
      await startSteamFlow();
    } else {
      showGameDeviceChoice();
    }
    window.setTimeout(() => { void checkForUpdates(false); }, 1200);
  } catch (error) {
    const remainingSplashTime = Math.max(0, 5000 - (performance.now() - splashStartedAt));
    await new Promise((resolve) => window.setTimeout(resolve, remainingSplashTime));
    elements.splash.hidden = true;
    elements.app.hidden = false;
    elements.errorMessage.textContent = String(error);
    showScreen(elements.errorScreen);
  }
}

window.addEventListener("DOMContentLoaded", () => {
  elements.certificateDone.addEventListener("click", () => {
    void runAction("complete_certificate_setup");
  });
  const startCapture = (mode: "account" | "siege") => {
    const command = mode === "account" ? "start_account_capture" : "start_siege_capture";
    void runAction(command, { steamMode: selectedGameDevice() === "steam" });
  };
  elements.captureAccount.forEach((button) => {
    button.addEventListener("click", () => { startCapture("account"); });
  });
  elements.captureSiege.forEach((button) => {
    button.addEventListener("click", () => { startCapture("siege"); });
  });
  elements.stopListening.addEventListener("click", () => {
    void runAction(selectedGameDevice() === "steam" ? "cancel_steam_export" : "cancel_export");
  });
  elements.stopSiegeListening.addEventListener("click", () => {
    void runAction(selectedGameDevice() === "steam" ? "cancel_steam_export" : "cancel_export");
  });
  elements.windowsCertificateAction.addEventListener("click", () => {
    void runAction("complete_windows_certificate_setup");
  });
  elements.certificateDownload.addEventListener("click", () => {
    if (latestStatus?.certificateUrl) void openUrl(latestStatus.certificateUrl);
  });
  elements.revealExport.addEventListener("click", () => {
    if (!latestStatus?.exportPath) return;
    void invoke("reveal_export_in_file_manager", { path: latestStatus.exportPath }).catch((error) => {
      elements.errorMessage.textContent = `Impossible d’ouvrir le dossier du JSON : ${String(error)}`;
      showScreen(elements.errorScreen);
    });
  });
  elements.quitApp.addEventListener("click", () => { void quitApplication(); });
  elements.retryAction.addEventListener("click", () => {
    const device = selectedGameDevice();
    if (!device || device === "android") {
      showGameDeviceChoice();
    } else if (device === "steam") {
      void startSteamFlow();
    } else if (latestStatus?.certificateSetupCompleted) {
      void runAction("start_proxy_setup");
    } else {
      void runAction("start_certificate_setup", { regenerate: false });
    }
  });
  elements.updateDismiss.addEventListener("click", closeUpdateDialog);
  elements.updateInstall.addEventListener("click", () => { void installAvailableUpdate(); });
  elements.chooseApple.addEventListener("click", () => { void selectAppleDevice(); });
  elements.chooseSteam.addEventListener("click", () => { void selectSteamDevice(); });
  // Android deliberately remains in the technical device model while its
  // selector card stays hidden until real-device compatibility is validated.
  elements.chooseAndroid.addEventListener("click", showGameDeviceChoice);
  void installAppMenu();
  void initialize();
});
