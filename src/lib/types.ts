export type Architecture = "neutral" | "x86" | "x64" | "arm" | "arm64";
export type DeploymentScope = "current_user" | "all_users";
export type ProxyMode = "disabled" | "system" | "http" | "https" | "socks5";
export type ThemeMode = "light" | "dark" | "system";
export type JobControl = "pause" | "resume" | "retry_deployment" | "cancel";
export type JobStage =
  | "queued"
  | "resolving"
  | "selecting"
  | "downloading"
  | "paused"
  | "verifying"
  | "preparing"
  | "deploying"
  | "awaiting_process_exit"
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
  | "download_proxy_failed"
  | "download_proxy_auth_required"
  | "download_timeout"
  | "download_connection_failed"
  | "download_response_failed"
  | "download_http_status"
  | "download_redirect_rejected"
  | "download_io_failed"
  | "download_url_expired"
  | "hash_mismatch"
  | "signature_invalid"
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
  retry?: "never" | "retry" | "re_resolve" | "reconcile_inventory";
  jobId?: string;
  details?: ReadonlyArray<
    { kind: "field"; field: string }
    | { kind: "http_status"; status: number }
    | { kind: "redacted" }
  >;
}

export interface CatalogProduct {
  productId: string;
  packageFamilyName: string | null;
  appName: string;
  packageName: string | null;
  publisher: string | null;
  iconUrl: string | null;
  metadataState: "complete" | "partial";
  packageFormats: string[];
  frameworkDependencies: string[];
}

export interface AppDetails extends CatalogProduct {
  market: string;
  language: string;
  supportedArchitectures: Architecture[];
  selectionPreview: SelectionPreview;
  localAction: LocalProductAction;
}

export interface LocalProductAction {
  kind: "install" | "update" | "open";
  deploymentScope: DeploymentScope | null;
  installedVersion: string | null;
  availableVersion: string | null;
  launchable: boolean;
}

export interface SelectionPreview {
  installable: boolean;
  main: {
    version: string;
    architecture: Architecture;
    format: string;
    language: string | null;
  } | null;
  dependencyCount: number;
  rejectionReason: "market" | "operating_system" | "format" | "architecture" | "dependency" | "package_not_installed" | "version" | "no_compatible_package" | null;
}

export interface JobSnapshot {
  jobId: string;
  sequence: number;
  progressRevision: number;
  productId: string;
  packageFamilyName: string | null;
  title?: string;
  stage: JobStage;
  bytesDone: number;
  bytesTotal: number | null;
  deploymentProgress: number | null;
  version: string | null;
  architecture: Architecture | null;
  language: string | null;
  allowedControls: JobControl[];
  error: SafeError | null;
  blockedProcesses: ProcessDescriptor[];
  updatedAt: number;
}

export interface ProcessDescriptor {
  pid: number;
  name: string;
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
  appName: string;
  packageName: string;
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
  maxConcurrentUpdateScans: number;
  theme: ThemeMode;
  diagnosticsEnabled: boolean;
}

export interface UpdateCandidate {
  appName: string;
  packageName: string;
  publisher: string;
  packageFamilyName: string;
  currentVersion: string;
  availableVersion: string;
  selectedUpdateId: string;
  productId: string | null;
  deploymentScope: DeploymentScope;
}

export interface UpdateScanResult {
  scannedMainPackages: number;
  associatedPackages: number;
  candidates: UpdateCandidate[];
  skipped: Array<{
    packageFamilyName: string;
    reason: "missing_association" | "source_identity_mismatch" | "catalog_unavailable" | "selection_rejected";
  }>;
  complete: boolean;
}

export interface DiagnosticExport {
  fileName: string;
  destination: "downloads";
}

export interface TerminatePackageProcessesResult {
  matched: ProcessDescriptor[];
  terminated: ProcessDescriptor[];
  remaining: ProcessDescriptor[];
}
