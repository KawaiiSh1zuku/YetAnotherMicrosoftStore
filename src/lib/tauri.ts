import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AppDetails,
  AppSettings,
  CatalogProduct,
  DeploymentScope,
  DiagnosticExport,
  InventorySnapshot,
  JobChangedHint,
  JobControl,
  JobEventPage,
  JobSnapshot,
  TerminatePackageProcessesResult,
  UpdateScanResult,
} from "./types";

export interface SearchRequest {
  query: string;
  market: string;
  language: string;
}

export interface DetailsRequest {
  productId: string;
  market: string;
  language: string;
}

export interface StartJobRequest extends DetailsRequest {
  scope: DeploymentScope;
  selectedUpdateId?: string;
  packageFamilyName?: string;
}

export interface JobControlRequest {
  jobId: string;
  commandId: string;
  expectedSequence: number;
  control: JobControl;
}

export interface ListJobEventsRequest {
  afterCursor: number | null;
  limit: number;
}

export interface DatabaseCleanupReport {
  removedJobs: number;
  removedEvents: number;
  removedCommands: number;
  removedDiagnostics: number;
  removedProgress: number;
}

export interface StoreClient {
  searchApps(request: SearchRequest): Promise<CatalogProduct[]>;
  getAppDetails(request: DetailsRequest): Promise<AppDetails>;
  scanInstalledPackages(scope: DeploymentScope): Promise<InventorySnapshot>;
  scanUpdates(): Promise<UpdateScanResult>;
  startInstall(request: StartJobRequest): Promise<JobSnapshot>;
  startUpdate(request: StartJobRequest): Promise<JobSnapshot>;
  requestJobControl(request: JobControlRequest): Promise<JobSnapshot>;
  terminateJobPackageProcesses(jobId: string): Promise<TerminatePackageProcessesResult>;
  launchInstalledApp(productId: string): Promise<void>;
  getJob(jobId: string): Promise<JobSnapshot | null>;
  listJobs(): Promise<JobSnapshot[]>;
  listJobEvents(request: ListJobEventsRequest): Promise<JobEventPage>;
  getSettings(): Promise<AppSettings>;
  updateSettings(settings: AppSettings): Promise<AppSettings>;
  clearCache(): Promise<void>;
  cleanupDatabase(): Promise<DatabaseCleanupReport>;
  exportDiagnostics(): Promise<DiagnosticExport>;
  subscribeJobChanges(listener: (hint: JobChangedHint) => void): Promise<UnlistenFn>;
}

export const tauriClient: StoreClient = {
  searchApps: (request) => invoke("search_apps", { request }),
  getAppDetails: (request) => invoke("get_app_details", { request }),
  scanInstalledPackages: (scope) => invoke("scan_installed_packages", { scope }),
  scanUpdates: () => invoke("scan_updates"),
  startInstall: (request) => invoke("start_install", { request }),
  startUpdate: (request) => invoke("start_update", { request }),
  requestJobControl: (request) => invoke("request_job_control", { request }),
  terminateJobPackageProcesses: (jobId) => invoke("terminate_job_package_processes", { jobId }),
  launchInstalledApp: (productId) => invoke("launch_installed_app", { productId }),
  getJob: (jobId) => invoke("get_job", { jobId }),
  listJobs: () => invoke("list_jobs"),
  listJobEvents: (request) => invoke("list_job_events", { request }),
  getSettings: () => invoke("get_settings"),
  updateSettings: (settings) => invoke("update_settings", { settings }),
  clearCache: () => invoke("clear_cache"),
  cleanupDatabase: () => invoke("cleanup_database"),
  exportDiagnostics: () => invoke("export_diagnostics"),
  subscribeJobChanges: async (listener) =>
    listen<JobChangedHint>("job://changed", ({ payload }) => listener(payload)),
};

export function shouldReplayHint(hint: JobChangedHint, current?: JobSnapshot): boolean {
  return current === undefined || hint.sequence > current.sequence;
}

export function mergeJobSnapshots(
  current: ReadonlyMap<string, JobSnapshot>,
  incoming: Iterable<JobSnapshot>,
): Map<string, JobSnapshot> {
  const jobs = new Map(current);
  for (const snapshot of incoming) {
    const existing = jobs.get(snapshot.jobId);
    if (
      existing === undefined
      || snapshot.sequence > existing.sequence
      || (snapshot.sequence === existing.sequence && snapshot.progressRevision > existing.progressRevision)
    ) {
      jobs.set(snapshot.jobId, {
        ...snapshot,
        title: snapshot.title ?? existing?.title,
      });
    } else if (snapshot.sequence === existing.sequence && existing.title === undefined && snapshot.title) {
      jobs.set(snapshot.jobId, { ...existing, title: snapshot.title });
    }
  }
  return jobs;
}

export function applyEventPage(
  current: ReadonlyMap<string, JobSnapshot>,
  page: JobEventPage,
): { jobs: Map<string, JobSnapshot>; cursor: number | null } {
  let jobs = new Map(current);
  for (const event of [...page.events].sort((left, right) => left.cursor - right.cursor)) {
    jobs = mergeJobSnapshots(jobs, [event.snapshot]);
  }
  return { jobs, cursor: page.nextCursor };
}

export async function replayJobEvents(
  client: Pick<StoreClient, "listJobEvents">,
  current: ReadonlyMap<string, JobSnapshot>,
  afterCursor: number | null,
  limit = 100,
): Promise<{ jobs: Map<string, JobSnapshot>; cursor: number | null }> {
  let jobs = new Map(current);
  let cursor = afterCursor;
  for (;;) {
    const page = await client.listJobEvents({ afterCursor: cursor, limit });
    const merged = applyEventPage(jobs, page);
    jobs = merged.jobs;
    cursor = merged.cursor;
    if (page.events.length < limit) return { jobs, cursor };
  }
}

export function newCommandId(): string {
  const id = globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random().toString(16).slice(2)}`;
  return `ui-${id}`;
}
