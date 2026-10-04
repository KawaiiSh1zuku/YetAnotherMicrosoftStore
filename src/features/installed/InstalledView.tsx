import { RefreshCw, Search } from "lucide-react";
import { useEffect, useMemo, type Dispatch, type SetStateAction } from "react";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Input } from "../../components/ui/input";
import { localizeError } from "../../lib/i18n";
import type { StoreClient } from "../../lib/tauri";
import type { AppSettings, InventorySnapshot, JobSnapshot, UpdateCandidate, UpdateScanResult } from "../../lib/types";

interface InstalledViewProps {
  client: StoreClient;
  settings: AppSettings | null;
  state: InstalledState;
  setState: Dispatch<SetStateAction<InstalledState>>;
  onJobStarted: (job: JobSnapshot) => void;
}

export interface InstalledState {
  snapshot: InventorySnapshot | null;
  updateResult: UpdateScanResult | null;
  query: string;
  status: "idle" | "loading" | "ready" | "error";
  scanning: boolean;
  error: string | null;
}

export const initialInstalledState: InstalledState = {
  snapshot: null,
  updateResult: null,
  query: "",
  status: "idle",
  scanning: false,
  error: null,
};

export function InstalledView({ client, settings, state, setState, onJobStarted }: InstalledViewProps) {
  const { snapshot, updateResult, query, status, scanning, error } = state;

  async function loadInventory() {
    setState((current) => ({ ...current, status: "loading", error: null }));
    try {
      const nextSnapshot = await client.scanInstalledPackages("all_users");
      setState((current) => ({ ...current, snapshot: nextSnapshot, status: "ready" }));
    } catch (value) {
      setState((current) => ({ ...current, error: localizeError(value), status: "error" }));
    }
  }

  useEffect(() => {
    if (state.status === "idle") void loadInventory();
  }, [client]);

  async function scanUpdates() {
    setState((current) => ({ ...current, scanning: true, error: null }));
    try {
      const nextResult = await client.scanUpdates();
      setState((current) => ({ ...current, updateResult: nextResult }));
    } catch (value) {
      setState((current) => ({ ...current, error: localizeError(value) }));
    } finally {
      setState((current) => ({ ...current, scanning: false }));
    }
  }

  async function update(candidate: UpdateCandidate) {
    if (!candidate.productId) return;
    try {
      const job = await client.startUpdate({
        productId: candidate.productId,
        market: settings?.market ?? "US",
        language: settings?.preferredLanguages[0] ?? "en-US",
        scope: candidate.deploymentScope,
        selectedUpdateId: candidate.selectedUpdateId,
        packageFamilyName: candidate.packageFamilyName,
      });
      onJobStarted(job);
    } catch (value) {
      setState((current) => ({ ...current, error: localizeError(value) }));
    }
  }

  const candidatesByPackage = useMemo(() => new Map(
    (updateResult?.candidates ?? []).map((candidate) => [candidate.packageFamilyName.toLocaleLowerCase(), candidate]),
  ), [updateResult]);

  const records = useMemo(() => snapshot?.records.filter((record) => {
    const needle = query.trim().toLocaleLowerCase();
    return !needle || [record.appName, record.packageName, record.packageFamilyName, record.publisher]
      .some((value) => value.toLocaleLowerCase().includes(needle));
  }).map((record, index) => ({ record, index }))
    .sort((left, right) => {
      const leftHasUpdate = candidatesByPackage.has(left.record.packageFamilyName.toLocaleLowerCase());
      const rightHasUpdate = candidatesByPackage.has(right.record.packageFamilyName.toLocaleLowerCase());
      return Number(rightHasUpdate) - Number(leftHasUpdate) || left.index - right.index;
    })
    .map(({ record }) => record) ?? [], [candidatesByPackage, query, snapshot]);

  return (
    <section className="view" aria-labelledby="installed-heading">
      <header className="view-header">
        <div><p className="eyebrow">WINDOWS 包清单</p><h1 id="installed-heading">已安装</h1></div>
        <div className="header-actions">
          <Button onClick={() => void loadInventory()}><RefreshCw aria-hidden="true" size={17} />刷新</Button>
          <Button variant="primary" disabled={scanning} onClick={() => void scanUpdates()}>{scanning ? "正在扫描..." : "扫描更新"}</Button>
        </div>
      </header>
      {snapshot && <div className="inventory-meta"><Badge>{snapshot.complete ? "完整清单" : "部分清单"}</Badge><span>Windows build {snapshot.osBuild}</span><span>{snapshot.records.length} 个包</span></div>}
      {updateResult && <div className={updateResult.complete ? "success-message" : "warning-list"} role="status">
        扫描完成：{updateResult.scannedMainPackages} 个主包，{updateResult.associatedPackages} 个已关联，
        {updateResult.candidates.length ? `发现 ${updateResult.candidates.length} 个更新。` : "未发现更新。"}
        {!updateResult.complete && ` ${updateResult.skipped.length} 个包被跳过。`}
      </div>}
      <label className="table-search"><span className="sr-only">筛选已安装应用</span><Search aria-hidden="true" size={17} /><Input value={query} onChange={(event) => setState((current) => ({ ...current, query: event.target.value }))} placeholder="筛选名称或发布者" /></label>
      {error && <div className="inline-alert" role="alert">{error}</div>}
      {status === "loading" && <div role="status">正在读取本机包清单...</div>}
      {status !== "loading" && records.length === 0 && <div className="empty-state"><strong>{query ? "没有匹配的已安装应用" : "未发现已安装应用"}</strong></div>}
      {records.length > 0 && (
        <div className="table-wrap">
          <table><thead><tr><th>应用包</th><th>版本</th><th>架构</th><th>来源</th><th><span className="sr-only">操作</span></th></tr></thead>
            <tbody>{records.map((record) => {
              const candidate = candidatesByPackage.get(record.packageFamilyName.toLocaleLowerCase());
              return <tr key={record.packageFullName}>
                <td data-label="应用包"><strong title={record.appName}>{record.appName}</strong><span title={record.packageName}>{record.packageName || record.packageFamilyName}</span><span title={record.publisher}>{record.publisher}</span></td>
                <td data-label="版本">{record.version.join(".")}</td><td data-label="架构">{record.architecture}</td>
                <td data-label="来源"><Badge className="source-badge">{record.hasOtherUsers || record.provisionedForFutureUsers ? "所有用户" : "当前管理员账户"}</Badge></td>
                <td>{candidate && <Button compact variant="primary" disabled={!candidate.productId} onClick={() => void update(candidate)}>更新至 {candidate.availableVersion}</Button>}</td>
              </tr>;
            })}</tbody>
          </table>
        </div>
      )}
      {snapshot?.warnings.length ? <div className="warning-list" role="status">清单包含 {snapshot.warnings.length} 条脱敏警告。</div> : null}
    </section>
  );
}
