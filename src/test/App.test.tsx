import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { axe } from "vitest-axe";
import App from "../App";
import { localizeError } from "../lib/i18n";
import type { JobChangedHint, JobSnapshot } from "../lib/types";
import { appDetails, catalogProduct, createClient, job, settings } from "./fixtures";

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

  it("saves installed payload retention and confirms database cleanup", async () => {
    const user = userEvent.setup();
    const client = createClient({
      cleanupDatabase: vi.fn().mockResolvedValue({
        removedJobs: 2,
        removedEvents: 10,
        removedCommands: 1,
        removedDiagnostics: 1,
        removedProgress: 0,
      }),
    });
    render(<App client={client} />);

    await user.click(screen.getByRole("button", { name: "设置" }));
    const retention = await screen.findByRole("checkbox", { name: /安装后保留载荷/ });
    await user.click(retention);
    await user.click(screen.getByRole("button", { name: "保存设置" }));
    await waitFor(() => expect(client.updateSettings).toHaveBeenCalledWith(
      expect.objectContaining({ keepInstalledPayloads: true }),
    ));

    await user.click(screen.getByRole("button", { name: "清理任务数据库" }));
    expect(await screen.findByRole("alertdialog")).toHaveTextContent("失败任务和应用设置会保留");
    await user.click(screen.getByRole("button", { name: "确认清理" }));
    expect(client.cleanupDatabase).toHaveBeenCalledOnce();
    expect(await screen.findByRole("status")).toHaveTextContent("已清理 2 个任务和 10 条历史事件");
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
          selectedUpdateId: "has-update-v2",
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

    await user.click(screen.getByRole("button", { name: "设置" }));
    await screen.findByRole("heading", { name: "设置" });
    await user.click(screen.getByRole("button", { name: "已安装" }));
    expect(await screen.findByText(/发现 1 个更新/)).toBeVisible();
    expect(client.scanInstalledPackages).toHaveBeenCalledOnce();
    expect(client.scanUpdates).toHaveBeenCalledOnce();

    await user.click(screen.getByRole("button", { name: "更新至 2.0.0.0" }));
    expect(client.startUpdate).toHaveBeenCalledWith({
      productId: "9UPDATE",
      market: "US",
      language: "en-US",
      scope: "current_user",
      selectedUpdateId: "has-update-v2",
      packageFamilyName: "Has.Update_example",
    });
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

  it("polls list_jobs while the queue contains an active task", async () => {
    const client = createClient();
    render(<App client={client} />);

    await userEvent.click(screen.getByRole("button", { name: "队列" }));
    await screen.findByRole("article", { name: /Windows Terminal/ });
    await waitFor(() => expect(client.listJobs).toHaveBeenCalledTimes(2), { timeout: 1_200 });
  });

  it("shows CPU-heavy package preparation separately from Windows deployment", async () => {
    const client = createClient({
      listJobs: vi.fn().mockResolvedValue([{
        ...job,
        stage: "preparing",
        deploymentProgress: null,
        allowedControls: ["cancel"],
      }]),
    });
    render(<App client={client} />);

    await userEvent.click(screen.getByRole("button", { name: "队列" }));

    expect(await screen.findByText("正在准备安装")).toBeVisible();
    expect(screen.queryByRole("progressbar")).not.toBeInTheDocument();
    expect(screen.queryByText("0%")).not.toBeInTheDocument();
  });

  it("keeps an event snapshot that arrives while the queue listener is registering", async () => {
    let resolveList!: (jobs: JobSnapshot[]) => void;
    const listJobs = vi.fn().mockImplementation(
      () => new Promise<JobSnapshot[]>((resolve) => { resolveList = resolve; }),
    );
    const eventJob = { ...job, sequence: 8, stage: "verifying" as const, bytesDone: 100, bytesTotal: 100 };
    const subscribeJobChanges = vi.fn().mockImplementation(
      async (listener: (hint: JobChangedHint) => void) => {
        listener({ jobId: job.jobId, sequence: 8, updatedAt: 30 });
        return () => undefined;
      },
    );
    const client = createClient({
      listJobs,
      listJobEvents: vi.fn().mockResolvedValue({
        nextCursor: 1,
        events: [{ cursor: 1, jobId: job.jobId, sequence: 8, snapshot: eventJob }],
      }),
      subscribeJobChanges,
    });
    render(<App client={client} />);

    await userEvent.click(screen.getByRole("button", { name: "队列" }));
    expect(await screen.findByText("正在验证签名")).toBeVisible();

    await act(async () => resolveList([{ ...job, sequence: 7, stage: "downloading" }]));

    await waitFor(() => expect(screen.getByText("序列 8")).toBeVisible());
    expect(screen.queryByText("序列 7")).not.toBeInTheDocument();
  });

  it("renders signature verification without stale download progress", async () => {
    const client = createClient({
      listJobs: vi.fn().mockResolvedValue([{
        ...job,
        stage: "verifying",
        bytesDone: 100,
        bytesTotal: 100,
        allowedControls: [],
      }]),
    });
    render(<App client={client} />);

    await userEvent.click(screen.getByRole("button", { name: "队列" }));

    expect(await screen.findByText("正在验证签名")).toBeVisible();
    expect(screen.queryByText("100%")).not.toBeInTheDocument();
    expect(screen.queryByRole("progressbar")).not.toBeInTheDocument();
  });

  it("confirms, terminates related package processes, and retries an in-use job", async () => {
    const user = userEvent.setup();
    const blockedJob = {
      ...job,
      sequence: 7,
      stage: "awaiting_process_exit" as const,
      allowedControls: ["retry_deployment", "cancel"] as const,
      blockedProcesses: [
        { pid: 420, name: "Terminal.exe" },
        { pid: 421, name: "OpenConsole.exe" },
      ],
      error: {
        code: "package_in_use" as const,
        messageKey: "errors.packageInUse",
        retry: "retry" as const,
        jobId: job.jobId,
        details: [],
      },
    };
    const terminateJobPackageProcesses = vi.fn().mockResolvedValue({
      matched: blockedJob.blockedProcesses,
      terminated: blockedJob.blockedProcesses,
      remaining: [],
    });
    const client = Object.assign(createClient({
      listJobs: vi.fn().mockResolvedValue([blockedJob]),
      requestJobControl: vi.fn().mockResolvedValue({ ...blockedJob, sequence: 8, stage: "deploying", error: null, blockedProcesses: [] }),
    }), { terminateJobPackageProcesses });
    render(<App client={client} />);

    await user.click(screen.getByRole("button", { name: "队列" }));
    expect(await screen.findByRole("alertdialog", { name: "结束相关应用进程" })).toBeVisible();
    expect(screen.getByText("Terminal.exe (PID 420)")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "结束相关进程" }));

    await waitFor(() => expect(terminateJobPackageProcesses).toHaveBeenCalledWith(job.jobId));
    expect(client.requestJobControl).toHaveBeenCalledWith({
      jobId: job.jobId,
      expectedSequence: 7,
      control: "retry_deployment",
      commandId: expect.stringMatching(/^ui-/),
    });
  });

  it("keeps the process dialog open and lists every remaining name and pid", async () => {
    const user = userEvent.setup();
    const blockedJob = {
      ...job,
      sequence: 9,
      stage: "awaiting_process_exit" as const,
      allowedControls: ["retry_deployment", "cancel"] as const,
      blockedProcesses: [{ pid: 420, name: "Terminal.exe" }],
      error: {
        code: "package_in_use" as const,
        messageKey: "errors.packageInUse",
        retry: "retry" as const,
        jobId: job.jobId,
        details: [],
      },
    };
    const remaining = [
      { pid: 420, name: "Terminal.exe" },
      { pid: 421, name: "OpenConsole.exe" },
    ];
    const client = createClient({
      listJobs: vi.fn().mockResolvedValue([blockedJob]),
      terminateJobPackageProcesses: vi.fn().mockResolvedValue({
        matched: remaining,
        terminated: [],
        remaining,
      }),
    });
    render(<App client={client} />);

    await user.click(screen.getByRole("button", { name: "队列" }));
    await user.click(await screen.findByRole("button", { name: "结束相关进程" }));

    expect(await screen.findByText("Terminal.exe (PID 420)")).toBeVisible();
    expect(screen.getByText("OpenConsole.exe (PID 421)")).toBeVisible();
    expect(screen.getByRole("alertdialog", { name: "结束相关应用进程" })).toBeVisible();
    expect(client.requestJobControl).not.toHaveBeenCalled();
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

it("starts an update with the backend-derived deployment scope", async () => {
  const user = userEvent.setup();
  const details = {
    ...appDetails,
    localAction: {
      kind: "update" as const,
      deploymentScope: "all_users" as const,
      installedVersion: "1.0.0.0",
      availableVersion: "1.2.3.4",
      launchable: true,
    },
  };
  const client = createClient({ getAppDetails: vi.fn().mockResolvedValue(details) });
  render(<App client={client} />);

  await user.type(screen.getByRole("searchbox", { name: "搜索 Microsoft Store" }), "terminal");
  await user.click(screen.getByRole("button", { name: "搜索" }));
  await user.click(await screen.findByRole("button", { name: /Windows Terminal/ }));
  await user.click(await screen.findByRole("button", { name: "更新" }));
  await user.click(screen.getByRole("button", { name: "确认更新" }));

  expect(client.startUpdate).toHaveBeenCalledWith({
    productId: catalogProduct.productId,
    market: "US",
    language: "en-US",
    scope: "all_users",
  });
  expect(client.startInstall).not.toHaveBeenCalled();
});

it("opens an installed app without showing a deployment confirmation", async () => {
  const user = userEvent.setup();
  const details = {
    ...appDetails,
    selectionPreview: { ...appDetails.selectionPreview, installable: false },
    localAction: {
      kind: "open" as const,
      deploymentScope: null,
      installedVersion: "1.2.3.4",
      availableVersion: "1.2.3.4",
      launchable: true,
    },
  };
  const client = createClient({ getAppDetails: vi.fn().mockResolvedValue(details) });
  render(<App client={client} />);

  await user.type(screen.getByRole("searchbox", { name: "搜索 Microsoft Store" }), "terminal");
  await user.click(screen.getByRole("button", { name: "搜索" }));
  await user.click(await screen.findByRole("button", { name: /Windows Terminal/ }));
  await user.click(await screen.findByRole("button", { name: "打开" }));

  expect(client.launchInstalledApp).toHaveBeenCalledWith(catalogProduct.productId);
  expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  expect(client.startInstall).not.toHaveBeenCalled();
});

it("keeps an unlaunchable installed app as disabled Open instead of Install", async () => {
  const user = userEvent.setup();
  const details = {
    ...appDetails,
    selectionPreview: { ...appDetails.selectionPreview, installable: false },
    localAction: {
      kind: "open" as const,
      deploymentScope: null,
      installedVersion: "1.2.3.4",
      availableVersion: "1.2.3.4",
      launchable: false,
    },
  };
  const client = createClient({ getAppDetails: vi.fn().mockResolvedValue(details) });
  render(<App client={client} />);

  await user.type(screen.getByRole("searchbox", { name: "搜索 Microsoft Store" }), "terminal");
  await user.click(screen.getByRole("button", { name: "搜索" }));
  await user.click(await screen.findByRole("button", { name: /Windows Terminal/ }));

  expect(await screen.findByRole("button", { name: "打开" })).toBeDisabled();
  expect(screen.getByText("此应用没有可启动的入口。")).toBeVisible();
  expect(screen.queryByRole("button", { name: "安装" })).not.toBeInTheDocument();
});
