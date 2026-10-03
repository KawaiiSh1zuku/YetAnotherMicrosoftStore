import { Boxes, ListChecks, Search, Settings, Store } from "lucide-react";
import type { ReactNode } from "react";
import { cn } from "../lib/utils";

export type PrimaryView = "search" | "queue" | "installed" | "settings";

interface AppShellProps {
  activeView: PrimaryView | "details";
  onNavigate: (view: PrimaryView) => void;
  activeJobs: number;
  children: ReactNode;
}

const destinations = [
  { id: "search", label: "搜索", icon: Search },
  { id: "queue", label: "队列", icon: ListChecks },
  { id: "installed", label: "已安装", icon: Boxes },
  { id: "settings", label: "设置", icon: Settings },
] as const;

export function AppShell({ activeView, onNavigate, activeJobs, children }: AppShellProps) {
  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand" aria-label="Yet Another Microsoft Store">
          <span className="brand__mark"><Store aria-hidden="true" size={20} /></span>
          <span className="brand__copy"><strong>Yet Another</strong><span>Microsoft Store</span></span>
        </div>
        <PrimaryNavigation activeView={activeView} onNavigate={onNavigate} activeJobs={activeJobs} />
        <p className="sidebar__status"><span className="status-dot" /> 本机任务服务</p>
      </aside>
      <div className="workspace">
        <header className="topbar">
          <div>
            <p className="eyebrow">WINDOWS 应用工作台</p>
            <p className="topbar__title">目录、任务与本机状态</p>
          </div>
          {activeJobs > 0 && <span className="activity-pill" aria-live="polite">{activeJobs} 个任务进行中</span>}
        </header>
        <main id="main-content" className="workspace__main">{children}</main>
      </div>
      <div className="mobile-nav">
        <PrimaryNavigation activeView={activeView} onNavigate={onNavigate} activeJobs={activeJobs} />
      </div>
    </div>
  );
}

function PrimaryNavigation({ activeView, onNavigate, activeJobs }: Omit<AppShellProps, "children">) {
  return (
    <nav className="primary-nav" aria-label="主导航">
      {destinations.map(({ id, label, icon: Icon }) => (
        <button
          key={id}
          type="button"
          className={cn("nav-item", (activeView === id || (activeView === "details" && id === "search")) && "is-active")}
          aria-current={activeView === id || (activeView === "details" && id === "search") ? "page" : undefined}
          aria-label={id === "search" ? "打开搜索" : label}
          onClick={() => onNavigate(id)}
        >
          <Icon aria-hidden="true" size={19} />
          <span>{label}</span>
          {id === "queue" && activeJobs > 0 && <span className="nav-count" aria-label={`${activeJobs} 个活动任务`}>{activeJobs}</span>}
        </button>
      ))}
    </nav>
  );
}
