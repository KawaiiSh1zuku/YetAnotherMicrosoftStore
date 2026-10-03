import { RefreshCw, Search } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Input } from "../../components/ui/input";
import { localizeError } from "../../lib/i18n";
import type { StoreClient } from "../../lib/tauri";
import type { AppSettings, InventorySnapshot, JobSnapshot, UpdateCandidate, UpdateScanResult } from "../../lib/types";

interface InstalledViewProps {
  client: StoreClient;
  settings: AppSettings | null;
  onJobStarted: (job: JobSnapshot) => void;
}

export function InstalledView({ client, settings, onJobStarted }: InstalledViewProps) {
  const [snapshot, setSnapshot] = useState<InventorySnapshot | null>(null);
  const [updateResult, setUpdateResult] = useState<UpdateScanResult | null>(null);
  const [query, setQuery] = useState("");
  const [status, setStatus] = useState<"loading" | "ready" | "error">("loading");
  const [scanning, setScanning] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function loadInventory() {
    setStatus("loading");
    try {
      setSnapshot(await client.scanInstalledPackages("all_users"));
      setStatus("ready");
    } catch (value) {
      setError(localizeError(value));
      setStatus("error");
    }
  }

  useEffect(() => { void loadInventory(); }, [client]);

  async function scanUpdates() {
    setScanning(true);
    setError(null);
    try { setUpdateResult(await client.scanUpdates()); }
    catch (value) { setError(localizeError(value)); }
    finally { setScanning(false); }
  }

  async function update(candidate: UpdateCandidate) {
    if (!candidate.productId) return;
    try {
      const job = await client.startUpdate({
        productId: candidate.productId,
        market: settings?.market ?? "US",
        language: settings?.preferredLanguages[0] ?? "en-US",
        scope: candidate.deploymentScope,
      });
      onJobStarted(job);
    } catch (value) { setError(localizeError(value)); }
  }

  const records = useMemo(() => snapshot?.records.filter((record) => {
    const needle = query.trim().toLocaleLowerCase();
    return !needle || [record.appName, record.packageName, record.packageFamilyName, record.publisher]
      .some((value) => value.toLocaleLowerCase().includes(needle));
  }) ?? [], [query, snapshot]);

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
      <label className="table-search"><span className="sr-only">筛选已安装应用</span><Search aria-hidden="true" size={17} /><Input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="筛选名称或发布者" /></label>
      {error && <div className="inline-alert" role="alert">{error}</div>}
      {status === "loading" && <div role="status">正在读取本机包清单...</div>}
      {status !== "loading" && records.length === 0 && <div className="empty-state"><strong>{query ? "没有匹配的已安装应用" : "未发现已安装应用"}</strong></div>}
      {records.length > 0 && (
        <div className="table-wrap">
          <table><thead><tr><th>应用包</th><th>版本</th><th>架构</th><th>来源</th><th><span className="sr-only">操作</span></th></tr></thead>
            <tbody>{records.map((record) => {
              const candidate = updateResult?.candidates.find((item) => item.packageFamilyName === record.packageFamilyName);
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
