import * as AlertDialogPrimitive from "@radix-ui/react-alert-dialog";
import { CirclePause, CirclePlay, OctagonX, RefreshCw, XCircle } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Progress } from "../../components/ui/progress";
import { jobControlLabel, jobStageLabel, localizeError, safeErrorLabel } from "../../lib/i18n";
import { mergeJobSnapshots, newCommandId, replayJobEvents, shouldReplayHint, type StoreClient } from "../../lib/tauri";
import type { JobControl, JobSnapshot, ProcessDescriptor } from "../../lib/types";

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
  const [dialogTarget, setDialogTarget] = useState<{ jobId: string; sequence: number } | null>(null);
  const [dialogError, setDialogError] = useState<string | null>(null);
  const [remainingProcesses, setRemainingProcesses] = useState<ProcessDescriptor[]>([]);
  const openedBlockedSequences = useRef(new Set<string>());

  function replaceJobs(next: Map<string, JobSnapshot>) {
    jobsRef.current = next;
    setJobs(next);
    onJobsChanged([...next.values()]);
  }

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    let replayChain = Promise.resolve();
    let polling = false;

    const pollTimer = window.setInterval(() => {
      if (
        !active
        || polling
        || ![...jobsRef.current.values()].some((job) => !terminalStages.has(job.stage))
      ) return;
      polling = true;
      void client.listJobs()
        .then((snapshots) => {
          if (active) replaceJobs(mergeJobSnapshots(jobsRef.current, snapshots));
        })
        .catch((value) => {
          if (active) setError(localizeError(value));
        })
        .finally(() => { polling = false; });
    }, 500);

    function enqueueReplay() {
      replayChain = replayChain.then(async () => {
        const merged = await replayJobEvents(client, jobsRef.current, cursorRef.current);
        if (!active) return;
        cursorRef.current = merged.cursor;
        replaceJobs(merged.jobs);
      }).catch((value) => {
        if (active) setError(localizeError(value));
      });
      return replayChain;
    }

    async function initialize() {
      try {
        const stop = await client.subscribeJobChanges((hint) => {
          const current = jobsRef.current.get(hint.jobId);
          if (active && shouldReplayHint(hint, current)) void enqueueReplay();
        });
        if (!active) {
          stop();
          return;
        }
        unlisten = stop;
        const snapshots = await client.listJobs();
        if (!active) return;
        replaceJobs(mergeJobSnapshots(jobsRef.current, snapshots));
        await enqueueReplay();
        if (active) setStatus("ready");
      } catch (value) {
        if (active) {
          setError(localizeError(value));
          setStatus("error");
        }
      }
    }

    void initialize();
    return () => { active = false; window.clearInterval(pollTimer); unlisten?.(); };
  }, [client]);

  const visibleJobs = useMemo(() => [...jobs.values()]
    .filter((job) => filter === "all" || (filter === "completed" ? terminalStages.has(job.stage) : !terminalStages.has(job.stage)))
    .sort((left, right) => right.updatedAt - left.updatedAt), [filter, jobs]);

  const dialogJob = dialogTarget ? jobs.get(dialogTarget.jobId) : undefined;

  useEffect(() => {
    const blocked = [...jobs.values()]
      .filter((candidate) => candidate.stage === "awaiting_process_exit")
      .sort((left, right) => right.sequence - left.sequence)
      .find((candidate) => !openedBlockedSequences.current.has(`${candidate.jobId}:${candidate.sequence}`));
    if (blocked) {
      openedBlockedSequences.current.add(`${blocked.jobId}:${blocked.sequence}`);
      setDialogTarget({ jobId: blocked.jobId, sequence: blocked.sequence });
      setDialogError(null);
      setRemainingProcesses([]);
    }
  }, [jobs]);

  useEffect(() => {
    if (dialogTarget && dialogJob?.stage !== "awaiting_process_exit") {
      setDialogTarget(null);
    }
  }, [dialogJob?.stage, dialogTarget]);

  async function control(job: JobSnapshot, action: JobControl): Promise<boolean> {
    try {
      const updated = await client.requestJobControl({
        jobId: job.jobId,
        expectedSequence: job.sequence,
        control: action,
        commandId: newCommandId(),
      });
      replaceJobs(mergeJobSnapshots(jobsRef.current, [updated]));
      return true;
    } catch (value) {
      setError(localizeError(value));
      return false;
    }
  }

  async function terminateAndRetry(job: JobSnapshot) {
    setTerminatingJobId(job.jobId);
    setDialogError(null);
    setRemainingProcesses([]);
    try {
      const result = await client.terminateJobPackageProcesses(job.jobId);
      if (result.remaining.length > 0) {
        setRemainingProcesses(result.remaining);
        setDialogError("仍有相关进程无法结束。");
        return;
      }
      if (await control(job, "retry_deployment")) setDialogTarget(null);
    } catch (value) {
      setDialogError(localizeError(value));
    } finally {
      setTerminatingJobId(null);
    }
  }

  async function retryDeployment(job: JobSnapshot) {
    setDialogError(null);
    if (await control(job, "retry_deployment")) setDialogTarget(null);
  }

  function openBlockedDialog(job: JobSnapshot) {
    setDialogTarget({ jobId: job.jobId, sequence: job.sequence });
    setDialogError(null);
    setRemainingProcesses([]);
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
        {visibleJobs.map((job) => <JobRow key={job.jobId} job={job} onControl={control} onOpenBlockedDialog={openBlockedDialog} terminating={terminatingJobId === job.jobId} />)}
      </div>
      <ProcessBlockingDialog
        job={dialogJob?.stage === "awaiting_process_exit" ? dialogJob : undefined}
        open={dialogJob?.stage === "awaiting_process_exit"}
        terminating={dialogJob ? terminatingJobId === dialogJob.jobId : false}
        remaining={remainingProcesses}
        error={dialogError}
        onOpenChange={(open) => { if (!open) setDialogTarget(null); }}
        onTerminate={terminateAndRetry}
        onRetry={retryDeployment}
      />
    </section>
  );
}

function JobRow({ job, onControl, onOpenBlockedDialog, terminating }: {
  job: JobSnapshot;
  onControl: (job: JobSnapshot, control: JobControl) => void;
  onOpenBlockedDialog: (job: JobSnapshot) => void;
  terminating: boolean;
}) {
  const downloadPercent = job.bytesTotal && job.bytesTotal > 0 ? Math.round(job.bytesDone / job.bytesTotal * 100) : 0;
  const icons = { pause: CirclePause, resume: CirclePlay, retry_deployment: RefreshCw, cancel: XCircle } as const;
  return (
    <article className="queue-item" aria-label={`${job.title ?? job.productId}，${jobStageLabel(job.stage)}`}>
      <div className="queue-item__main">
        <div><strong>{job.title ?? job.productId}</strong><span>{job.packageFamilyName ?? "正在解析包身份"}</span></div>
        <Badge className={`stage stage--${job.stage}`}>{jobStageLabel(job.stage)}</Badge>
      </div>
      {job.stage === "downloading" && <div className="queue-progress"><Progress value={downloadPercent} label={`${job.title ?? job.productId} 下载进度`} /><span>{downloadPercent}%</span></div>}
      {job.stage === "deploying" && <div className="queue-progress"><Progress value={job.deploymentProgress ?? 0} label={`${job.title ?? job.productId} 安装进度`} /><span>{job.deploymentProgress ?? 0}%</span></div>}
      {safeErrorLabel(job.error) && <p className="job-error">{safeErrorLabel(job.error)}</p>}
      <div className="queue-item__footer">
        <span>序列 {job.sequence}{job.version ? ` · ${job.version}` : ""}</span>
        <div className="row-actions">
          {job.stage === "awaiting_process_exit" && job.error?.code === "package_in_use" && <Button compact variant="danger" disabled={terminating} onClick={() => onOpenBlockedDialog(job)}><OctagonX aria-hidden="true" size={16} />处理占用进程</Button>}
          {job.allowedControls.filter((action) => action !== "retry_deployment").map((action) => {
            const Icon = icons[action];
            return <Button key={action} compact variant={action === "cancel" ? "ghost" : "secondary"} onClick={() => void onControl(job, action)}><Icon aria-hidden="true" size={16} />{jobControlLabel(action)}</Button>;
          })}
        </div>
      </div>
    </article>
  );
}

function ProcessBlockingDialog({ job, open, terminating, remaining, error, onOpenChange, onTerminate, onRetry }: {
  job?: JobSnapshot;
  open: boolean;
  terminating: boolean;
  remaining: ProcessDescriptor[];
  error: string | null;
  onOpenChange: (open: boolean) => void;
  onTerminate: (job: JobSnapshot) => void | Promise<void>;
  onRetry: (job: JobSnapshot) => void | Promise<void>;
}) {
  const displayedProcesses = remaining.length > 0 ? remaining : (job?.blockedProcesses ?? []);
  return (
    <AlertDialogPrimitive.Root open={open} onOpenChange={onOpenChange}>
      <AlertDialogPrimitive.Portal>
        <AlertDialogPrimitive.Overlay className="dialog-overlay" />
        <AlertDialogPrimitive.Content className="dialog-content">
          <AlertDialogPrimitive.Title className="dialog-title">结束相关应用进程</AlertDialogPrimitive.Title>
          <AlertDialogPrimitive.Description className="dialog-description">
            这会强制结束属于该应用包的进程，未保存的数据可能丢失。
          </AlertDialogPrimitive.Description>
          {displayedProcesses.length > 0 && <ul className="process-list" aria-label="占用进程">
            {displayedProcesses.map((process) => <li key={`${process.pid}:${process.name}`}>{process.name} (PID {process.pid})</li>)}
          </ul>}
          {error && <div className="inline-alert" role="alert">{error}</div>}
          <div className="dialog-actions dialog-actions--wrap">
            <AlertDialogPrimitive.Cancel asChild><Button>返回</Button></AlertDialogPrimitive.Cancel>
            <Button variant="secondary" disabled={!job || terminating} onClick={() => { if (job) void onRetry(job); }}><RefreshCw aria-hidden="true" size={16} />重试部署</Button>
            <Button variant="danger" disabled={!job || terminating} onClick={() => { if (job) void onTerminate(job); }}><OctagonX aria-hidden="true" size={16} />{terminating ? "正在结束..." : "结束相关进程"}</Button>
          </div>
        </AlertDialogPrimitive.Content>
      </AlertDialogPrimitive.Portal>
    </AlertDialogPrimitive.Root>
  );
}
