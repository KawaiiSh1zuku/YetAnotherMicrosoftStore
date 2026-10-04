import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => {
    const product = {
      productId: "9NBLGGH4NNS1",
      packageFamilyName: "Microsoft.WindowsTerminal_8wekyb3d8bbwe",
      appName: "Windows Terminal",
      packageName: "Microsoft.WindowsTerminal",
      publisher: "Microsoft Corporation",
      iconUrl: null,
      metadataState: "complete",
      packageFormats: ["msixbundle"],
      frameworkDependencies: [],
    };
    const settings = {
      region: "US", market: "US", preferredArchitectures: ["x64"], preferredLanguages: ["en-US"],
      proxyMode: "disabled", proxyHost: null, proxyPort: null, proxyCredentials: "prompt_every_time",
      cacheEnabled: true, maxCacheBytes: 10_737_418_240, retentionDays: 30,
      keepInstalledPayloads: false, maxConcurrentDownloads: 2, maxConcurrentUpdateScans: 16, theme: "light", diagnosticsEnabled: false,
    };
    const job = {
      jobId: "job-browser", sequence: 1, productId: product.productId, packageFamilyName: product.packageFamilyName,
      title: product.appName, stage: "queued", bytesDone: 0, bytesTotal: null, version: null, architecture: "x64",
      language: "en-US", deploymentProgress: null, allowedControls: ["cancel"], error: null, blockedProcesses: [], updatedAt: 1,
    };
    window.__YAMS_TEST_CLIENT__ = {
      searchApps: async () => [product],
      getAppDetails: async () => ({ ...product, market: "US", language: "en-US", supportedArchitectures: ["x64"], selectionPreview: { installable: true, main: { version: "1.2.3.4", architecture: "x64", format: "msix_bundle", language: "en-US" }, dependencyCount: 1, rejectionReason: null }, localAction: { kind: "install", deploymentScope: null, installedVersion: null, availableVersion: "1.2.3.4", launchable: false } }),
      scanInstalledPackages: async () => ({ source: "all_users_elevated", capturedAt: "2026-10-03T00:00:00Z", osBuild: "19045", complete: true, records: [], warnings: [] }),
      scanUpdates: async () => ({ scannedMainPackages: 0, associatedPackages: 0, candidates: [], skipped: [], complete: true }), startInstall: async () => job, startUpdate: async () => job,
      requestJobControl: async () => job, getJob: async () => job, listJobs: async () => [job],
      terminateJobPackageProcesses: async () => ({ matched: [], terminated: [], remaining: [] }),
      launchInstalledApp: async () => undefined,
      listJobEvents: async () => ({ events: [], nextCursor: null }), getSettings: async () => settings,
      updateSettings: async (next) => next, clearCache: async () => undefined,
      exportDiagnostics: async () => ({ fileName: "yamstore-diagnostics-browser.json", destination: "downloads" }),
      subscribeJobChanges: async () => () => undefined,
    };
  });
});

test("keyboard flow restores focus and has no serious axe findings", async ({ page }) => {
  await page.goto("/");
  const search = page.getByRole("searchbox", { name: "搜索 Microsoft Store" });
  await search.fill("terminal");
  await search.press("Enter");
  const result = page.getByRole("button", { name: /Windows Terminal/ });
  await expect(result).toBeVisible();
  await result.focus();
  await result.press("Enter");
  await expect(page.getByRole("heading", { name: "Windows Terminal" })).toBeVisible();

  const install = page.getByRole("button", { name: "安装", exact: true });
  await install.focus();
  await install.press("Enter");
  await expect(page.getByRole("alertdialog")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(install).toBeFocused();

  const back = page.getByRole("button", { name: "返回搜索结果" });
  await back.focus();
  await back.press("Enter");
  await expect(result).toBeFocused();

  const results = await new AxeBuilder({ page }).analyze();
  expect(results.violations.filter((item) => ["serious", "critical"].includes(item.impact ?? ""))).toEqual([]);
});

test("360 px workbench keeps navigation and text inside the viewport", async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 740 });
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "查找 Windows 应用" })).toBeVisible();
  await page.getByRole("button", { name: "设置" }).click();
  await expect(page.getByRole("heading", { name: "设置" })).toBeVisible();
  await page.getByRole("combobox", { name: "添加语言" }).selectOption("ja-JP");
  await page.getByRole("button", { name: "上移 ja-JP" }).click();
  await expect(page.getByRole("combobox", { name: "优先语言 1" })).toHaveValue("ja-JP");

  const overflow = await page.evaluate(() => ({
    document: document.documentElement.scrollWidth - document.documentElement.clientWidth,
    body: document.body.scrollWidth - document.body.clientWidth,
  }));
  expect(overflow.document).toBeLessThanOrEqual(0);
  expect(overflow.body).toBeLessThanOrEqual(0);

  const navigation = page.getByRole("navigation", { name: "主导航" });
  await expect(navigation).toBeVisible();
  const box = await navigation.boundingBox();
  expect(box).not.toBeNull();
  expect((box?.x ?? 0) + (box?.width ?? 0)).toBeLessThanOrEqual(360);

  const results = await new AxeBuilder({ page }).analyze();
  expect(results.violations.filter((item) => ["serious", "critical"].includes(item.impact ?? ""))).toEqual([]);
});

test("blocked deployment dialog stays usable and bounded at 360 px", async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 740 });
  await page.goto("/");
  await page.evaluate(() => {
    const blocked = {
      jobId: "job-blocked", sequence: 7, productId: "9NBLGGH4NNS1",
      packageFamilyName: "Microsoft.WindowsTerminal_8wekyb3d8bbwe", title: "Windows Terminal",
      stage: "awaiting_process_exit", bytesDone: 100, bytesTotal: 100, deploymentProgress: 35,
      version: "1.2.3.4", architecture: "x64", language: "en-US",
      allowedControls: ["retry_deployment", "cancel"],
      error: { code: "package_in_use", messageKey: "errors.packageInUse", retry: "retry", jobId: "job-blocked", details: [] },
      blockedProcesses: [{ pid: 420, name: "Terminal.exe" }], updatedAt: 7,
    };
    window.__YAMS_TEST_CLIENT__.listJobs = async () => [blocked];
    window.__YAMS_TEST_CLIENT__.terminateJobPackageProcesses = async () => ({
      matched: [{ pid: 420, name: "Terminal.exe" }, { pid: 421, name: "OpenConsole.exe" }],
      terminated: [],
      remaining: [{ pid: 420, name: "Terminal.exe" }, { pid: 421, name: "OpenConsole.exe" }],
    });
  });

  await page.getByRole("button", { name: "队列" }).click();
  const dialog = page.getByRole("alertdialog", { name: "结束相关应用进程" });
  await expect(dialog).toBeVisible();
  await dialog.getByRole("button", { name: "结束相关进程" }).click();
  await expect(dialog.getByText("OpenConsole.exe (PID 421)")).toBeVisible();

  const box = await dialog.boundingBox();
  expect(box).not.toBeNull();
  expect(box?.x ?? -1).toBeGreaterThanOrEqual(0);
  expect((box?.x ?? 0) + (box?.width ?? 0)).toBeLessThanOrEqual(360);
  const results = await new AxeBuilder({ page }).include("[role=alertdialog]").analyze();
  expect(results.violations.filter((item) => ["serious", "critical"].includes(item.impact ?? ""))).toEqual([]);
});
