export type Architecture = "neutral" | "x86" | "x64" | "arm" | "arm64";
export type DeploymentScope = "current_user" | "all_users";
export type ProxyMode = "disabled" | "system" | "http" | "https" | "socks5";
export type ThemeMode = "light" | "dark" | "system";
export type JobControl = "pause" | "resume" | "cancel";
export type JobStage =
  | "queued"
  | "resolving"
  | "selecting"
  | "downloading"
  | "paused"
  | "verifying"
  | "awaiting_elevation"
  | "deploying"
  | "interrupted"
  | "needs_reconciliation"
  | "completed"
  | "failed"
  | "cancelled";

export type ErrorCode =
  | "catalog_not_found"
  | "catalog_unavailable"
  | "license_required"
  | "market_unavailable"
  | "no_compatible_package"
  | "dependency_unresolved"
  | "download_failed"
  | "download_url_expired"
  | "hash_mismatch"
  | "signature_invalid"
  | "elevation_cancelled"
  | "deployment_denied"
  | "deployment_failed"
  | "package_in_use"
  | "store_entitlement_missing"
  | "store_channel_unavailable"
  | "source_identity_mismatch"
  | "version_ahead_of_catalog"
  | "msixvc_capability_unavailable"
  | "unsupported_package_type"
  | "package_not_installed";

export interface SafeError {
  code: ErrorCode;
  messageKey: string;
  retry?: "never" | "retry" | "re_resolve" | "request_elevation" | "reconcile_inventory";
  jobId?: string;
  details?: ReadonlyArray<{ kind: "field"; field: string } | { kind: "redacted" }>;
}

export interface CatalogProduct {
  productId: string;
  packageFamilyName: string | null;
  title: string;
  publisher: string | null;
  packageFormats: string[];
  frameworkDependencies: string[];
}

export interface AppDetails extends CatalogProduct {
  market: string;
  language: string;
  supportedArchitectures: Architecture[];
}

export interface JobSnapshot {
  jobId: string;
  sequence: number;
  productId: string;
  packageFamilyName: string | null;
  title?: string;
  stage: JobStage;
  bytesDone: number;
  bytesTotal: number | null;
  version: string | null;
  architecture: Architecture | null;
  language: string | null;
  requiresElevation: boolean;
  allowedControls: JobControl[];
  error: SafeError | null;
  updatedAt: number;
}

export interface JobChangedHint {
  jobId: string;
  sequence: number;
  updatedAt: number;
}

export interface StoredJobEvent {
  cursor: number;
  jobId: string;
  sequence: number;
  snapshot: JobSnapshot;
}

export interface JobEventPage {
  events: StoredJobEvent[];
  nextCursor: number | null;
}

export interface PackageInventoryRecord {
  identityName: string;
  publisher: string;
  packageFamilyName: string;
  packageFullName: string;
  version: [number, number, number, number];
  architecture: string;
  packageKind: "main" | "framework" | "resource" | "optional" | "bundle" | "unknown";
  installedForCurrentUser: boolean;
  hasOtherUsers: boolean;
  provisionedForFutureUsers: boolean;
}

export interface InventorySnapshot {
  source: "current_user" | "all_users_elevated";
  capturedAt: string;
  osBuild: string;
  complete: boolean;
  records: PackageInventoryRecord[];
  warnings: string[];
}

export interface AppSettings {
  region: string;
  market: string;
  preferredArchitectures: Architecture[];
  preferredLanguages: string[];
  proxyMode: ProxyMode;
  proxyHost: string | null;
  proxyPort: number | null;
  proxyCredentials: "prompt_every_time" | "windows_credential_manager";
  cacheEnabled: boolean;
  maxCacheBytes: number;
  retentionDays: number;
  keepInstalledPayloads: boolean;
  maxConcurrentDownloads: number;
  theme: ThemeMode;
  diagnosticsEnabled: boolean;
}

export interface UpdateCandidate {
  packageFamilyName: string;
  currentVersion: string;
  availableVersion: string;
  productId: string | null;
}
