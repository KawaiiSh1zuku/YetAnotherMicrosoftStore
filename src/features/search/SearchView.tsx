import { ArrowRight, Search } from "lucide-react";
import { useEffect, useRef } from "react";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Input } from "../../components/ui/input";
import { Skeleton } from "../../components/ui/skeleton";
import { localizeError } from "../../lib/i18n";
import type { StoreClient } from "../../lib/tauri";
import type { AppSettings, CatalogProduct } from "../../lib/types";

export interface SearchState {
  query: string;
  results: CatalogProduct[];
  status: "idle" | "loading" | "ready" | "empty" | "error";
  error: string | null;
  recent: string[];
}

interface SearchViewProps {
  client: StoreClient;
  settings: AppSettings | null;
  state: SearchState;
  setState: (state: SearchState | ((current: SearchState) => SearchState)) => void;
  onOpen: (product: CatalogProduct) => void;
  restoreProductId: string | null;
  onFocusRestored: () => void;
}

export function SearchView({
  client,
  settings,
  state,
  setState,
  onOpen,
  restoreProductId,
  onFocusRestored,
}: SearchViewProps) {
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!restoreProductId) return;
    const target = document.querySelector<HTMLButtonElement>(`[data-product-id="${CSS.escape(restoreProductId)}"]`);
    target?.focus();
    onFocusRestored();
  }, [onFocusRestored, restoreProductId]);

  async function submit(query = state.query) {
    const trimmed = query.trim();
    if (!trimmed) {
      inputRef.current?.focus();
      return;
    }
    setState((current) => ({ ...current, query: trimmed, status: "loading", error: null }));
    try {
      const results = await client.searchApps({
        query: trimmed,
        market: settings?.market ?? "US",
        language: settings?.preferredLanguages[0] ?? "en-US",
      });
      setState((current) => ({
        ...current,
        results,
        status: results.length ? "ready" : "empty",
        recent: [trimmed, ...current.recent.filter((item) => item !== trimmed)].slice(0, 4),
      }));
    } catch (error) {
      setState((current) => ({ ...current, status: "error", error: localizeError(error) }));
    }
  }

  return (
    <section className="view" aria-labelledby="search-heading">
      <header className="view-header">
        <div>
          <p className="eyebrow">MICROSOFT STORE 目录</p>
          <h1 id="search-heading">查找 Windows 应用</h1>
        </div>
        <div className="context-badges" aria-label="当前目录上下文">
          <Badge>{settings?.market ?? "--"} 市场</Badge>
          <Badge>{settings?.preferredLanguages[0] ?? "--"}</Badge>
        </div>
      </header>

      <form className="search-form" role="search" onSubmit={(event) => { event.preventDefault(); void submit(); }}>
        <label className="search-field">
          <span className="sr-only">搜索 Microsoft Store</span>
          <Search aria-hidden="true" size={19} />
          <Input
            ref={inputRef}
            type="search"
            value={state.query}
            aria-label="搜索 Microsoft Store"
            placeholder="输入应用名称"
            autoComplete="off"
            onChange={(event) => setState((current) => ({ ...current, query: event.target.value }))}
          />
        </label>
        <Button type="submit" variant="primary">搜索</Button>
      </form>

      {state.recent.length > 0 && (
        <div className="recent-searches" aria-label="最近搜索">
          <span>最近</span>
          {state.recent.map((item) => (
            <button key={item} type="button" onClick={() => void submit(item)}>{item}</button>
          ))}
        </div>
      )}

      <div className="results" aria-live="polite">
        {state.status === "idle" && <EmptyState title="尚无搜索结果" />}
        {state.status === "loading" && (
          <div role="status" aria-label="正在搜索" className="loading-stack">
            <span>正在搜索...</span><Skeleton /><Skeleton /><Skeleton />
          </div>
        )}
        {state.status === "empty" && <EmptyState title="没有找到匹配的应用。" />}
        {state.status === "error" && <div className="inline-alert" role="alert">{state.error}</div>}
        {state.status === "ready" && (
          <div className="result-list" aria-label="搜索结果">
            {state.results.map((product) => (
              <button
                key={product.productId}
                type="button"
                className="result-item"
                data-product-id={product.productId}
                onClick={() => onOpen(product)}
              >
                <span className="app-glyph" aria-hidden="true">{initials(product.title)}</span>
                <span className="result-item__copy">
                  <strong>{product.title}</strong>
                  <span>{product.publisher ?? "发布者未提供"}</span>
                  <span className="result-formats">{product.packageFormats.length ? product.packageFormats.join(" · ") : "包格式待解析"}</span>
                </span>
                <ArrowRight aria-hidden="true" size={18} />
              </button>
            ))}
          </div>
        )}
      </div>
    </section>
  );
}

function initials(title: string): string {
  return title.split(/\s+/).filter(Boolean).slice(0, 2).map((part) => part[0]?.toUpperCase()).join("") || "APP";
}

function EmptyState({ title, detail }: { title: string; detail?: string }) {
  return <div className="empty-state"><strong>{title}</strong>{detail && <p>{detail}</p>}</div>;
}
