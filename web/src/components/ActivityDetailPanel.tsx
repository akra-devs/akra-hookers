import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useQuery } from "@tanstack/react-query";

import type {
  ActivityConversationTurn,
  ActivityPromptSummary,
  ActivityResultSummary,
  ActivityTime,
  ApiClient,
} from "../api";
import {
  isActivityKindVisible,
  type ActivityVisibility,
} from "../activity-visibility";
import { formatActivityTime } from "../time";
import { useTimelineAnchor } from "../useTimelineAnchor";
import { ActivityDeleteDialog } from "./ActivityDeleteDialog";
import { UiIcon } from "./UiIcon";
import { useDialogFocus } from "./useDialogFocus";

type ActivityDetailPanelProps = {
  activityId: number;
  activityVisibility: ActivityVisibility;
  client: ApiClient;
  onClose: () => void;
  onDeleted: (activityId: number) => void;
  onSelectActivity: (activityId: number) => void;
};

const CONVERSATION_PAGE_SIZE = 8;
let bodyScrollLockCount = 0;
let bodyOverflowBeforeLock = "";

function useBodyScrollLock() {
  useEffect(() => {
    if (bodyScrollLockCount === 0) {
      bodyOverflowBeforeLock = document.body.style.overflow;
      document.body.style.overflow = "hidden";
    }
    bodyScrollLockCount += 1;
    return () => {
      bodyScrollLockCount = Math.max(0, bodyScrollLockCount - 1);
      if (bodyScrollLockCount === 0) {
        document.body.style.overflow = bodyOverflowBeforeLock;
      }
    };
  }, []);
}

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

function ResultSummary({
  summary,
  regenerating,
  regenerationError,
  onRegenerate,
}: {
  summary: ActivityResultSummary;
  regenerating: boolean;
  regenerationError: string;
  onRegenerate: () => void;
}) {
  return (
    <section
      className={`activity-detail__result activity-detail__result--${summary.status}`}
      data-testid="activity-result-summary"
      data-status={summary.status}
      aria-labelledby="result-summary-heading"
      aria-busy={summary.status === "pending" || regenerating || undefined}
    >
      <div className="activity-detail__result-heading">
        <h3 id="result-summary-heading">결과 요약</h3>
        {(summary.can_regenerate || regenerating) && (
          <button
            type="button"
            disabled={regenerating}
            onClick={onRegenerate}
            aria-label="결과 요약 재생성"
            title="보관 중인 응답으로 결과 요약 재생성"
          >
            <UiIcon name="refresh" size={13} />
            {regenerating ? "로딩..." : "재생성"}
          </button>
        )}
      </div>
      {regenerating && <p aria-live="polite">로딩...</p>}
      {!regenerating && summary.status === "ready" && (
        <ol>
          {summary.lines.map((line, index) => <li key={`${index}-${line}`}>{line}</li>)}
        </ol>
      )}
      {!regenerating && summary.status === "pending" && (
        <p aria-live="polite">Codex Spark가 결과를 요약하는 중입니다.</p>
      )}
      {!regenerating && summary.status === "failed" && <p>결과 요약을 만들지 못했습니다.</p>}
      {!regenerating && summary.status === "unavailable" && <p>저장된 결과 요약이 없습니다.</p>}
      {regenerationError && <p className="inline-error" role="alert">{regenerationError}</p>}
    </section>
  );
}

function TimelineResultSummary({ summary }: { summary: ActivityResultSummary }) {
  if (summary.status === "unavailable") return null;
  if (summary.status === "pending") {
    return <p className="activity-detail__turn-result-state">RES · 결과 요약 중</p>;
  }
  if (summary.status === "failed") {
    return <p className="activity-detail__turn-result-state is-failed">RES · 결과 요약 실패</p>;
  }
  if (summary.status !== "ready") return null;
  return (
    <p className="activity-detail__turn-result">
      <strong>RES</strong>
      <span>{summary.lines[0]}</span>
      <em>+2</em>
    </p>
  );
}

function ExpandedTimelineResultSummary({ summary }: { summary: ActivityResultSummary }) {
  let content;
  if (summary.status === "ready") {
    content = (
      <span className="activity-conversation-dialog__result-lines">
        {summary.lines.map((line, index) => (
          <span key={`${index}-${line}`}>{line}</span>
        ))}
      </span>
    );
  } else if (summary.status === "pending") {
    content = <span className="is-pending">결과를 요약하는 중입니다.</span>;
  } else if (summary.status === "failed") {
    content = <span className="is-failed">결과 요약을 만들지 못했습니다.</span>;
  } else {
    content = <span className="is-unavailable">저장된 결과 요약이 없습니다.</span>;
  }

  return (
    <div className="activity-conversation-dialog__result">
      <strong>RES</strong>
      {content}
    </div>
  );
}

function promptSummaryLabel(summary: ActivityPromptSummary): string | null {
  if (summary.status === "pending") return "요청 정리 중";
  if (summary.status === "failed") return "원문 표시";
  if (summary.status !== "ready") return null;
  if (summary.mode === "contextual") return "문맥 보강";
  if (summary.mode === "standalone") return "요청 요약";
  if (summary.mode === "passthrough") return "원문 정리";
  return null;
}

function RequestSummary({
  summary,
  prompt,
  compact = false,
}: {
  summary: ActivityPromptSummary;
  prompt: string;
  compact?: boolean;
}) {
  const text = summary.text ?? prompt;
  const label = promptSummaryLabel(summary);
  return (
    <>
      {label && (
        <span
          className={`activity-detail__request-status activity-detail__request-status--${summary.status}`}
          data-status={summary.status}
          aria-live={compact ? undefined : "polite"}
        >
          {label}
        </span>
      )}
      {compact ? (
        <p className="activity-detail__turn-request">
          <strong>REQ</strong>
          <span>{text}</span>
        </p>
      ) : <ExpandablePrompt text={text} ariaLive="polite" />}
    </>
  );
}

function ExpandablePrompt({
  text,
  ariaLive,
  className = "",
}: {
  text: string;
  ariaLive?: "polite";
  className?: string;
}) {
  const contentId = useId();
  const contentRef = useRef<HTMLParagraphElement>(null);
  const [expanded, setExpanded] = useState(false);
  const [canExpand, setCanExpand] = useState(false);

  useLayoutEffect(() => {
    setExpanded(false);
    const content = contentRef.current;
    if (!content) return;
    const measure = () => {
      const lineHeight = Number.parseFloat(window.getComputedStyle(content).lineHeight);
      const collapsedHeight = Number.isFinite(lineHeight)
        ? lineHeight * 4
        : content.clientHeight;
      setCanExpand(content.scrollHeight > collapsedHeight + 1);
    };
    const frame = window.requestAnimationFrame(measure);
    const observer = new ResizeObserver(measure);
    observer.observe(content);
    return () => {
      window.cancelAnimationFrame(frame);
      observer.disconnect();
    };
  }, [text]);

  return (
    <div className={`expandable-prompt${expanded ? " is-expanded" : ""}${className ? ` ${className}` : ""}`}>
      <p ref={contentRef} id={contentId} aria-live={ariaLive}>{text}</p>
      {canExpand && (
        <button
          type="button"
          className="expandable-prompt__toggle"
          aria-controls={contentId}
          aria-expanded={expanded}
          onClick={() => setExpanded((current) => !current)}
        >
          {expanded ? "접기" : "더 보기"}
        </button>
      )}
    </div>
  );
}

function RawPromptDisclosure({ prompt }: { prompt: string }) {
  return (
    <details className="activity-detail__raw-prompt">
      <summary>수집된 원문 보기</summary>
      <p>{prompt}</p>
    </details>
  );
}

function hasDerivedPrompt(summary: ActivityPromptSummary, prompt: string) {
  return summary.text !== null && summary.text !== prompt;
}

type ConversationFlowDialogProps = {
  activityId: number;
  activityVisibility: ActivityVisibility;
  client: ApiClient;
  conversationIndex: number;
  conversationTotal: number;
  provider: string;
  refreshKey: number;
  onClose: () => void;
  onDeleteRequest: (turn: ActivityConversationTurn) => void;
};

function ConversationFlowDialog({
  activityId,
  activityVisibility,
  client,
  conversationIndex,
  conversationTotal,
  provider,
  refreshKey,
  onClose,
  onDeleteRequest,
}: ConversationFlowDialogProps) {
  const dialogRef = useRef<HTMLElement>(null);
  const initialPage = Math.max(0, Math.floor((conversationIndex - 1) / CONVERSATION_PAGE_SIZE));
  const [pageIndex, setPageIndex] = useState(initialPage);
  const pageQuery = useQuery({
    queryKey: [
      "activity-conversation-page",
      activityId,
      pageIndex,
      activityVisibility,
      refreshKey,
    ],
    queryFn: () => client.activity(activityId, {
      limit: CONVERSATION_PAGE_SIZE,
      offset: pageIndex * CONVERSATION_PAGE_SIZE,
      includeInternal: activityVisibility.internal,
    }),
    retry: false,
    refetchInterval: (query) => query.state.status === "success" ? 500 : false,
  });
  const turns = pageQuery.data?.conversation ?? [];
  const total = pageQuery.data?.conversation_total ?? conversationTotal;
  const pageCount = Math.max(1, Math.ceil(total / CONVERSATION_PAGE_SIZE));
  const timelineKey = `${pageIndex}:${turns.map(({ id }) => id).join(":")}`;
  const timelineRef = useTimelineAnchor(activityId, timelineKey);
  useDialogFocus(dialogRef, "[data-conversation-dialog-close]");
  useBodyScrollLock();

  useEffect(() => {
    if (pageIndex < pageCount) return;
    setPageIndex(pageCount - 1);
  }, [pageCount, pageIndex]);

  return createPortal(
    <div className="dialog-backdrop activity-conversation-dialog__backdrop">
      <section
        ref={dialogRef}
        id="conversation-flow-dialog"
        className="dialog-card activity-conversation-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="conversation-flow-title"
        aria-describedby="conversation-flow-description"
        onKeyDown={(event) => {
          if (event.key !== "Escape") return;
          event.stopPropagation();
          onClose();
        }}
      >
        <header className="dialog-heading activity-conversation-dialog__heading">
          <div>
            <h2 id="conversation-flow-title">대화 흐름</h2>
            <p id="conversation-flow-description">
              <span>{provider}</span>
              오래된 기록부터 · {pageIndex + 1}/{pageCount} 페이지 · 총 {total}개
            </p>
          </div>
          <button
            data-conversation-dialog-close
            type="button"
            onClick={onClose}
            aria-label="대화 흐름 닫기"
            title="대화 흐름 닫기"
          >
            <UiIcon name="close" />
          </button>
        </header>

        {pageQuery.isError ? (
          <div className="activity-conversation-dialog__page-state" role="alert">
            <p>이 페이지의 대화 기록을 불러오지 못했습니다.</p>
            <button type="button" onClick={() => void pageQuery.refetch()}>다시 시도</button>
          </div>
        ) : pageQuery.isPending ? (
          <div className="activity-conversation-dialog__page-state" aria-live="polite">
            대화 기록을 불러오는 중입니다.
          </div>
        ) : (
          <ol
            ref={timelineRef}
            className="activity-conversation-dialog__timeline"
            aria-label="확대된 대화 기록"
          >
            {turns.map((turn) => {
              const summaryLabel = promptSummaryLabel(turn.prompt_summary);
              return (
                <li
                  key={turn.id}
                  className={turn.selected ? "is-selected" : undefined}
                  data-activity-id={turn.id}
                  aria-current={turn.selected ? "true" : undefined}
                >
                  <div className="activity-conversation-dialog__turn-heading">
                    <span className="activity-conversation-dialog__project">
                      {turn.project?.name ?? "Inbox"}
                    </span>
                    <span className="activity-conversation-dialog__turn-meta">
                      {turn.selected && <span className="is-selected-label">선택됨</span>}
                      {!turn.on_canvas && <span>캔버스에 없음</span>}
                      <time dateTime={turn.time.value ?? undefined}>
                        {formatActivityTime(turn.time)}
                      </time>
                      <button
                        type="button"
                        className="activity-conversation-dialog__delete"
                        aria-label={`활동 기록 ${turn.id} 삭제`}
                        title="이 활동 기록 삭제"
                        onClick={() => onDeleteRequest(turn)}
                      >
                        <UiIcon name="trash" size={14} />
                        삭제
                      </button>
                    </span>
                  </div>
                  <div className="activity-conversation-dialog__request">
                    <span className="activity-conversation-dialog__section-heading">
                      <strong>REQ</strong>
                      {summaryLabel && (
                        <span
                          className={`activity-detail__request-status activity-detail__request-status--${turn.prompt_summary.status}`}
                          data-status={turn.prompt_summary.status}
                        >
                          {summaryLabel}
                        </span>
                      )}
                    </span>
                    <ExpandablePrompt
                      className="activity-conversation-dialog__prompt"
                      text={turn.prompt_summary.text ?? turn.prompt}
                    />
                  </div>
                  <ExpandedTimelineResultSummary summary={turn.result_summary} />
                </li>
              );
            })}
          </ol>
        )}

        <footer className="activity-conversation-dialog__footer">
          <span aria-live="polite">
            {total === 0
              ? "기록 없음"
              : `${pageIndex * CONVERSATION_PAGE_SIZE + 1}–${Math.min(
                (pageIndex + 1) * CONVERSATION_PAGE_SIZE,
                total,
              )} / ${total}`}
          </span>
          <div className="activity-conversation-dialog__pagination">
            <button
              type="button"
              disabled={pageIndex === 0 || pageQuery.isFetching}
              onClick={() => setPageIndex((current) => Math.max(0, current - 1))}
            >
              이전
            </button>
            <strong>{pageIndex + 1} / {pageCount}</strong>
            <button
              type="button"
              disabled={pageIndex + 1 >= pageCount || pageQuery.isFetching}
              onClick={() => setPageIndex((current) => Math.min(pageCount - 1, current + 1))}
            >
              다음
            </button>
          </div>
        </footer>
      </section>
    </div>,
    document.body,
  );
}

export function ActivityDetailPanel({
  activityId,
  activityVisibility,
  client,
  onClose,
  onDeleted,
  onSelectActivity,
}: ActivityDetailPanelProps) {
  const panelRef = useRef<HTMLElement>(null);
  const detailQuery = useQuery({
    queryKey: ["activity", activityId, activityVisibility],
    queryFn: () => client.activity(activityId, {
      includeInternal: activityVisibility.internal,
    }),
    retry: false,
    refetchInterval: (query) =>
      query.state.status === "success" ? 500 : false,
  });
  const detail = detailQuery.data;
  const [technicalOpen, setTechnicalOpen] = useState(false);
  const [copyStatus, setCopyStatus] = useState("");
  const [additionalTurns, setAdditionalTurns] = useState<ActivityConversationTurn[]>([]);
  const [additionalPageCursors, setAdditionalPageCursors] = useState<number[]>([]);
  const [pageHasMore, setPageHasMore] = useState<boolean | null>(null);
  const [loadingMore, setLoadingMore] = useState(false);
  const [pageError, setPageError] = useState("");
  const [conversationExpanded, setConversationExpanded] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<{ id: number; prompt: string } | null>(null);
  const [deletionRevision, setDeletionRevision] = useState(0);
  const [regeneratingResult, setRegeneratingResult] = useState(false);
  const [resultRegenerationError, setResultRegenerationError] = useState("");
  const regenerationAcceptedRef = useRef(false);
  useEffect(() => {
    panelRef.current?.focus({ preventScroll: false });
  }, [activityId]);
  useEffect(() => {
    setAdditionalTurns([]);
    setAdditionalPageCursors([]);
    setPageHasMore(null);
    setPageError("");
  }, [
    activityId,
    activityVisibility.internal,
    detail?.conversation_total,
  ]);
  useEffect(() => {
    regenerationAcceptedRef.current = false;
    setRegeneratingResult(false);
    setResultRegenerationError("");
  }, [activityId]);
  useEffect(() => {
    if (!regeneratingResult || !regenerationAcceptedRef.current || !detail) return;
    if (detail.result_summary.status === "pending") return;
    regenerationAcceptedRef.current = false;
    setRegeneratingResult(false);
  }, [detail, regeneratingResult]);
  const pageTurns = detail
    ? [...detail.conversation, ...additionalTurns].filter(
      (turn, index, turns) => turns.findIndex(({ id }) => id === turn.id) === index,
    ).filter((turn) => isActivityKindVisible(turn.activity_kind, activityVisibility))
    : [];
  const timelineTurns = detail
    ? [...pageTurns, detail.selected_turn].filter(
      (turn, index, turns) => turns.findIndex(({ id }) => id === turn.id) === index,
    ).filter((turn) => isActivityKindVisible(turn.activity_kind, activityVisibility))
    : [];
  const timelineKey = timelineTurns.map(({ id }) => id).join(":");
  const timelineRef = useTimelineAnchor(activityId, timelineKey);
  const loadMore = async () => {
    const cursor = pageTurns.at(-1)?.id;
    if (!detail || cursor === undefined) return;
    setLoadingMore(true);
    setPageError("");
    try {
      const page = await client.activity(activityId, {
        limit: 100,
        afterId: cursor,
        includeInternal: activityVisibility.internal,
      });
      setAdditionalTurns((current) => [...current, ...page.conversation]);
      setAdditionalPageCursors((current) =>
        current.includes(cursor) ? current : [...current, cursor]
      );
      setPageHasMore(page.conversation_has_more);
    } catch (cause) {
      setPageError(cause instanceof Error ? cause.message : "대화 기록을 더 불러오지 못했습니다.");
    } finally {
      setLoadingMore(false);
    }
  };
  useEffect(() => {
    if (additionalPageCursors.length === 0) return;
    let cancelled = false;
    let refreshInFlight = false;
    const refreshAdditionalPages = async () => {
      if (refreshInFlight) return;
      refreshInFlight = true;
      try {
        const pages = await Promise.all(additionalPageCursors.map((afterId) =>
          client.activity(activityId, {
            limit: 100,
            afterId,
            includeInternal: activityVisibility.internal,
          })
        ));
        if (cancelled) return;
        setAdditionalTurns((current) => {
          const byId = new Map(current.map((turn) => [turn.id, turn]));
          for (const turn of pages.flatMap((page) => page.conversation)) {
            byId.set(turn.id, turn);
          }
          return [...byId.values()];
        });
        const lastPage = pages.at(-1);
        if (lastPage) setPageHasMore(lastPage.conversation_has_more);
      } catch {
        // Keep the loaded page visible; the next interval retries its refresh.
      } finally {
        refreshInFlight = false;
      }
    };
    void refreshAdditionalPages();
    const interval = window.setInterval(() => void refreshAdditionalPages(), 500);
    return () => {
      cancelled = true;
      window.clearInterval(interval);
    };
  }, [
    activityId,
    activityVisibility.internal,
    additionalPageCursors,
    client,
  ]);
  const copyTechnicalValue = async (label: string, value: string) => {
    try {
      await navigator.clipboard.writeText(value);
      setCopyStatus(`${label} 복사됨`);
    } catch {
      setCopyStatus(`${label}을 복사하지 못했습니다.`);
    }
  };
  const confirmActivityDelete = async () => {
    if (!deleteTarget) return;
    const deletedId = deleteTarget.id;
    await client.deleteActivity(deletedId);
    setDeleteTarget(null);
    setAdditionalTurns([]);
    setAdditionalPageCursors([]);
    setPageHasMore(null);
    setPageError("");
    setDeletionRevision((current) => current + 1);
    if (deletedId === activityId) {
      setConversationExpanded(false);
      onDeleted(deletedId);
      return;
    }
    void detailQuery.refetch();
  };
  const regenerateResult = async () => {
    if (!detail?.result_summary.can_regenerate || regeneratingResult) return;
    setRegeneratingResult(true);
    setResultRegenerationError("");
    try {
      await client.regenerateResultSummary(detail.id);
      regenerationAcceptedRef.current = true;
      await detailQuery.refetch({ throwOnError: true });
    } catch (cause) {
      regenerationAcceptedRef.current = false;
      setRegeneratingResult(false);
      setResultRegenerationError(
        cause instanceof Error
          ? cause.message
          : "보관 중인 응답으로 결과 요약을 다시 만들지 못했습니다.",
      );
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
        <div className="activity-detail__header-actions">
          <button
            type="button"
            className="activity-detail__delete"
            onClick={() => setDeleteTarget({ id: detail.id, prompt: detail.prompt })}
            aria-label="이 활동 기록 삭제"
            title="이 활동 기록 삭제"
          >
            <UiIcon name="trash" size={15} />
            삭제
          </button>
          <button type="button" onClick={onClose} aria-label="상세 닫기" title="상세 닫기">
            <UiIcon name="close" />
          </button>
        </div>
      </header>
      <div
        className="activity-detail__context"
        role="region"
        aria-label="선택한 활동 정보"
        tabIndex={0}
      >
      <section className="activity-detail__selected" aria-label="선택한 활동">
        <span className="activity-detail__project">{detail.project?.name ?? "Inbox"}</span>
        <RequestSummary summary={detail.prompt_summary} prompt={detail.prompt} />
        {hasDerivedPrompt(detail.prompt_summary, detail.prompt) && (
          <RawPromptDisclosure prompt={detail.prompt} />
        )}
        <span className="activity-detail__provider">{detail.provider}</span>
      </section>
      <ResultSummary
        summary={detail.result_summary}
        regenerating={regeneratingResult}
        regenerationError={resultRegenerationError}
        onRegenerate={() => void regenerateResult()}
      />
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
          {detail.technical.agent_id && (
            <div>
              <dt>Agent ID</dt>
              <dd>
                <code>{detail.technical.agent_id}</code>
                <button
                  type="button"
                  onClick={() => void copyTechnicalValue(
                    "Agent ID",
                    detail.technical.agent_id!,
                  )}
                >
                  Agent ID copy
                </button>
              </dd>
            </div>
          )}
          {detail.technical.agent_type && (
            <div>
              <dt>Agent type</dt>
              <dd><code>{detail.technical.agent_type}</code></dd>
            </div>
          )}
        </dl>}
        <span className="sr-only" aria-live="polite">{copyStatus}</span>
      </details>
      </div>
      <section className="activity-detail__timeline" aria-labelledby="conversation-heading">
        <div className="activity-detail__timeline-heading">
          <h3 id="conversation-heading">
            대화 기록 ({timelineTurns.length}/{detail.conversation_total})
          </h3>
          <button
            type="button"
            className="activity-detail__timeline-expand"
            aria-haspopup="dialog"
            aria-controls="conversation-flow-dialog"
            aria-label="대화 기록 크게 보기"
            onClick={() => setConversationExpanded(true)}
          >
            <UiIcon name="expand" size={15} />
            크게 보기
          </button>
        </div>
        <ol ref={timelineRef} aria-label="대화 기록">
          {timelineTurns.map((turn) => (
            <li
              key={turn.id}
              className={turn.selected ? "activity-detail__turn is-selected" : "activity-detail__turn"}
              data-activity-id={turn.id}
              aria-current={turn.selected ? "true" : undefined}
            >
              <button
                type="button"
                className="activity-detail__turn-button"
                onClick={() => onSelectActivity(turn.id)}
                aria-current={turn.selected ? "true" : undefined}
              >
                <div>
                  <span>{turn.project?.name ?? "Inbox"}</span>
                  {!turn.on_canvas && <span>캔버스에 없음</span>}
                </div>
                <RequestSummary summary={turn.prompt_summary} prompt={turn.prompt} compact />
                <TimelineResultSummary summary={turn.result_summary} />
                <time dateTime={turn.time.value ?? undefined}>{formatActivityTime(turn.time)}</time>
              </button>
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
      {conversationExpanded && (
        <ConversationFlowDialog
          activityId={activityId}
          activityVisibility={activityVisibility}
          client={client}
          conversationIndex={detail.conversation_index}
          conversationTotal={detail.conversation_total}
          provider={detail.provider}
          refreshKey={deletionRevision}
          onClose={() => setConversationExpanded(false)}
          onDeleteRequest={(turn) => setDeleteTarget({ id: turn.id, prompt: turn.prompt })}
        />
      )}
      {deleteTarget && (
        <ActivityDeleteDialog
          activityId={deleteTarget.id}
          prompt={deleteTarget.prompt}
          onCancel={() => setDeleteTarget(null)}
          onConfirm={confirmActivityDelete}
        />
      )}
    </aside>
  );
}
