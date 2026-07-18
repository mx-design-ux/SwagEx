import { invoke } from "@tauri-apps/api/core";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import * as QRCode from "qrcode";

type Phase = "idle" | "setup" | "listening" | "captured" | "error";

type StatusSnapshot = {
  phase: Phase;
  message: string;
  setupCompleted: boolean;
  localIp?: string;
  port?: number;
  certificateUrl?: string;
  certificateWasCreated: boolean;
  exportPath?: string;
  profileName?: string;
};

const elements = {
  phaseLabel: document.querySelector<HTMLElement>("#phase-label")!,
  statusDot: document.querySelector<HTMLElement>("#status-dot")!,
  statusMessage: document.querySelector<HTMLElement>("#status-message")!,
  primaryAction: document.querySelector<HTMLButtonElement>("#primary-action")!,
  setupAgain: document.querySelector<HTMLButtonElement>("#setup-again")!,
  errorMessage: document.querySelector<HTMLElement>("#error-message")!,
  setupCard: document.querySelector<HTMLElement>("#setup-card")!,
  captureCard: document.querySelector<HTMLElement>("#capture-card")!,
  successCard: document.querySelector<HTMLElement>("#success-card")!,
  proxyAddress: document.querySelector<HTMLElement>("#proxy-address")!,
  captureAddress: document.querySelector<HTMLElement>("#capture-address")!,
  certificateUrl: document.querySelector<HTMLElement>("#certificate-url")!,
  certificateQr: document.querySelector<HTMLImageElement>("#certificate-qr")!,
  copyLink: document.querySelector<HTMLButtonElement>("#copy-link")!,
  completeSetup: document.querySelector<HTMLButtonElement>("#complete-setup")!,
  cancelSetup: document.querySelector<HTMLButtonElement>("#cancel-setup")!,
  cancelCapture: document.querySelector<HTMLButtonElement>("#cancel-capture")!,
  exportName: document.querySelector<HTMLElement>("#export-name")!,
  revealExport: document.querySelector<HTMLButtonElement>("#reveal-export")!,
  newExport: document.querySelector<HTMLButtonElement>("#new-export")!,
};

let latestStatus: StatusSnapshot | null = null;
let pollTimer: number | undefined;

function phaseLabel(phase: Phase): string {
  return {
    idle: "PRÊT",
    setup: "CONFIGURATION IPHONE",
    listening: "EN ATTENTE DU JEU",
    captured: "EXPORT TERMINÉ",
    error: "ERREUR",
  }[phase];
}

function updateStatus(status: StatusSnapshot): void {
  latestStatus = status;
  elements.phaseLabel.textContent = phaseLabel(status.phase);
  elements.statusMessage.textContent = status.message;
  elements.statusDot.dataset.phase = status.phase;
  elements.errorMessage.hidden = status.phase !== "error";
  elements.errorMessage.textContent = status.phase === "error" ? status.message : "";

  const inSetup = status.phase === "setup";
  const listening = status.phase === "listening";
  const captured = status.phase === "captured";
  const busy = inSetup || listening || captured;

  elements.setupCard.hidden = !inSetup;
  elements.captureCard.hidden = !listening;
  elements.successCard.hidden = !captured;
  elements.primaryAction.hidden = busy;
  elements.primaryAction.disabled = false;
  elements.primaryAction.textContent = status.setupCompleted
    ? "Exporter un JSON frais"
    : "Configurer mon iPhone";
  elements.setupAgain.hidden = !status.setupCompleted || busy;

  if (inSetup && status.localIp && status.port && status.certificateUrl) {
    const address = `${status.localIp}:${status.port}`;
    elements.proxyAddress.textContent = address;
    elements.certificateUrl.textContent = status.certificateUrl;
    void QRCode.toDataURL(status.certificateUrl, {
      margin: 1,
      width: 180,
      color: { dark: "#172033", light: "#ffffff" },
    }).then((url) => {
      elements.certificateQr.src = url;
    });
  }

  if (listening && status.localIp && status.port) {
    elements.captureAddress.textContent = `${status.localIp}:${status.port}`;
  }

  if (captured) {
    elements.exportName.textContent = status.exportPath ?? "Le fichier a été enregistré dans Téléchargements.";
  }
}

async function refreshStatus(): Promise<void> {
  try {
    updateStatus(await invoke<StatusSnapshot>("export_status"));
  } catch (error) {
    elements.errorMessage.hidden = false;
    elements.errorMessage.textContent = String(error);
  }
}

async function initialize(): Promise<void> {
  try {
    const status = await invoke<StatusSnapshot>("export_status");
    updateStatus(status);
    if (!status.setupCompleted && status.phase === "idle") {
      await startSetup();
    }
  } catch (error) {
    elements.errorMessage.hidden = false;
    elements.errorMessage.textContent = String(error);
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

async function startSetup(): Promise<void> {
  elements.primaryAction.disabled = true;
  elements.errorMessage.hidden = true;
  try {
    updateStatus(await invoke<StatusSnapshot>("start_setup"));
    beginPolling();
  } catch (error) {
    updateStatus({
      phase: "error",
      message: String(error),
      setupCompleted: false,
      certificateWasCreated: false,
    });
  }
}

async function startExport(): Promise<void> {
  elements.primaryAction.disabled = true;
  elements.errorMessage.hidden = true;
  try {
    updateStatus(await invoke<StatusSnapshot>("start_export"));
    beginPolling();
  } catch (error) {
    updateStatus({
      phase: "error",
      message: String(error),
      setupCompleted: latestStatus?.setupCompleted ?? false,
      certificateWasCreated: false,
    });
  }
}

async function completeSetup(): Promise<void> {
  try {
    updateStatus(await invoke<StatusSnapshot>("complete_setup"));
    stopPolling();
  } catch (error) {
    updateStatus({
      phase: "error",
      message: String(error),
      setupCompleted: false,
      certificateWasCreated: false,
    });
  }
}

async function resetSetup(): Promise<void> {
  try {
    updateStatus(await invoke<StatusSnapshot>("reset_setup"));
    await startSetup();
  } catch (error) {
    updateStatus({
      phase: "error",
      message: String(error),
      setupCompleted: false,
      certificateWasCreated: false,
    });
  }
}

async function cancelExport(): Promise<void> {
  updateStatus(await invoke<StatusSnapshot>("cancel_export"));
  stopPolling();
}

async function copyLink(): Promise<void> {
  if (!latestStatus?.certificateUrl) return;
  await navigator.clipboard.writeText(latestStatus.certificateUrl);
  const original = elements.copyLink.textContent;
  elements.copyLink.textContent = "Lien copié";
  window.setTimeout(() => {
    elements.copyLink.textContent = original;
  }, 1400);
}

window.addEventListener("DOMContentLoaded", () => {
  elements.primaryAction.addEventListener("click", () => {
    if (latestStatus?.setupCompleted) {
      void startExport();
    } else {
      void startSetup();
    }
  });
  elements.setupAgain.addEventListener("click", () => void resetSetup());
  elements.completeSetup.addEventListener("click", () => void completeSetup());
  elements.cancelSetup.addEventListener("click", () => void cancelExport());
  elements.cancelCapture.addEventListener("click", () => void cancelExport());
  elements.copyLink.addEventListener("click", () => void copyLink());
  elements.newExport.addEventListener("click", () => void startExport());
  elements.revealExport.addEventListener("click", () => {
    if (latestStatus?.exportPath) void revealItemInDir(latestStatus.exportPath);
  });
  void initialize();
});
