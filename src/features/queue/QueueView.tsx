import { CirclePause, CirclePlay, OctagonX, XCircle } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { ConfirmDialog } from "../../components/ui/alert-dialog";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Progress } from "../../components/ui/progress";
import { jobControlLabel, jobStageLabel, localizeError, safeErrorLabel } from "../../lib/i18n";
import { newCommandId, replayJobEvents, shouldReplayHint, type StoreClient } from "../../lib/tauri";
import type { JobControl, JobSnapshot } from "../../lib/types";

interface QueueViewProps {
  client: StoreClient;
  seedJobs: JobSnapshot[];
  onJobsChanged: (jobs: JobSnapshot[]) => void;
}

type QueueFilter = "active" | "completed" | "all";
const terminalStages = new Set(["completed", "failed", "cancelled"]);

export function QueueView({ client, seedJobs, onJobsChanged }: QueueViewProps) {
  const [jobs, setJobs] = useState(() => new Map(seedJobs.map((job) => [job.jobId, job])));
  const jobsRef = useRef(jobs);
  const cursorRef = useRef<number | null>(null);
  const [filter, setFilter] = useState<QueueFilter>("active");
  const [status, setStatus] = useState<"loading" | "ready" | "error">("loading");
  const [error, setError] = useState<string | null>(null);
  const [terminatingJobId, setTerminatingJobId] = useState<string | null>(null);

  function replaceJobs(next: Map<string, JobSnapshot>) {
    jobsRef.current = next;
    setJobs(next);
    onJobsChanged([...next.values()]);
  }

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    client.listJobs().then((snapshots) => {
      if (!active) return;
      replaceJobs(new Map(snapshots.map((job) => [job.jobId, job])));
      setStatus("ready");
    }).catch((value) => {
      if (active) { setError(localizeError(value)); setStatus("error"); }
    });
    client.subscribeJobChanges(async (hint) => {
      const current = jobsRef.current.get(hint.jobId);
      if (!active || !shouldReplayHint(hint, current)) return;
      try {
        const merged = await replayJobEvents(client, jobsRef.current, cursorRef.current);
        if (!active) return;
        cursorRef.current = merged.cursor;
        replaceJobs(merged.jobs);
      } catch (value) {
        if (active) setError(localizeError(value));
      }
    }).then((stop) => { if (active) unlisten = stop; else stop(); });
    return () => { active = false; unlisten?.(); };
  }, [client]);

  const visibleJobs = useMemo(() => [...jobs.values()]
    .filter((job) => filter === "all" || (filter === "completed" ? terminalStages.has(job.stage) : !terminalStages.has(job.stage)))
    .sort((left, right) => right.updatedAt - left.updatedAt), [filter, jobs]);

  async function control(job: JobSnapshot, action: JobControl) {
    try {
      const updated = await client.requestJobControl({
        jobId: job.jobId,
        expectedSequence: job.sequence,
        control: action,
        commandId: newCommandId(),
      });
      const next = new Map(jobsRef.current);
      if ((next.get(updated.jobId)?.sequence ?? -1) <= updated.sequence) next.set(updated.jobId, updated);
      replaceJobs(next);
    } catch (value) {
      setError(localizeError(value));
    }
  }

  async function terminateAndRetry(job: JobSnapshot) {
    setTerminatingJobId(job.jobId);
    setError(null);
    try {
      const result = await client.terminateJobPackageProcesses(job.jobId);
      if (result.failed > 0) {
        setError(`仍有 ${result.failed} 个相关进程无法结束，请关闭应用后重试。`);
        return;
      }
      await control(job, "resume");
    } catch (value) {
      setError(localizeError(value));
    } finally {
      setTerminatingJobId(null);
    }
  }

  return (
    <section className="view" aria-labelledby="queue-heading">
      <header className="view-header">
        <div><p className="eyebrow">后台任务</p><h1 id="queue-heading">安装队列</h1></div>
      </header>
      <div className="filter-tabs" role="group" aria-label="任务筛选">
        {(["active", "completed", "all"] as const).map((value) => (
          <button key={value} type="button" aria-pressed={filter === value} onClick={() => setFilter(value)}>
            {value === "active" ? "进行中" : value === "completed" ? "已结束" : "全部"}
          </button>
        ))}
      </div>
      <div className="sr-only" aria-live="polite">队列中有 {visibleJobs.length} 个任务</div>
      {error && <div className="inline-alert" role="alert">{error}</div>}
      {status === "loading" && <div role="status">正在加载任务...</div>}
      {status !== "loading" && visibleJobs.length === 0 && <div className="empty-state"><strong>没有任务</strong></div>}
      <div className="queue-list">
        {visibleJobs.map((job) => <JobRow key={job.jobId} job={job} onControl={control} onTerminateAndRetry={terminateAndRetry} terminating={terminatingJobId === job.jobId} />)}
      </div>
    </section>
  );
}

function JobRow({ job, onControl, onTerminateAndRetry, terminating }: {
  job: JobSnapshot;
  onControl: (job: JobSnapshot, control: JobControl) => void;
  onTerminateAndRetry: (job: JobSnapshot) => void | Promise<void>;
  terminating: boolean;
}) {
  const downloadPercent = job.bytesTotal && job.bytesTotal > 0 ? Math.round(job.bytesDone / job.bytesTotal * 100) : 0;
  const progress = job.stage === "deploying" ? (job.deploymentProgress ?? 0) : downloadPercent;
  const progressKind = job.stage === "deploying" ? "安装" : "下载";
  const icons = { pause: CirclePause, resume: CirclePlay, cancel: XCircle } as const;
  return (
    <article className="queue-item" aria-label={`${job.title ?? job.productId}，${jobStageLabel(job.stage)}`}>
      <div className="queue-item__main">
        <div><strong>{job.title ?? job.productId}</strong><span>{job.packageFamilyName ?? "正在解析包身份"}</span></div>
        <Badge className={`stage stage--${job.stage}`}>{jobStageLabel(job.stage)}</Badge>
      </div>
      {(job.stage === "downloading" || job.stage === "deploying") && <div className="queue-progress"><Progress value={progress} label={`${job.title ?? job.productId} ${progressKind}进度`} /><span>{progress}%</span></div>}
      {safeErrorLabel(job.error) && <p className="job-error">{safeErrorLabel(job.error)}</p>}
      <div className="queue-item__footer">
        <span>序列 {job.sequence}{job.version ? ` · ${job.version}` : ""}</span>
        <div className="row-actions">
          {job.error?.code === "package_in_use" && <ConfirmDialog
            trigger={<Button compact variant="danger" disabled={terminating}><OctagonX aria-hidden="true" size={16} />{terminating ? "正在结束..." : "结束相关进程并重试"}</Button>}
            title="结束相关应用进程"
            description="这会强制结束属于该应用包的进程，未保存的数据可能丢失。系统进程和其他应用不会被结束。"
            confirmLabel="确认结束"
            destructive
            onConfirm={() => onTerminateAndRetry(job)}
          />}
          {job.allowedControls.map((action) => {
            const Icon = icons[action];
            return <Button key={action} compact variant={action === "cancel" ? "ghost" : "secondary"} onClick={() => void onControl(job, action)}><Icon aria-hidden="true" size={16} />{jobControlLabel(action)}</Button>;
          })}
        </div>
      </div>
    </article>
  );
}
