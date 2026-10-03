import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { axe } from "vitest-axe";
import App from "../App";
import { catalogProduct, createClient, settings } from "./fixtures";

describe("M6 desktop workbench", () => {
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
