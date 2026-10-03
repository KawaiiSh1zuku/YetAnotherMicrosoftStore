import type { ErrorCode, JobControl, JobStage, SafeError } from "./types";

const errorMessages: Record<ErrorCode, string> = {
  catalog_not_found: "未找到这个应用。",
  catalog_unavailable: "暂时无法连接 Microsoft Store，请稍后重试。",
  license_required: "此应用需要可用的商店授权。",
  market_unavailable: "此应用在当前市场不可用。",
  no_compatible_package: "没有与此设备兼容的安装包。",
  dependency_unresolved: "无法解析应用依赖，请重新获取应用信息。",
  download_failed: "下载失败，请检查网络后重试。",
  download_url_expired: "下载地址已过期，正在等待重新解析。",
  hash_mismatch: "下载内容校验失败，未执行安装。",
  signature_invalid: "安装包签名未通过系统信任校验。",
  elevation_cancelled: "已取消管理员授权。",
  deployment_denied: "Windows 拒绝了部署请求。",
  deployment_failed: "安装未完成，已保留原有应用状态。",
  package_in_use: "应用正在使用中，请关闭后重试。",
  store_entitlement_missing: "当前账户没有此应用的商店授权。",
  store_channel_unavailable: "Microsoft Store 服务当前不可用。",
  source_identity_mismatch: "应用来源身份与安装包不一致。",
  version_ahead_of_catalog: "已安装版本高于目录版本，不会降级。",
  msixvc_capability_unavailable: "此应用使用当前版本尚不支持的包格式。",
  unsupported_package_type: "此安装包类型暂不支持。",
  package_not_installed: "未找到对应的已安装应用。",
};

const stageLabels: Record<JobStage, string> = {
  queued: "等待中",
  resolving: "正在解析",
  selecting: "正在选择包",
  downloading: "正在下载",
  paused: "已暂停",
  verifying: "正在验证",
  awaiting_elevation: "等待管理员授权",
  deploying: "正在安装",
  interrupted: "等待恢复",
  needs_reconciliation: "正在核对系统状态",
  completed: "已完成",
  failed: "失败",
  cancelled: "已取消",
};

const controlLabels: Record<JobControl, string> = {
  pause: "暂停",
  resume: "继续",
  cancel: "取消",
};

export function localizeError(value: unknown): string {
  if (typeof value === "object" && value !== null && "code" in value) {
    const code = (value as { code?: string }).code as ErrorCode;
    if (code in errorMessages) return errorMessages[code];
  }
  return "操作未完成，请稍后重试。";
}

export function jobStageLabel(stage: JobStage): string {
  return stageLabels[stage];
}

export function jobControlLabel(control: JobControl): string {
  return controlLabels[control];
}

export function safeErrorLabel(error: SafeError | null): string | null {
  return error ? errorMessages[error.code] : null;
}
