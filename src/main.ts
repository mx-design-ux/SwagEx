import { invoke } from "@tauri-apps/api/core";
import { Menu, Submenu } from "@tauri-apps/api/menu";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { openUrl } from "@tauri-apps/plugin-opener";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import * as QRCode from "qrcode";
import "@fontsource/kalam/400.css";
import "./styles.css";

type Phase = "idle" | "certificate_setup" | "proxy_setup" | "listening" | "captured" | "error";

type StatusSnapshot = {
  phase: Phase;
  message: string;
  certificateSetupCompleted: boolean;
  proxySetupCompleted: boolean;
  localIp?: string;
  port?: number;
  certificateUrl?: string;
  certificateWasCreated: boolean;
  exportPath?: string;
  profileName?: string;
};

const elements = {
  splash: document.querySelector<HTMLElement>("#splash-screen")!,
  app: document.querySelector<HTMLElement>("#app-screen")!,
  certificateScreen: document.querySelector<HTMLElement>("#certificate-screen")!,
  proxyScreen: document.querySelector<HTMLElement>("#proxy-screen")!,
  waitingScreen: document.querySelector<HTMLElement>("#waiting-screen")!,
  jsonScreen: document.querySelector<HTMLElement>("#json-screen")!,
  errorScreen: document.querySelector<HTMLElement>("#error-screen")!,
  certificateQr: document.querySelector<HTMLImageElement>("#certificate-qr")!,
  certificateDownload: document.querySelector<HTMLButtonElement>("#certificate-download")!,
  certificateDone: document.querySelector<HTMLButtonElement>("#certificate-done")!,
  proxyHost: document.querySelector<HTMLElement>("#proxy-host")!,
  proxyPort: document.querySelector<HTMLElement>("#proxy-port")!,
  proxyDone: document.querySelector<HTMLButtonElement>("#proxy-done")!,
  regenerateCertificate: document.querySelector<HTMLButtonElement>("#regenerate-certificate")!,
  stopListening: document.querySelector<HTMLButtonElement>("#stop-listening")!,
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
};

let latestStatus: StatusSnapshot | null = null;
let pollTimer: number | undefined;
let actionInProgress = false;
let availableUpdate: Update | null = null;
let updateInProgress = false;

function showScreen(screen: HTMLElement): void {
  [elements.certificateScreen, elements.proxyScreen, elements.waitingScreen, elements.jsonScreen, elements.errorScreen]
    .forEach((candidate) => { candidate.hidden = candidate !== screen; });
}

function updateStatus(status: StatusSnapshot): void {
  latestStatus = status;

  if (status.phase === "certificate_setup") {
    showScreen(elements.certificateScreen);
  } else if (status.phase === "proxy_setup" || status.phase === "idle") {
    showScreen(elements.proxyScreen);
  } else if (status.phase === "listening") {
    showScreen(elements.waitingScreen);
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
      margin: 1,
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

  elements.certificateDone.disabled = actionInProgress;
  elements.proxyDone.disabled = actionInProgress;
  elements.regenerateCertificate.disabled = actionInProgress;
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
  } catch (error) {
    updateInProgress = false;
    elements.updateDismiss.disabled = false;
    elements.updateInstall.disabled = false;
    elements.updateProgress.textContent = `La mise à jour a échoué : ${String(error)}`;
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
    const notes = update.body?.trim();
    showUpdateMessage(
      `SwagEx ${update.version} est disponible`,
      notes ? `Notes de version : ${notes}` : "Une nouvelle version est prête à être installée.",
      true,
    );
  } catch (error) {
    if (manual) {
      showUpdateMessage("Mise à jour indisponible", `Impossible de vérifier les mises à jour : ${String(error)}`, false);
    }
  }
}

async function installAppMenu(): Promise<void> {
  try {
    const updates = updateMenuItem();
    const quit = quitMenuItem();
    const appSubmenu = await Submenu.new({ text: "SwagEx", items: [updates, quit] });
    const menu = await Menu.new({ items: [appSubmenu] });
    await menu.setAsAppMenu();
  } catch {
    // The native menu is optional in browser development and older runtimes.
  }
}

function updateMenuItem() {
  return {
    id: "check-for-updates",
    text: "Rechercher les mises à jour…",
    action: () => { void checkForUpdates(true); },
  } as const;
}

function quitApplication(): void {
  stopPolling();
  void getCurrentWindow().close().catch((error) => {
    elements.errorMessage.textContent = `Impossible de fermer SwagEx : ${String(error)}`;
    showScreen(elements.errorScreen);
  });
}

function quitMenuItem() {
  return {
    id: "quit-app",
    text: "Quitter SwagEx",
    accelerator: "CmdOrCtrl+Q",
    action: quitApplication,
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
    proxySetupCompleted: false,
    certificateWasCreated: false,
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

async function initialize(): Promise<void> {
  const splashStartedAt = performance.now();
  try {
    const status = await invoke<StatusSnapshot>("export_status");
    if (status.certificateSetupCompleted) {
      updateStatus(await invoke<StatusSnapshot>("start_proxy_setup"));
    } else {
      updateStatus(await invoke<StatusSnapshot>("start_certificate_setup", { regenerate: false }));
    }
    const remainingSplashTime = Math.max(0, 5000 - (performance.now() - splashStartedAt));
    await new Promise((resolve) => window.setTimeout(resolve, remainingSplashTime));
    elements.splash.hidden = true;
    elements.app.hidden = false;
    beginPolling();
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
  elements.proxyDone.addEventListener("click", () => {
    // The iPhone forgets its manual proxy after each use, so this action is
    // deliberately identical on every run and immediately starts capture.
    void runAction("complete_proxy_setup");
  });
  elements.regenerateCertificate.addEventListener("click", () => {
    void runAction("start_certificate_setup", { regenerate: true });
  });
  elements.stopListening.addEventListener("click", () => {
    void runAction("cancel_export");
  });
  elements.certificateDownload.addEventListener("click", () => {
    if (latestStatus?.certificateUrl) void openUrl(latestStatus.certificateUrl);
  });
  elements.revealExport.addEventListener("click", () => {
    if (!latestStatus?.exportPath) return;
    void invoke("reveal_export_in_finder", { path: latestStatus.exportPath }).catch((error) => {
      elements.errorMessage.textContent = `Impossible d’ouvrir le dossier du JSON : ${String(error)}`;
      showScreen(elements.errorScreen);
    });
  });
  elements.quitApp.addEventListener("click", quitApplication);
  elements.retryAction.addEventListener("click", () => {
    if (latestStatus?.certificateSetupCompleted) {
      void runAction("start_proxy_setup");
    } else {
      void runAction("start_certificate_setup", { regenerate: false });
    }
  });
  elements.updateDismiss.addEventListener("click", closeUpdateDialog);
  elements.updateInstall.addEventListener("click", () => { void installAvailableUpdate(); });
  void installAppMenu();
  void initialize();
});
