import { useCallback, useEffect, useMemo, useState } from "react";
import { AppShell, type PrimaryView } from "./components/AppShell";
import { DetailsView } from "./features/details/DetailsView";
import { InstalledView } from "./features/installed/InstalledView";
import { QueueView } from "./features/queue/QueueView";
import { SearchView, type SearchState } from "./features/search/SearchView";
import { SettingsView } from "./features/settings/SettingsView";
import { localizeError } from "./lib/i18n";
import { tauriClient, type StoreClient } from "./lib/tauri";
import type { AppSettings, CatalogProduct, JobSnapshot, ThemeMode } from "./lib/types";
import "./App.css";

interface AppProps { client?: StoreClient }
type AppView = PrimaryView | "details";

const initialSearch: SearchState = {
  query: "",
  results: [],
  status: "idle",
  error: null,
  recent: [],
};

function App({ client = tauriClient }: AppProps) {
  const [view, setView] = useState<AppView>("search");
  const [selectedProduct, setSelectedProduct] = useState<CatalogProduct | null>(null);
  const [restoreProductId, setRestoreProductId] = useState<string | null>(null);
  const [search, setSearch] = useState(initialSearch);
  const [jobs, setJobs] = useState<JobSnapshot[]>([]);
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [settingsLoadError, setSettingsLoadError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    client.getSettings().then((value) => {
      if (active) { setSettings(value); setSettingsLoadError(null); }
    }).catch((error) => {
      if (active) setSettingsLoadError(localizeError(error));
    });
    return () => { active = false; };
  }, [client]);

  useEffect(() => {
    const theme = settings?.theme ?? "system";
    const media = globalThis.matchMedia?.("(prefers-color-scheme: dark)");
    const update = () => applyTheme(theme, media?.matches ?? false);
    update();
    if (theme === "system") media?.addEventListener("change", update);
    return () => media?.removeEventListener("change", update);
  }, [settings?.theme]);

  const activeJobs = useMemo(() => jobs.filter((job) => !["completed", "failed", "cancelled"].includes(job.stage)).length, [jobs]);

  function navigate(next: PrimaryView) {
    if (view === "details" && next === "search" && selectedProduct) setRestoreProductId(selectedProduct.productId);
    setView(next);
  }

  function openDetails(product: CatalogProduct) {
    setSelectedProduct(product);
    setView("details");
  }

  function backToSearch() {
    setRestoreProductId(selectedProduct?.productId ?? null);
    setView("search");
  }

  function jobStarted(job: JobSnapshot) {
    setJobs((current) => [job, ...current.filter((item) => item.jobId !== job.jobId)]);
    setView("queue");
  }

  const focusRestored = useCallback(() => setRestoreProductId(null), []);

  return (
    <AppShell activeView={view} onNavigate={navigate} activeJobs={activeJobs}>
      {view === "search" && <SearchView client={client} settings={settings} state={search} setState={setSearch} onOpen={openDetails} restoreProductId={restoreProductId} onFocusRestored={focusRestored} />}
      {view === "details" && selectedProduct && <DetailsView client={client} product={selectedProduct} settings={settings} onBack={backToSearch} onJobStarted={jobStarted} />}
      {view === "queue" && <QueueView client={client} seedJobs={jobs} onJobsChanged={setJobs} />}
      {view === "installed" && <InstalledView client={client} settings={settings} onJobStarted={jobStarted} />}
      {view === "settings" && <SettingsView client={client} settings={settings} loadError={settingsLoadError} onSettingsChanged={setSettings} />}
    </AppShell>
  );
}

function applyTheme(theme: ThemeMode, systemDark: boolean) {
  const effective = theme === "system" ? (systemDark ? "dark" : "light") : theme;
  document.documentElement.dataset.theme = effective;
}

export default App;
