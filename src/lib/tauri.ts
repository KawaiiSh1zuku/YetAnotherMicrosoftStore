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
  UpdateCandidate,
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

export interface StoreClient {
  searchApps(request: SearchRequest): Promise<CatalogProduct[]>;
  getAppDetails(request: DetailsRequest): Promise<AppDetails>;
  scanInstalledPackages(scope: DeploymentScope): Promise<InventorySnapshot>;
  scanUpdates(): Promise<UpdateCandidate[]>;
  startInstall(request: StartJobRequest): Promise<JobSnapshot>;
  startUpdate(request: StartJobRequest): Promise<JobSnapshot>;
  requestJobControl(request: JobControlRequest): Promise<JobSnapshot>;
  getJob(jobId: string): Promise<JobSnapshot | null>;
  listJobs(): Promise<JobSnapshot[]>;
  listJobEvents(request: ListJobEventsRequest): Promise<JobEventPage>;
  getSettings(): Promise<AppSettings>;
  updateSettings(settings: AppSettings): Promise<AppSettings>;
  clearCache(): Promise<void>;
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
  getJob: (jobId) => invoke("get_job", { jobId }),
  listJobs: () => invoke("list_jobs"),
  listJobEvents: (request) => invoke("list_job_events", { request }),
  getSettings: () => invoke("get_settings"),
  updateSettings: (settings) => invoke("update_settings", { settings }),
  clearCache: () => invoke("clear_cache"),
  exportDiagnostics: () => invoke("export_diagnostics"),
  subscribeJobChanges: async (listener) =>
    listen<JobChangedHint>("job://changed", ({ payload }) => listener(payload)),
};

export function shouldReplayHint(hint: JobChangedHint, current?: JobSnapshot): boolean {
  return current === undefined || hint.sequence > current.sequence;
}

export function applyEventPage(
  current: ReadonlyMap<string, JobSnapshot>,
  page: JobEventPage,
): { jobs: Map<string, JobSnapshot>; cursor: number | null } {
  const jobs = new Map(current);
  for (const event of [...page.events].sort((left, right) => left.cursor - right.cursor)) {
    const existing = jobs.get(event.jobId);
    if (existing === undefined || event.sequence > existing.sequence) {
      jobs.set(event.jobId, {
        ...event.snapshot,
        title: event.snapshot.title ?? existing?.title,
      });
    }
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
