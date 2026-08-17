import { useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";

import type { ApiClient, WorkLog } from "../api";
import { formatActivityTime } from "../time";
import { UiIcon } from "./UiIcon";

type WorkDetailPanelProps = {
  client: ApiClient;
  workId: number;
  onClose: () => void;
  onOpenActivity: (activityId: number) => void;
  onChanged: () => Promise<unknown>;
};

function WorkLogItem({
  log,
  removing,
  onOpen,
  onRemove,
}: {
  log: WorkLog;
  removing: boolean;
  onOpen: () => void;
  onRemove: () => void;
}) {
  const request = log.prompt_summary.text ?? log.prompt;
  return (
    <li className="work-detail__log" data-testid={`work-log-${log.id}`}>
      <div className="work-detail__log-heading">
        <span>LOG {log.id}</span>
        <time dateTime={log.time.value ?? undefined}>{formatActivityTime(log.time)}</time>
      </div>
      <p>{request}</p>
      {log.result_summary.status === "ready" && (
        <ol aria-label="결과 요약">
          {log.result_summary.lines.map((line) => <li key={line}>{line}</li>)}
        </ol>
      )}
      {log.prompt_summary.text !== null && log.prompt_summary.text !== log.prompt && (
        <details>
          <summary>수집된 원문 미리보기</summary>
          <p>{log.prompt}</p>
        </details>
      )}
      <div className="work-detail__log-actions">
        <button type="button" onClick={onOpen}>전체 기록 보기</button>
        <button type="button" disabled={removing} onClick={onRemove}>
          {removing ? "분리 중…" : "작업에서 빼기"}
        </button>
      </div>
    </li>
  );
}

export function WorkDetailPanel({
  client,
  workId,
  onClose,
  onOpenActivity,
  onChanged,
}: WorkDetailPanelProps) {
  const panelRef = useRef<HTMLElement>(null);
  const titleRef = useRef<HTMLInputElement>(null);
  const [editingTitle, setEditingTitle] = useState(false);
  const [title, setTitle] = useState("");
  const [savingTitle, setSavingTitle] = useState(false);
  const [removingId, setRemovingId] = useState<number | null>(null);
  const [error, setError] = useState("");
  const detail = useQuery({
    queryKey: ["work-item", workId],
    queryFn: () => client.workItem(workId),
    retry: false,
    refetchInterval: 1_000,
  });
  useEffect(() => panelRef.current?.focus(), [workId]);
  useEffect(() => {
    if (!editingTitle && detail.data) setTitle(detail.data.title);
  }, [detail.data, editingTitle]);
  useEffect(() => {
    if (editingTitle) titleRef.current?.focus();
  }, [editingTitle]);

  const saveTitle = async () => {
    if (!detail.data || title === detail.data.title) {
      setEditingTitle(false);
      return;
    }
    setSavingTitle(true);
    setError("");
    try {
      await client.updateWork(workId, { title });
      await Promise.all([detail.refetch(), onChanged()]);
      setEditingTitle(false);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "작업 이름을 저장하지 못했습니다.");
    } finally {
      setSavingTitle(false);
    }
  };
  const removeLog = async (activityId: number) => {
    setRemovingId(activityId);
    setError("");
    try {
      await client.removeWorkLog(workId, activityId);
      await onChanged();
      if ((detail.data?.logs.length ?? 0) <= 1) {
        onClose();
      } else {
        await detail.refetch();
      }
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "로그를 작업에서 분리하지 못했습니다.");
    } finally {
      setRemovingId(null);
    }
  };

  return (
    <aside
      ref={panelRef}
      className="activity-detail work-detail"
      data-testid="work-detail-panel"
      aria-label="작업 상세"
      tabIndex={-1}
    >
      <header className="activity-detail__header">
        <div>
          <span className="work-detail__type">Project memory</span>
          <h2>작업 상세</h2>
        </div>
        <button type="button" onClick={onClose} aria-label="작업 상세 닫기" title="작업 상세 닫기">
          <UiIcon name="close" />
        </button>
      </header>
      {detail.isError && (
        <div className="work-detail__state" role="alert">
          <p>작업 상세를 불러오지 못했습니다.</p>
          <button type="button" onClick={() => void detail.refetch()}>다시 시도</button>
        </div>
      )}
      {!detail.data && !detail.isError && <p className="work-detail__state">작업을 불러오는 중입니다.</p>}
      {detail.data && (
        <>
          <section className="work-detail__identity">
            <span>{detail.data.project.name}</span>
            {editingTitle ? (
              <form
                onSubmit={(event) => {
                  event.preventDefault();
                  void saveTitle();
                }}
              >
                <label htmlFor="work-detail-title">작업 이름</label>
                <input
                  ref={titleRef}
                  id="work-detail-title"
                  value={title}
                  maxLength={80}
                  onChange={(event) => setTitle(event.target.value)}
                />
                <div>
                  <button type="submit" disabled={savingTitle || title.trim().length === 0}>
                    {savingTitle ? "저장 중…" : "저장"}
                  </button>
                  <button type="button" onClick={() => setEditingTitle(false)}>취소</button>
                </div>
              </form>
            ) : (
              <div>
                <h3>{detail.data.title}</h3>
                <button type="button" onClick={() => setEditingTitle(true)}>이름 편집</button>
              </div>
            )}
            <p>사용자가 확인한 로그 {detail.data.log_count}개로 구성된 작업입니다.</p>
          </section>
          <section className="work-detail__sources" aria-labelledby="work-sources-heading">
            <header>
              <h3 id="work-sources-heading">근거 로그</h3>
              <span>{detail.data.logs.length}</span>
            </header>
            <ol>
              {detail.data.logs.map((log) => (
                <WorkLogItem
                  key={log.id}
                  log={log}
                  removing={removingId === log.id}
                  onOpen={() => onOpenActivity(log.id)}
                  onRemove={() => void removeLog(log.id)}
                />
              ))}
            </ol>
          </section>
        </>
      )}
      {error && <p className="inline-error" role="alert">{error}</p>}
    </aside>
  );
}
