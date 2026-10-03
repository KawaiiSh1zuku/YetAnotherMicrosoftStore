import { ArrowLeft, CheckCircle2, Download, ShieldCheck } from "lucide-react";
import { useEffect, useState } from "react";
import { ConfirmDialog } from "../../components/ui/alert-dialog";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Skeleton } from "../../components/ui/skeleton";
import { localizeError } from "../../lib/i18n";
import type { StoreClient } from "../../lib/tauri";
import type { AppDetails, AppSettings, CatalogProduct, DeploymentScope, JobSnapshot } from "../../lib/types";

interface DetailsViewProps {
  client: StoreClient;
  product: CatalogProduct;
  settings: AppSettings | null;
  onBack: () => void;
  onJobStarted: (job: JobSnapshot) => void;
}

export function DetailsView({ client, product, settings, onBack, onJobStarted }: DetailsViewProps) {
  const [details, setDetails] = useState<AppDetails | null>(null);
  const [scope, setScope] = useState<DeploymentScope>("current_user");
  const [status, setStatus] = useState<"loading" | "ready" | "starting" | "error">("loading");
  const [error, setError] = useState<string | null>(null);
  const [iconFailed, setIconFailed] = useState(false);

  useEffect(() => {
    let active = true;
    setStatus("loading");
    client.getAppDetails({
      productId: product.productId,
      market: settings?.market ?? "US",
      language: settings?.preferredLanguages[0] ?? "en-US",
    }).then((value) => {
      if (active) { setDetails(value); setStatus("ready"); }
    }).catch((value) => {
      if (active) { setError(localizeError(value)); setStatus("error"); }
    });
    return () => { active = false; };
  }, [client, product.productId, settings?.market, settings?.preferredLanguages]);

  async function install() {
    setStatus("starting");
    setError(null);
    try {
      const job = await client.startInstall({
        productId: product.productId,
        market: details?.market ?? settings?.market ?? "US",
        language: details?.language ?? settings?.preferredLanguages[0] ?? "en-US",
        scope,
      });
      onJobStarted(job);
      setStatus("ready");
    } catch (value) {
      setError(localizeError(value));
      setStatus("error");
    }
  }

  return (
    <section className="view details-view" aria-labelledby="details-heading">
      <Button variant="ghost" className="back-button" onClick={onBack}>
        <ArrowLeft aria-hidden="true" size={18} /> 返回搜索结果
      </Button>
      <header className="details-header">
        {(details?.iconUrl ?? product.iconUrl) && !iconFailed
          ? <img className="app-glyph app-glyph--large app-icon" src={details?.iconUrl ?? product.iconUrl ?? ""} alt="" onError={() => setIconFailed(true)} />
          : <div className="app-glyph app-glyph--large" aria-hidden="true">{product.appName.slice(0, 2).toUpperCase()}</div>}
        <div>
          <p className="eyebrow">应用详情</p>
          <h1 id="details-heading">{details?.appName ?? product.appName}</h1>
          <p>{product.publisher ?? "发布者未提供"}</p>
        </div>
      </header>

      {status === "loading" && <div className="details-loading"><Skeleton /><Skeleton /><Skeleton /></div>}
      {error && <div className="inline-alert" role="alert">{error}</div>}
      {details && (
        <>
          <div className="fact-strip" aria-label="应用安装信息">
            <div><span>市场</span><strong>{details.market}</strong></div>
            <div><span>语言</span><strong>{details.language}</strong></div>
            <div><span>架构</span><strong>{details.selectionPreview.main?.architecture ?? "不可用"}</strong></div>
            <div><span>格式</span><strong>{details.selectionPreview.main?.format ?? (details.packageFormats.join(", ") || "待解析")}</strong></div>
          </div>

          <div className="identity-grid" aria-label="应用身份">
            <div><span>应用名</span><strong title={details.appName}>{details.appName}</strong></div>
            <div><span>包名</span><strong title={details.packageName ?? ""}>{details.packageName ?? "待解析"}</strong></div>
            <div><span>PFN</span><strong title={details.packageFamilyName ?? ""}>{details.packageFamilyName ?? "待解析"}</strong></div>
            <div><span>发布者</span><strong title={details.publisher ?? ""}>{details.publisher ?? "未提供"}</strong></div>
          </div>
          {!details.selectionPreview.installable && <div className="inline-alert" role="status">{selectionRejection(details.selectionPreview.rejectionReason)}</div>}

          <section className="details-section" aria-labelledby="install-options-heading">
            <div>
              <h2 id="install-options-heading">安装范围</h2>
              <p>当前管理员账户安装不会更改其他 Windows 账户。</p>
            </div>
            <div className="segmented-control" aria-label="安装范围">
              <button type="button" aria-pressed={scope === "current_user"} onClick={() => setScope("current_user")}>当前管理员账户</button>
              <button type="button" aria-pressed={scope === "all_users"} onClick={() => setScope("all_users")}>所有用户</button>
            </div>
          </section>

          <div className="trust-row">
            <span><ShieldCheck aria-hidden="true" size={18} /> 系统信任签名验证</span>
            <span><CheckCircle2 aria-hidden="true" size={18} /> 安装前完整性检查</span>
          </div>

          <div className="details-actions">
            <ConfirmDialog
              trigger={<Button variant="primary" disabled={status === "starting" || !details.selectionPreview.installable}><Download aria-hidden="true" size={18} />安装</Button>}
              title={`安装 ${product.appName}`}
              description={scope === "all_users" ? "应用将为所有用户部署。" : "应用将安装到当前管理员账户。下载与验证会在后台继续。"}
              confirmLabel="确认安装"
              onConfirm={install}
            />
            {scope === "all_users" && <Badge>所有用户</Badge>}
          </div>
        </>
      )}
    </section>
  );
}

function selectionRejection(reason: AppDetails["selectionPreview"]["rejectionReason"]): string {
  const labels = {
    market: "当前市场不可用。",
    operating_system: "当前 Windows 版本不满足要求。",
    format: "当前系统不支持此包格式。",
    architecture: "没有与当前设备兼容的架构。",
    dependency: "缺少必需依赖。",
    package_not_installed: "未找到可更新的已安装包。",
    version: "已安装版本不低于目录版本。",
    no_compatible_package: "没有可用的兼容安装包。",
  } as const;
  return reason ? labels[reason] : "没有可用的兼容安装包。";
}
