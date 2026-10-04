import type { ErrorCode, JobControl, JobStage, SafeError } from "./types";

const errorMessages: Record<ErrorCode, string> = {
  catalog_not_found: "未找到这个应用。",
  catalog_unavailable: "暂时无法连接 Microsoft Store，请稍后重试。",
  license_required: "此应用需要可用的商店授权。",
  market_unavailable: "此应用在当前市场不可用。",
  no_compatible_package: "没有与此设备兼容的安装包。",
  dependency_unresolved: "无法解析应用依赖，请重新获取应用信息。",
  download_failed: "下载失败，请检查网络后重试。",
  download_proxy_failed: "无法连接代理服务器，请检查代理协议、地址和端口。",
  download_proxy_auth_required: "代理服务器要求身份验证，请检查代理凭据。",
  download_timeout: "下载连接超时，请稍后重试。",
  download_connection_failed: "无法连接下载服务器，TLS 握手或网络连接失败。",
  download_response_failed: "下载响应在传输完成前中断，请重试。",
  download_http_status: "下载服务器返回了错误状态。",
  download_redirect_rejected: "下载地址重定向到了不受信任的位置，已停止下载。",
  download_io_failed: "无法写入下载缓存，请检查磁盘空间和文件权限。",
  download_url_expired: "下载地址已过期，正在等待重新解析。",
  hash_mismatch: "下载内容校验失败，未执行安装。",
  signature_invalid: "安装包签名未通过系统信任校验。",
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
  verifying: "正在验证签名",
  preparing: "正在准备安装",
  deploying: "正在安装",
  awaiting_process_exit: "等待关闭占用进程",
  interrupted: "等待恢复",
  needs_reconciliation: "正在核对系统状态",
  completed: "已完成",
  failed: "失败",
  cancelled: "已取消",
};

const controlLabels: Record<JobControl, string> = {
  pause: "暂停",
  resume: "继续",
  retry_deployment: "重试部署",
  cancel: "取消",
};

export function localizeError(value: unknown): string {
  if (typeof value === "object" && value !== null && "code" in value) {
    const error = value as SafeError;
    if (error.code in errorMessages) return errorLabel(error);
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
  return error ? errorLabel(error) : null;
}

function errorLabel(error: SafeError): string {
  const message = errorMessages[error.code];
  const status = error.details?.find((detail) => detail.kind === "http_status");
  return status?.kind === "http_status" ? `${message}（HTTP ${status.status}）` : message;
}
