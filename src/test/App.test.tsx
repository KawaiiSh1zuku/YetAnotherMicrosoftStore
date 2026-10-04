import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { axe } from "vitest-axe";
import App from "../App";
import { localizeError } from "../lib/i18n";
import { catalogProduct, createClient, job, settings } from "./fixtures";

describe("M6 desktop workbench", () => {
  it("shows the concrete HTTP status for download failures", () => {
    expect(localizeError({
      code: "download_http_status",
      messageKey: "errors.downloadHttpStatus",
      retry: "retry",
      details: [{ kind: "http_status", status: 429 }],
    })).toBe("下载服务器返回了错误状态。（HTTP 429）");

    expect(localizeError({
      code: "download_url_expired",
      messageKey: "errors.downloadUrlExpired",
      retry: "re_resolve",
      details: [{ kind: "http_status", status: 404 }],
    })).toBe("下载地址已过期，正在等待重新解析。（HTTP 404）");
  });

  it("restores focus to the selected result after closing details", async () => {
    const user = userEvent.setup();
    const client = createClient();
    render(<App client={client} />);

    await user.type(screen.getByRole("searchbox", { name: "搜索 Microsoft Store" }), "terminal");
    await user.click(screen.getByRole("button", { name: "搜索" }));
    const result = await screen.findByRole("button", { name: /Windows Terminal/ });
    await user.click(result);

    expect(await screen.findByRole("heading", { name: "Windows Terminal" })).toBeVisible();
    await user.click(screen.getByRole("button", { name: "返回搜索结果" }));
    expect(screen.getByRole("button", { name: /Windows Terminal/ })).toHaveFocus();
  });

  it("submits only allowed durable job controls", async () => {
    const user = userEvent.setup();
    const client = createClient();
    render(<App client={client} />);

    await user.click(screen.getByRole("button", { name: "队列" }));
    const row = await screen.findByRole("article", { name: /Windows Terminal/ });
    await user.click(within(row).getByRole("button", { name: "暂停" }));

    expect(client.requestJobControl).toHaveBeenCalledWith({
      jobId: "job-1",
      expectedSequence: 4,
      control: "pause",
      commandId: expect.stringMatching(/^ui-/),
    });
    expect(within(row).queryByRole("button", { name: "继续" })).not.toBeInTheDocument();
  });

  it("validates custom proxy settings before invoking update_settings", async () => {
    const user = userEvent.setup();
    const client = createClient({
      getSettings: vi.fn().mockResolvedValue({ ...settings, proxyMode: "socks5" }),
    });
    render(<App client={client} />);

    await user.click(screen.getByRole("button", { name: "设置" }));
    await screen.findByRole("heading", { name: "设置" });
    await user.click(screen.getByRole("button", { name: "保存设置" }));

    const host = screen.getByRole("textbox", { name: "代理主机" });
    expect(host).toHaveAttribute("aria-invalid", "true");
    expect(screen.getByText("请输入不含协议的代理主机名或 IP 地址。")).toBeVisible();
    expect(client.updateSettings).not.toHaveBeenCalled();
  });

  it("exports a redacted diagnostics report from settings", async () => {
    const user = userEvent.setup();
    const client = createClient();
    render(<App client={client} />);

    await user.click(screen.getByRole("button", { name: "设置" }));
    await screen.findByRole("heading", { name: "设置" });
    await user.click(screen.getByRole("button", { name: "导出诊断" }));

    expect(client.exportDiagnostics).toHaveBeenCalledOnce();
    expect(await screen.findByRole("status")).toHaveTextContent(
      "诊断文件已保存到下载目录：yamstore-diagnostics-1.json",
    );
  });

  it("saves an independent update scan concurrency up to sixty four", async () => {
    const user = userEvent.setup();
    const client = createClient();
    render(<App client={client} />);

    await user.click(screen.getByRole("button", { name: "设置" }));
    const concurrency = await screen.findByRole("spinbutton", { name: "更新扫描并发数" });
    await user.clear(concurrency);
    await user.type(concurrency, "64");
    await user.click(screen.getByRole("button", { name: "保存设置" }));

    await waitFor(() =>
      expect(client.updateSettings).toHaveBeenCalledWith(
        expect.objectContaining({ maxConcurrentUpdateScans: 64 }),
      ),
    );
  });

  it("saves the user ordered language priority list", async () => {
    const user = userEvent.setup();
    const client = createClient();
    render(<App client={client} />);

    await user.click(screen.getByRole("button", { name: "设置" }));
    const firstLanguage = await screen.findByRole("combobox", { name: "优先语言 1" });
    await user.selectOptions(firstLanguage, "zh-CN");
    await user.selectOptions(screen.getByRole("combobox", { name: "添加语言" }), "ja-JP");
    expect(within(firstLanguage).queryByRole("option", { name: /ja-JP/ })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "上移 ja-JP" }));
    await user.click(screen.getByRole("button", { name: "保存设置" }));

    await waitFor(() =>
      expect(client.updateSettings).toHaveBeenCalledWith(
        expect.objectContaining({ preferredLanguages: ["ja-JP", "zh-CN"] }),
      ),
    );
  });

  it("orders applications with available updates before the remaining installed apps", async () => {
    const user = userEvent.setup();
    const client = createClient({
      scanInstalledPackages: vi.fn().mockResolvedValue({
        source: "all_users_elevated",
        capturedAt: "2026-10-03T00:00:00Z",
        osBuild: "19045",
        complete: true,
        records: [
          {
            appName: "No Update",
            packageName: "No.Update",
            identityName: "No.Update",
            publisher: "Example",
            packageFamilyName: "No.Update_example",
            packageFullName: "No.Update_1.0.0.0_x64__example",
            version: [1, 0, 0, 0],
            architecture: "x64",
            packageKind: "main",
            installedForCurrentUser: true,
            hasOtherUsers: false,
            provisionedForFutureUsers: false,
          },
          {
            appName: "Has Update",
            packageName: "Has.Update",
            identityName: "Has.Update",
            publisher: "Example",
            packageFamilyName: "Has.Update_example",
            packageFullName: "Has.Update_1.0.0.0_x64__example",
            version: [1, 0, 0, 0],
            architecture: "x64",
            packageKind: "main",
            installedForCurrentUser: true,
            hasOtherUsers: false,
            provisionedForFutureUsers: false,
          },
        ],
        warnings: [],
      }),
      scanUpdates: vi.fn().mockResolvedValue({
        scannedMainPackages: 2,
        associatedPackages: 2,
        candidates: [{
          appName: "Has Update",
          packageName: "Has.Update",
          publisher: "Example",
          packageFamilyName: "Has.Update_example",
          currentVersion: "1.0.0.0",
          availableVersion: "2.0.0.0",
          productId: "9UPDATE",
          deploymentScope: "current_user",
        }],
        skipped: [],
        complete: true,
      }),
    });
    render(<App client={client} />);

    await user.click(screen.getByRole("button", { name: "已安装" }));
    await screen.findByText("No Update");
    await user.click(screen.getByRole("button", { name: "扫描更新" }));
    await screen.findByText(/发现 1 个更新/);

    const rows = within(screen.getByRole("table")).getAllByRole("row").slice(1);
    expect(within(rows[0]).getByText("Has Update")).toBeVisible();
    expect(within(rows[1]).getByText("No Update")).toBeVisible();
  });

  it("shows deployment progress while an installation is running", async () => {
    const client = createClient({
      listJobs: vi.fn().mockResolvedValue([{
        ...job,
        stage: "deploying",
        deploymentProgress: 42,
        allowedControls: [],
      }]),
    });
    render(<App client={client} />);

    await userEvent.click(screen.getByRole("button", { name: "队列" }));

    expect(await screen.findByRole("progressbar", { name: "Windows Terminal 安装进度" })).toHaveAttribute("aria-valuenow", "42");
    expect(screen.getByText("42%")).toBeVisible();
  });

  it("confirms, terminates related package processes, and retries an in-use job", async () => {
    const user = userEvent.setup();
    const failedJob = {
      ...job,
      sequence: 7,
      stage: "failed" as const,
      allowedControls: ["resume", "cancel"] as const,
      error: {
        code: "package_in_use" as const,
        messageKey: "errors.packageInUse",
        retry: "retry" as const,
        jobId: job.jobId,
        details: [],
      },
    };
    const terminateJobPackageProcesses = vi.fn().mockResolvedValue({ matched: 2, terminated: 2, failed: 0 });
    const client = Object.assign(createClient({
      listJobs: vi.fn().mockResolvedValue([failedJob]),
      requestJobControl: vi.fn().mockResolvedValue({ ...failedJob, sequence: 8, stage: "queued", error: null }),
    }), { terminateJobPackageProcesses });
    render(<App client={client} />);

    await user.click(screen.getByRole("button", { name: "队列" }));
    await user.click(await screen.findByRole("button", { name: "已结束" }));
    await user.click(screen.getByRole("button", { name: "结束相关进程并重试" }));
    await user.click(screen.getByRole("button", { name: "确认结束" }));

    await waitFor(() => expect(terminateJobPackageProcesses).toHaveBeenCalledWith(job.jobId));
    expect(client.requestJobControl).toHaveBeenCalledWith({
      jobId: job.jobId,
      expectedSequence: 7,
      control: "resume",
      commandId: expect.stringMatching(/^ui-/),
    });
  });

  it("renders loading, empty, and localized safe error states", async () => {
    const user = userEvent.setup();
    let finishSearch: ((value: never[]) => void) | undefined;
    const client = createClient({
      searchApps: vi.fn().mockImplementation(
        () => new Promise<never[]>((resolve) => { finishSearch = resolve; }),
      ),
    });
    const { rerender } = render(<App client={client} />);

    await user.type(screen.getByRole("searchbox", { name: "搜索 Microsoft Store" }), "nothing");
    await user.click(screen.getByRole("button", { name: "搜索" }));
    expect(screen.getByRole("status")).toHaveTextContent("正在搜索");
    finishSearch?.([]);
    expect(await screen.findByText("没有找到匹配的应用。")).toBeVisible();

    const failing = createClient({
      searchApps: vi.fn().mockRejectedValue({
        code: "catalog_unavailable",
        messageKey: "errors.catalogUnavailable",
        details: [{ kind: "redacted", value: "C:\\Users\\Admin\\secret" }],
      }),
    });
    rerender(<App client={failing} />);
    await user.clear(screen.getByRole("searchbox", { name: "搜索 Microsoft Store" }));
    await user.type(screen.getByRole("searchbox", { name: "搜索 Microsoft Store" }), "terminal");
    await user.click(screen.getByRole("button", { name: "搜索" }));
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("暂时无法连接 Microsoft Store，请稍后重试。");
    expect(alert).not.toHaveTextContent("secret");
    expect(alert).not.toHaveTextContent("C:\\");
  });

  it("has no obvious accessibility violations in the narrow workbench", async () => {
    Object.defineProperty(window, "innerWidth", { configurable: true, value: 360 });
    const client = createClient();
    const { container } = render(<App client={client} />);
    await waitFor(() => expect(client.getSettings).toHaveBeenCalled());
    expect((await axe(container, { rules: { "color-contrast": { enabled: false } } })).violations).toEqual([]);
    expect(screen.getByRole("navigation", { name: "主导航" })).toBeVisible();
  });
});

it("starts installation from details with the selected locale", async () => {
  const user = userEvent.setup();
  const client = createClient();
  render(<App client={client} />);
  await user.type(screen.getByRole("searchbox", { name: "搜索 Microsoft Store" }), "terminal");
  await user.click(screen.getByRole("button", { name: "搜索" }));
  await user.click(await screen.findByRole("button", { name: /Windows Terminal/ }));
  await user.click(await screen.findByRole("button", { name: "安装" }));
  await user.click(screen.getByRole("button", { name: "确认安装" }));
  expect(client.startInstall).toHaveBeenCalledWith({
    productId: catalogProduct.productId,
    market: "US",
    language: "en-US",
    scope: "current_user",
  });
});
