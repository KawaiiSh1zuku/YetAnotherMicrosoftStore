import { zodResolver } from "@hookform/resolvers/zod";
import { ArrowDown, ArrowUp, Database, Download, Globe2, Network, Palette, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { useFieldArray, useForm } from "react-hook-form";
import { z } from "zod";
import { ConfirmDialog } from "../../components/ui/alert-dialog";
import { Button } from "../../components/ui/button";
import { Input } from "../../components/ui/input";
import { localizeError } from "../../lib/i18n";
import { LANGUAGE_PRESETS, languageLabel } from "../../lib/languages";
import type { StoreClient } from "../../lib/tauri";
import type { AppSettings, ProxyMode, ThemeMode } from "../../lib/types";

const settingsSchema = z.object({
  region: z.string().trim().regex(/^[A-Za-z]{2}$/, "请输入两个字母的地区代码。"),
  market: z.string().trim().regex(/^[A-Za-z]{2}$/, "请输入两个字母的市场代码。"),
  languages: z.array(z.object({
    tag: z.string().regex(/^[A-Za-z0-9]+(?:-[A-Za-z0-9]+)*$/, "请选择有效的语言。"),
  })).max(32, "最多可设置 32 个优先语言。").superRefine((languages, context) => {
    languages.forEach((language, index) => {
      if (languages.slice(0, index).some((candidate) => candidate.tag.toLowerCase() === language.tag.toLowerCase())) {
        context.addIssue({ code: "custom", path: [index, "tag"], message: "语言不能重复。" });
      }
    });
  }),
  proxyMode: z.enum(["disabled", "system", "http", "https", "socks5"]),
  proxyHost: z.string().optional(),
  proxyPort: z.number().int().min(0).max(65535).nullable().optional(),
  cacheEnabled: z.boolean(),
  keepInstalledPayloads: z.boolean(),
  maxCacheGiB: z.number().int().min(1).max(1024),
  retentionDays: z.number().int().min(1).max(365),
  maxConcurrentDownloads: z.number().int().min(1).max(8),
  maxConcurrentUpdateScans: z.number().int().min(1).max(64),
  theme: z.enum(["light", "dark", "system"]),
  diagnosticsEnabled: z.boolean(),
}).superRefine((value, context) => {
  if (["http", "https", "socks5"].includes(value.proxyMode) && (typeof value.proxyHost !== "string" || !/^[\w.-]+$/.test(value.proxyHost))) {
    context.addIssue({ code: "custom", path: ["proxyHost"], message: "请输入不含协议的代理主机名或 IP 地址。" });
  }
  if (["http", "https", "socks5"].includes(value.proxyMode) && (!value.proxyPort || value.proxyPort < 1)) {
    context.addIssue({ code: "custom", path: ["proxyPort"], message: "请输入代理端口。" });
  }
});

type SettingsForm = z.infer<typeof settingsSchema>;

interface SettingsViewProps {
  client: StoreClient;
  settings: AppSettings | null;
  loadError: string | null;
  onSettingsChanged: (settings: AppSettings) => void;
}

export function SettingsView({ client, settings, loadError, onSettingsChanged }: SettingsViewProps) {
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const { control, register, handleSubmit, reset, watch, setValue, formState: { errors, isSubmitting } } = useForm<SettingsForm>({
    resolver: zodResolver(settingsSchema),
    defaultValues: formValues(settings),
  });
  const proxyMode = watch("proxyMode");
  const theme = watch("theme");
  const languageValues = watch("languages");
  const { fields: languageFields, append: appendLanguage, move: moveLanguage, remove: removeLanguage } = useFieldArray({ control, name: "languages" });

  useEffect(() => { reset(formValues(settings)); }, [reset, settings]);

  async function save(values: SettingsForm) {
    if (!settings) return;
    setError(null);
    setMessage(null);
    const next: AppSettings = {
      ...settings,
      region: values.region.toUpperCase(),
      market: values.market.toUpperCase(),
      preferredLanguages: values.languages.map((language) => language.tag),
      proxyMode: values.proxyMode,
      proxyHost: isCustomProxy(values.proxyMode) ? values.proxyHost ?? null : null,
      proxyPort: isCustomProxy(values.proxyMode) ? values.proxyPort ?? null : null,
      cacheEnabled: values.cacheEnabled,
      keepInstalledPayloads: values.keepInstalledPayloads,
      maxCacheBytes: values.maxCacheGiB * 1024 ** 3,
      retentionDays: values.retentionDays,
      maxConcurrentDownloads: values.maxConcurrentDownloads,
      maxConcurrentUpdateScans: values.maxConcurrentUpdateScans,
      theme: values.theme,
      diagnosticsEnabled: values.diagnosticsEnabled,
    };
    try {
      const saved = await client.updateSettings(next);
      onSettingsChanged(saved);
      setMessage("设置已保存。");
    } catch (value) { setError(localizeError(value)); }
  }

  async function clearCache() {
    setError(null);
    try { await client.clearCache(); setMessage("已清理未被任务使用的缓存。"); }
    catch (value) { setError(localizeError(value)); }
  }

  async function cleanupDatabase() {
    setError(null);
    try {
      const report = await client.cleanupDatabase();
      setMessage(`已清理 ${report.removedJobs} 个任务和 ${report.removedEvents} 条历史事件。`);
    } catch (value) { setError(localizeError(value)); }
  }

  async function exportDiagnostics() {
    setError(null);
    setMessage(null);
    try {
      const exported = await client.exportDiagnostics();
      setMessage(`诊断文件已保存到下载目录：${exported.fileName}`);
    } catch (value) { setError(localizeError(value)); }
  }

  if (!settings) return <section className="view">{loadError ? <div className="inline-alert" role="alert">{loadError}</div> : <div role="status">正在加载设置...</div>}</section>;
  return (
    <section className="view" aria-labelledby="settings-heading">
      <header className="view-header"><div><p className="eyebrow">本机偏好</p><h1 id="settings-heading">设置</h1></div></header>
      <form className="settings-form" onSubmit={handleSubmit(save)} noValidate>
        <SettingsSection icon={<Globe2 aria-hidden="true" />} title="地区与兼容性">
          <div className="field-grid field-grid--two">
            <Field label="地区" error={errors.region?.message}><Input {...register("region")} aria-invalid={Boolean(errors.region)} /></Field>
            <Field label="市场" error={errors.market?.message}><Input {...register("market")} aria-invalid={Boolean(errors.market)} /></Field>
          </div>
          <div className="language-priority" aria-label="语言优先级">
            <div className="language-priority__list">
              {languageFields.map((field, index) => {
                const tag = languageValues[index]?.tag ?? field.tag;
                return (
                  <div className="language-priority__row" key={field.id}>
                    <span className="language-priority__rank">{index + 1}</span>
                    <select className="select" aria-label={`优先语言 ${index + 1}`} {...register(`languages.${index}.tag`)}>
                      {!LANGUAGE_PRESETS.some((option) => option === tag) && (
                        <option value={tag}>{languageLabel(tag)}</option>
                      )}
                      {LANGUAGE_PRESETS
                        .filter((option) => option === tag || (
                          option.toLowerCase() !== tag.toLowerCase()
                          && !languageValues.some((language, candidateIndex) => (
                            candidateIndex !== index
                            && language.tag.toLowerCase() === option.toLowerCase()
                          ))
                        ))
                        .map((option) => <option key={option} value={option}>{languageLabel(option)}</option>)}
                    </select>
                    <button type="button" className="language-priority__action" aria-label={`上移 ${tag}`} title="上移" disabled={index === 0} onClick={() => moveLanguage(index, index - 1)}><ArrowUp aria-hidden="true" /></button>
                    <button type="button" className="language-priority__action" aria-label={`下移 ${tag}`} title="下移" disabled={index === languageFields.length - 1} onClick={() => moveLanguage(index, index + 1)}><ArrowDown aria-hidden="true" /></button>
                    <button type="button" className="language-priority__action language-priority__action--danger" aria-label={`删除 ${tag}`} title="删除" onClick={() => removeLanguage(index)}><Trash2 aria-hidden="true" /></button>
                    {errors.languages?.[index]?.tag?.message && <small className="field-error language-priority__error">{errors.languages[index]?.tag?.message}</small>}
                  </div>
                );
              })}
            </div>
            {languageFields.length < Math.min(32, LANGUAGE_PRESETS.length) && (
              <select
                className="select language-priority__add"
                aria-label="添加语言"
                value=""
                onChange={(event) => {
                  if (event.target.value) appendLanguage({ tag: event.target.value });
                }}
              >
                <option value="">添加语言</option>
                {LANGUAGE_PRESETS.filter((option) => !languageValues.some(
                  (language) => language.tag.toLowerCase() === option.toLowerCase(),
                ))
                  .map((option) => <option key={option} value={option}>{languageLabel(option)}</option>)}
              </select>
            )}
          </div>
        </SettingsSection>

        <SettingsSection icon={<Network aria-hidden="true" />} title="网络与代理">
          <Field label="代理模式">
            <select className="select" {...register("proxyMode")}>
              <option value="disabled">直连</option><option value="system">Windows 系统代理</option><option value="http">HTTP / CONNECT</option><option value="https">HTTPS（TLS 代理）</option><option value="socks5">SOCKS5</option>
            </select>
          </Field>
          <div className="field-grid">
            <Field label="代理主机" error={errors.proxyHost?.message}>
              <Input {...register("proxyHost")} disabled={!isCustomProxy(proxyMode)} aria-label="代理主机" aria-invalid={Boolean(errors.proxyHost)} aria-describedby={errors.proxyHost ? "proxy-host-error" : undefined} />
            </Field>
            <Field label="代理端口" error={errors.proxyPort?.message}>
              <Input type="number" {...register("proxyPort", { setValueAs: (value) => value === "" ? null : Number(value) })} disabled={!isCustomProxy(proxyMode)} aria-label="代理端口" aria-invalid={Boolean(errors.proxyPort)} />
            </Field>
          </div>
        </SettingsSection>

        <SettingsSection icon={<Database aria-hidden="true" />} title="缓存与并发">
          <label className="toggle-row"><span><strong>启用已验证包缓存</strong><small>重复安装时复用通过校验的载荷</small></span><input type="checkbox" {...register("cacheEnabled")} /></label>
          <label className="toggle-row"><span><strong>安装后保留载荷</strong><small>关闭时，安装成功后删除该任务的已验证包文件</small></span><input type="checkbox" {...register("keepInstalledPayloads")} /></label>
          <div className="field-grid">
            <Field label="缓存上限（GiB）" error={errors.maxCacheGiB?.message}><Input type="number" {...register("maxCacheGiB", { valueAsNumber: true })} /></Field>
            <Field label="保留天数" error={errors.retentionDays?.message}><Input type="number" {...register("retentionDays", { valueAsNumber: true })} /></Field>
            <Field label="并发下载数" error={errors.maxConcurrentDownloads?.message}><Input type="number" {...register("maxConcurrentDownloads", { valueAsNumber: true })} /></Field>
            <Field label="更新扫描并发数" error={errors.maxConcurrentUpdateScans?.message}><Input type="number" min={1} max={64} {...register("maxConcurrentUpdateScans", { valueAsNumber: true })} /></Field>
          </div>
          <ConfirmDialog trigger={<Button variant="danger">清理缓存</Button>} title="清理可回收缓存" description="只会删除未被任务占用的缓存，不会卸载应用。" confirmLabel="确认清理" destructive onConfirm={clearCache} />
          <ConfirmDialog trigger={<Button variant="danger">清理任务数据库</Button>} title="清理已结束任务" description="将删除已完成和已取消任务的历史记录，并压缩数据库。失败任务和应用设置会保留。" confirmLabel="确认清理" destructive onConfirm={cleanupDatabase} />
        </SettingsSection>

        <SettingsSection icon={<Palette aria-hidden="true" />} title="外观与诊断">
          <div className="segmented-control" aria-label="主题">
            {(["light", "dark", "system"] as ThemeMode[]).map((value) => <button key={value} type="button" aria-pressed={theme === value} onClick={() => setValue("theme", value)}>{value === "light" ? "浅色" : value === "dark" ? "深色" : "跟随系统"}</button>)}
          </div>
          <label className="toggle-row"><span><strong>保存脱敏诊断</strong><small>不包含下载地址、凭据或本地路径</small></span><input type="checkbox" {...register("diagnosticsEnabled")} /></label>
          <Button type="button" variant="secondary" onClick={exportDiagnostics}><Download aria-hidden="true" />导出诊断</Button>
        </SettingsSection>

        {error && <div className="inline-alert" role="alert">{error}</div>}
        {message && <div className="success-message" role="status">{message}</div>}
        <div className="form-actions"><Button type="submit" variant="primary" disabled={isSubmitting}>保存设置</Button></div>
      </form>
    </section>
  );
}

function formValues(settings: AppSettings | null): SettingsForm {
  return {
    region: settings?.region ?? "US", market: settings?.market ?? "US",
    languages: (settings?.preferredLanguages ?? ["en-US"]).map((tag) => ({ tag })),
    proxyMode: settings?.proxyMode ?? "disabled", proxyHost: settings?.proxyHost ?? "", proxyPort: settings?.proxyPort ?? null,
    cacheEnabled: settings?.cacheEnabled ?? true, maxCacheGiB: Math.max(1, Math.round((settings?.maxCacheBytes ?? 10 * 1024 ** 3) / 1024 ** 3)),
    keepInstalledPayloads: settings?.keepInstalledPayloads ?? false,
    retentionDays: settings?.retentionDays ?? 30, maxConcurrentDownloads: settings?.maxConcurrentDownloads ?? 2,
    maxConcurrentUpdateScans: settings?.maxConcurrentUpdateScans ?? 16,
    theme: settings?.theme ?? "system", diagnosticsEnabled: settings?.diagnosticsEnabled ?? false,
  };
}

function isCustomProxy(mode: ProxyMode): boolean { return mode === "http" || mode === "https" || mode === "socks5"; }

function SettingsSection({ icon, title, children }: { icon: React.ReactNode; title: string; children: React.ReactNode }) {
  return <section className="settings-section"><header>{icon}<h2>{title}</h2></header><div className="settings-section__body">{children}</div></section>;
}

function Field({ label, error, children }: { label: string; error?: string; children: React.ReactNode }) {
  return <label className="field"><span>{label}</span>{children}{error && <small className="field-error" id={label === "代理主机" ? "proxy-host-error" : undefined}>{error}</small>}</label>;
}
