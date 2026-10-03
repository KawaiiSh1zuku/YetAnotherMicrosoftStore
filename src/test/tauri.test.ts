import { describe, expect, it, vi } from "vitest";
import { applyEventPage, replayJobEvents, shouldReplayHint } from "../lib/tauri";
import { job } from "./fixtures";

describe("job event cursor handling", () => {
  it("does not replay duplicate or older changed hints", () => {
    expect(shouldReplayHint({ jobId: job.jobId, sequence: 4, updatedAt: 21 }, job)).toBe(false);
    expect(shouldReplayHint({ jobId: job.jobId, sequence: 3, updatedAt: 22 }, job)).toBe(false);
    expect(shouldReplayHint({ jobId: job.jobId, sequence: 5, updatedAt: 23 }, job)).toBe(true);
  });

  it("applies ordered event pages without regressing a newer snapshot", () => {
    const current = new Map([[job.jobId, job]]);
    const merged = applyEventPage(current, {
      nextCursor: 18,
      events: [
        { cursor: 17, jobId: job.jobId, sequence: 2, snapshot: { ...job, sequence: 2, stage: "resolving" } },
        { cursor: 18, jobId: job.jobId, sequence: 5, snapshot: { ...job, sequence: 5, stage: "verifying" } },
      ],
    });

    expect(merged.jobs.get(job.jobId)?.sequence).toBe(5);
    expect(merged.jobs.get(job.jobId)?.stage).toBe("verifying");
    expect(merged.cursor).toBe(18);
  });

  it("preserves a known product title when historical event snapshots omit it", () => {
    const current = new Map([[job.jobId, { ...job, title: "Windows Terminal" }]]);
    const merged = applyEventPage(current, {
      nextCursor: 19,
      events: [
        {
          cursor: 19,
          jobId: job.jobId,
          sequence: 5,
          snapshot: { ...job, title: undefined, sequence: 5, stage: "verifying" },
        },
      ],
    });

    expect(merged.jobs.get(job.jobId)?.title).toBe("Windows Terminal");
  });

  it("replays every full cursor page before returning", async () => {
    const listJobEvents = vi
      .fn()
      .mockResolvedValueOnce({
        nextCursor: 2,
        events: [
          { cursor: 1, jobId: job.jobId, sequence: 5, snapshot: { ...job, sequence: 5 } },
          { cursor: 2, jobId: "job-2", sequence: 1, snapshot: { ...job, jobId: "job-2", sequence: 1 } },
        ],
      })
      .mockResolvedValueOnce({
        nextCursor: 3,
        events: [
          { cursor: 3, jobId: job.jobId, sequence: 6, snapshot: { ...job, sequence: 6, stage: "completed" } },
        ],
      });

    const replayed = await replayJobEvents(
      { listJobEvents },
      new Map([[job.jobId, job]]),
      null,
      2,
    );

    expect(listJobEvents).toHaveBeenNthCalledWith(1, { afterCursor: null, limit: 2 });
    expect(listJobEvents).toHaveBeenNthCalledWith(2, { afterCursor: 2, limit: 2 });
    expect(replayed.jobs.get(job.jobId)?.sequence).toBe(6);
    expect(replayed.cursor).toBe(3);
  });
});
