import { invoke } from "@tauri-apps/api/core";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import * as QRCode from "qrcode";

type Phase = "idle" | "listening" | "captured" | "error";

type StatusSnapshot = {
  phase: Phase;
  message: string;
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
  errorMessage: document.querySelector<HTMLElement>("#error-message")!,
  setupCard: document.querySelector<HTMLElement>("#setup-card")!,
  successCard: document.querySelector<HTMLElement>("#success-card")!,
  proxyAddress: document.querySelector<HTMLElement>("#proxy-address")!,
  certificateUrl: document.querySelector<HTMLElement>("#certificate-url")!,
  certificateQr: document.querySelector<HTMLImageElement>("#certificate-qr")!,
  copyLink: document.querySelector<HTMLButtonElement>("#copy-link")!,
  cancelAction: document.querySelector<HTMLButtonElement>("#cancel-action")!,
  exportName: document.querySelector<HTMLElement>("#export-name")!,
  revealExport: document.querySelector<HTMLButtonElement>("#reveal-export")!,
  newExport: document.querySelector<HTMLButtonElement>("#new-export")!,
};

let latestStatus: StatusSnapshot | null = null;
let pollTimer: number | undefined;

function phaseLabel(phase: Phase): string {
  return {
    idle: "PRÊT",
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

  const listening = status.phase === "listening";
  const captured = status.phase === "captured";
  elements.setupCard.hidden = !listening;
  elements.successCard.hidden = !captured;
  elements.primaryAction.hidden = listening || captured;
  elements.primaryAction.disabled = false;
  elements.primaryAction.textContent = status.phase === "error" ? "Réessayer" : "Exporter mon compte";

  if (listening && status.localIp && status.port && status.certificateUrl) {
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

function beginPolling(): void {
  if (pollTimer !== undefined) window.clearInterval(pollTimer);
  pollTimer = window.setInterval(() => void refreshStatus(), 500);
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
      certificateWasCreated: false,
    });
  }
}

async function cancelExport(): Promise<void> {
  await invoke<StatusSnapshot>("cancel_export");
  await refreshStatus();
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
  elements.primaryAction.addEventListener("click", () => void startExport());
  elements.cancelAction.addEventListener("click", () => void cancelExport());
  elements.copyLink.addEventListener("click", () => void copyLink());
  elements.newExport.addEventListener("click", () => void startExport());
  elements.revealExport.addEventListener("click", () => {
    if (latestStatus?.exportPath) void revealItemInDir(latestStatus.exportPath);
  });
  void refreshStatus();
});
