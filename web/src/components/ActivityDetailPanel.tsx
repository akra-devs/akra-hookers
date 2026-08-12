import { useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";

import type { ActivityConversationTurn, ActivityTime, ApiClient } from "../api";
import { formatActivityTime } from "../time";
import { useTimelineAnchor } from "../useTimelineAnchor";
import { UiIcon } from "./UiIcon";

type ActivityDetailPanelProps = {
  activityId: number;
  client: ApiClient;
  onClose: () => void;
};

function DetailTime({
  label,
  testId,
  time,
}: {
  label: string;
  testId: string;
  time: ActivityTime;
}) {
  return (
    <div data-testid={testId} data-provenance={time.provenance}>
      <dt>{label}</dt>
      <dd>
        <time dateTime={time.value ?? undefined}>{formatActivityTime(time)}</time>
      </dd>
    </div>
  );
}

export function ActivityDetailPanel({
  activityId,
  client,
  onClose,
}: ActivityDetailPanelProps) {
  const panelRef = useRef<HTMLElement>(null);
  const detailQuery = useQuery({
    queryKey: ["activity", activityId],
    queryFn: () => client.activity(activityId),
    retry: false,
    refetchInterval: (query) =>
      query.state.status === "success" ? 500 : false,
  });
  const detail = detailQuery.data;
  const [technicalOpen, setTechnicalOpen] = useState(false);
  const [copyStatus, setCopyStatus] = useState("");
  const [additionalTurns, setAdditionalTurns] = useState<ActivityConversationTurn[]>([]);
  const [pageHasMore, setPageHasMore] = useState<boolean | null>(null);
  const [loadingMore, setLoadingMore] = useState(false);
  const [pageError, setPageError] = useState("");
  useEffect(() => {
    panelRef.current?.focus({ preventScroll: false });
  }, [activityId]);
  useEffect(() => {
    setAdditionalTurns([]);
    setPageHasMore(null);
    setPageError("");
  }, [activityId, detail?.conversation_total]);
  const pageTurns = detail
    ? [...detail.conversation, ...additionalTurns].filter(
      (turn, index, turns) => turns.findIndex(({ id }) => id === turn.id) === index,
    )
    : [];
  const timelineTurns = detail
    ? [...pageTurns, detail.selected_turn].filter(
      (turn, index, turns) => turns.findIndex(({ id }) => id === turn.id) === index,
    )
    : [];
  const timelineKey = timelineTurns.map(({ id }) => id).join(":");
  const timelineRef = useTimelineAnchor(activityId, timelineKey);
  const loadMore = async () => {
    const cursor = pageTurns.at(-1)?.id;
    if (!detail || cursor === undefined) return;
    setLoadingMore(true);
    setPageError("");
    try {
      const page = await client.activity(activityId, { limit: 100, afterId: cursor });
      setAdditionalTurns((current) => [...current, ...page.conversation]);
      setPageHasMore(page.conversation_has_more);
    } catch (cause) {
      setPageError(cause instanceof Error ? cause.message : "대화 기록을 더 불러오지 못했습니다.");
    } finally {
      setLoadingMore(false);
    }
  };
  const copyTechnicalValue = async (label: string, value: string) => {
    try {
      await navigator.clipboard.writeText(value);
      setCopyStatus(`${label} 복사됨`);
    } catch {
      setCopyStatus(`${label}을 복사하지 못했습니다.`);
    }
  };

  if (detailQuery.isError) {
    return (
      <aside
        ref={panelRef}
        className="activity-detail"
        data-testid="activity-detail-panel"
        data-selected-activity-id={activityId}
        aria-label="활동 상세"
        tabIndex={-1}
      >
        <header className="activity-detail__header">
          <div>
            <p className="eyebrow">ACTIVITY DETAIL</p>
            <h2>활동 상세</h2>
          </div>
          <button type="button" onClick={onClose} aria-label="상세 닫기" title="상세 닫기">
            <UiIcon name="close" />
          </button>
        </header>
        <p className="inline-error" role="alert">
          활동 상세를 불러오지 못했습니다.
        </p>
        <div className="inline-actions">
          <button
            type="button"
            onClick={() => void detailQuery.refetch()}
          >
            다시 시도
          </button>
        </div>
      </aside>
    );
  }

  if (!detail) {
    return (
      <aside
        ref={panelRef}
        className="activity-detail"
        data-testid="activity-detail-panel"
        data-selected-activity-id={activityId}
        aria-label="활동 상세"
        tabIndex={-1}
      >
        <p>활동 상세를 불러오는 중입니다.</p>
      </aside>
    );
  }

  return (
    <aside
      ref={panelRef}
      className="activity-detail"
      data-testid="activity-detail-panel"
      data-selected-activity-id={activityId}
      aria-label="활동 상세"
      tabIndex={-1}
    >
      <header className="activity-detail__header">
        <div>
          <p className="eyebrow">ACTIVITY DETAIL</p>
          <h2>활동 상세</h2>
        </div>
        <button type="button" onClick={onClose} aria-label="상세 닫기" title="상세 닫기">
          <UiIcon name="close" />
        </button>
      </header>
      <section className="activity-detail__selected" aria-label="선택한 활동">
        <span className="activity-detail__project">{detail.project?.name ?? "Inbox"}</span>
        <p>{detail.prompt}</p>
        <span className="activity-detail__provider">{detail.provider}</span>
      </section>
      <dl className="activity-detail__facts">
        <DetailTime label="수집 시각" testId="captured-at" time={detail.captured_at} />
        <DetailTime label="최초 기록 시각" testId="first-recorded-at" time={detail.first_recorded_at} />
        <div>
          <dt>제출 경로</dt>
          <dd className="activity-detail__path" data-testid="submitted-cwd">
            {detail.submitted_cwd ?? "정확한 작업 경로를 사용할 수 없음"}
          </dd>
        </div>
        <div>
          <dt>감지 경로</dt>
          <dd
            className="activity-detail__path"
            data-testid="detected-path"
            data-resolution-source={detail.origin.resolution_source}
          >
            {detail.origin.display_path}
          </dd>
        </div>
      </dl>
      <details
        className="activity-detail__technical"
        onToggle={(event) => setTechnicalOpen(event.currentTarget.open)}
      >
        <summary
          onKeyDown={(event) => {
            if (event.key !== " " && event.key !== "Enter") return;
            event.preventDefault();
            const details = event.currentTarget.parentElement;
            if (details instanceof HTMLDetailsElement) details.open = !details.open;
          }}
        >
          기술 정보
        </summary>
        {technicalOpen && <dl>
          <div>
            <dt>세션 ID</dt>
            <dd>
              <code>{detail.technical.session_id}</code>
              <button
                type="button"
                onClick={() => void copyTechnicalValue("세션 ID", detail.technical.session_id)}
              >
                세션 ID 복사
              </button>
            </dd>
          </div>
          <div>
            <dt>턴 ID</dt>
            <dd>
              <code>{detail.technical.turn_id}</code>
              <button
                type="button"
                onClick={() => void copyTechnicalValue("턴 ID", detail.technical.turn_id)}
              >
                턴 ID 복사
              </button>
            </dd>
          </div>
        </dl>}
        <span className="sr-only" aria-live="polite">{copyStatus}</span>
      </details>
      <section className="activity-detail__timeline" aria-labelledby="conversation-heading">
        <h3 id="conversation-heading">
          대화 기록 ({timelineTurns.length}/{detail.conversation_total})
        </h3>
        <ol ref={timelineRef} aria-label="대화 기록">
          {timelineTurns.map((turn) => (
            <li
              key={turn.id}
              className={turn.selected ? "activity-detail__turn is-selected" : "activity-detail__turn"}
              data-activity-id={turn.id}
              aria-current={turn.selected ? "true" : undefined}
            >
              <div>
                <span>{turn.project?.name ?? "Inbox"}</span>
                {!turn.on_canvas && <span>캔버스에 없음</span>}
              </div>
              <p>{turn.prompt}</p>
              <time dateTime={turn.time.value ?? undefined}>{formatActivityTime(turn.time)}</time>
            </li>
          ))}
        </ol>
        {pageError && <p className="inline-error" role="alert">{pageError}</p>}
        {(pageHasMore ?? detail.conversation_has_more) && (
          <button type="button" disabled={loadingMore} onClick={() => void loadMore()}>
            {loadingMore ? "불러오는 중…" : "대화 기록 더 보기"}
          </button>
        )}
      </section>
    </aside>
  );
}
