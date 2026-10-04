import { vi } from "vitest";
import type {
  AppDetails,
  AppSettings,
  CatalogProduct,
  InventorySnapshot,
  JobSnapshot,
} from "../lib/types";
import type { StoreClient } from "../lib/tauri";

export const catalogProduct: CatalogProduct = {
  productId: "9NBLGGH4NNS1",
  packageFamilyName: "Microsoft.WindowsTerminal_8wekyb3d8bbwe",
  appName: "Windows Terminal",
  packageName: "Microsoft.WindowsTerminal",
  publisher: "Microsoft Corporation",
  iconUrl: "https://store-images.s-microsoft.com/image.png",
  metadataState: "complete",
  packageFormats: ["msixbundle"],
  frameworkDependencies: [],
};

export const appDetails: AppDetails = {
  ...catalogProduct,
  market: "US",
  language: "en-US",
  supportedArchitectures: ["x64", "arm64"],
  selectionPreview: {
    installable: true,
    main: { version: "1.2.3.4", architecture: "x64", format: "msix_bundle", language: "en-US" },
    dependencyCount: 1,
    rejectionReason: null,
  },
  localAction: {
    kind: "install",
    deploymentScope: null,
    installedVersion: null,
    availableVersion: "1.2.3.4",
    launchable: false,
  },
};

export const job: JobSnapshot = {
  jobId: "job-1",
  sequence: 4,
  productId: catalogProduct.productId,
  packageFamilyName: catalogProduct.packageFamilyName,
  title: catalogProduct.appName,
  stage: "downloading",
  bytesDone: 25,
  bytesTotal: 100,
  deploymentProgress: null,
  version: null,
  architecture: "x64",
  language: "en-US",
  allowedControls: ["pause", "cancel"],
  error: null,
  blockedProcesses: [],
  updatedAt: 20,
};

export const settings: AppSettings = {
  region: "US",
  market: "US",
  preferredArchitectures: ["x64"],
  preferredLanguages: ["en-US"],
  proxyMode: "disabled",
  proxyHost: null,
  proxyPort: null,
  proxyCredentials: "prompt_every_time",
  cacheEnabled: true,
  maxCacheBytes: 10_737_418_240,
  retentionDays: 30,
  keepInstalledPayloads: false,
  maxConcurrentDownloads: 2,
  maxConcurrentUpdateScans: 16,
  theme: "system",
  diagnosticsEnabled: false,
};

const inventory: InventorySnapshot = {
  source: "current_user",
  capturedAt: "2026-10-03T00:00:00Z",
  osBuild: "19045",
  complete: true,
  records: [],
  warnings: [],
};

export function createClient(overrides: Partial<StoreClient> = {}): StoreClient {
  return {
    searchApps: vi.fn().mockResolvedValue([catalogProduct]),
    getAppDetails: vi.fn().mockResolvedValue(appDetails),
    scanInstalledPackages: vi.fn().mockResolvedValue(inventory),
    scanUpdates: vi.fn().mockResolvedValue({
      scannedMainPackages: 0,
      associatedPackages: 0,
      candidates: [],
      skipped: [],
      complete: true,
    }),
    startInstall: vi.fn().mockResolvedValue(job),
    startUpdate: vi.fn().mockResolvedValue(job),
    requestJobControl: vi.fn().mockResolvedValue(job),
    terminateJobPackageProcesses: vi.fn().mockResolvedValue({ matched: [], terminated: [], remaining: [] }),
    launchInstalledApp: vi.fn().mockResolvedValue(undefined),
    getJob: vi.fn().mockResolvedValue(job),
    listJobs: vi.fn().mockResolvedValue([job]),
    listJobEvents: vi.fn().mockResolvedValue({ events: [], nextCursor: null }),
    getSettings: vi.fn().mockResolvedValue(settings),
    updateSettings: vi.fn().mockResolvedValue(settings),
    clearCache: vi.fn().mockResolvedValue(undefined),
    exportDiagnostics: vi.fn().mockResolvedValue({
      fileName: "yamstore-diagnostics-1.json",
      destination: "downloads",
    }),
    subscribeJobChanges: vi.fn().mockResolvedValue(() => undefined),
    ...overrides,
  };
}
