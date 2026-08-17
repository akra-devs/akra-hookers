import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";

import type {
  ApiClient,
  ActivityPeriod,
  CurationApplyResult,
  CurationLog,
  CurationLogState,
  CurationProposal,
  CurationProposalGroup,
  ProjectSummary,
} from "../api";
import { formatActivityTime } from "../time";
import { UiIcon } from "./UiIcon";

type CurationWorkspaceProps = {
  client: ApiClient;
  project: ProjectSummary;
  onCancel: () => void;
  onApplied: (result: CurationApplyResult) => Promise<void>;
  onFinish: (workIds: number[]) => void;
};

type Step = "select" | "review" | "complete";

function visibleRequest(log: CurationLog) {
  return log.prompt_summary.text ?? log.prompt;
}

function moveLog(
  groups: CurationProposalGroup[],
  activityId: number,
  targetIndex: number,
) {
  return groups.map((group, index) => ({
    ...group,
    log_ids: index === targetIndex
      ? [...group.log_ids.filter((id) => id !== activityId), activityId]
      : group.log_ids.filter((id) => id !== activityId),
  }));
}

function CurationLogRow({
  log,
  checked,
  disabled,
  confirmingDelete,
  regenerating,
  onChecked,
  onExclude,
  onDelete,
  onCancelDelete,
  onRegenerate,
}: {
  log: CurationLog;
  checked: boolean;
  disabled: boolean;
  confirmingDelete: boolean;
  regenerating: boolean;
  onChecked: (checked: boolean) => void;
  onExclude: () => void;
  onDelete: () => void;
  onCancelDelete: () => void;
  onRegenerate: () => void;
}) {
  const checkboxId = `curation-log-${log.id}`;
  const resultStatus = regenerating
    ? "로딩..."
    : log.result_summary.status === "pending"
      ? "결과 요약 중"
      : log.result_summary.status === "failed"
        ? "결과 요약 실패"
        : "결과 없음";
  return (
    <li className={checked ? "curation-log is-selected" : "curation-log"}>
      <div className="curation-log__select">
        <input
          id={checkboxId}
          type="checkbox"
          checked={checked}
          disabled={disabled}
          onChange={(event) => onChecked(event.target.checked)}
          aria-label={`${visibleRequest(log)} 선택`}
        />
        <span className="curation-log__body">
          <span className="curation-log__heading">
            <label htmlFor={checkboxId}><strong>{visibleRequest(log)}</strong></label>
            <time dateTime={log.time.value ?? undefined}>{formatActivityTime(log.time)}</time>
          </span>
          <span className="curation-log__result-row">
            {log.result_summary.status === "ready" ? (
              <span className="curation-log__result">
                <b>RES</b>
                {log.result_summary.lines[0]}
                <em>+2</em>
              </span>
            ) : (
              <span className={`curation-log__result is-${log.result_summary.status}`}>
                <b>RES</b>
                {resultStatus}
              </span>
            )}
            {(log.result_summary.can_regenerate || regenerating) && (
              <button
                type="button"
                className="curation-log__regenerate"
                disabled={regenerating}
                onClick={onRegenerate}
                aria-label={`${visibleRequest(log)} 결과 요약 재생성`}
                title="보관 중인 응답으로 결과 요약 재생성"
              >
                <UiIcon name="refresh" size={13} />
                {regenerating ? "로딩..." : "재생성"}
              </button>
            )}
          </span>
        </span>
      </div>
      <div className="curation-log__actions">
        {confirmingDelete ? (
          <>
            <span>복구 없이 숨길까요?</span>
            <button type="button" className="is-danger" onClick={onDelete}>삭제 확인</button>
            <button type="button" onClick={onCancelDelete}>취소</button>
          </>
        ) : (
          <>
            {log.state === "organized" ? (
              <span>작업에 포함됨</span>
            ) : (
              <button type="button" onClick={onExclude}>
                {log.state === "excluded" ? "정리 대상으로 복원" : "이번 정리에서 제외"}
              </button>
            )}
            <button
              type="button"
              className="icon-button curation-log__delete"
              onClick={onDelete}
              aria-label="로그 영구 제외"
              title="이 로그를 영구 제외"
            >
              <UiIcon name="trash" size={15} />
            </button>
          </>
        )}
      </div>
      <details className="curation-log__details">
        <summary>
          <span>더보기</span>
          <UiIcon name="chevron-down" size={15} />
        </summary>
        <div className="curation-log__evidence">
          <section>
            <h3>REQ · 수집된 요청 원문</h3>
            <p>{log.prompt}</p>
          </section>
          <section>
            <h3>RES · 저장된 응답</h3>
            {log.result_summary.status === "ready" ? (
              <ol aria-label="저장된 결과 요약">
                {log.result_summary.lines.map((line, index) => (
                  <li key={`${index}-${line}`}>{line}</li>
                ))}
              </ol>
            ) : (
              <p>{resultStatus}</p>
            )}
            <small>응답 원문은 화면에 노출하거나 장기 저장하지 않고 3줄 요약만 보관합니다.</small>
          </section>
        </div>
      </details>
    </li>
  );
}

export function CurationWorkspace({
  client,
  project,
  onCancel,
  onApplied,
  onFinish,
}: CurationWorkspaceProps) {
  const [step, setStep] = useState<Step>("select");
  const [stateFilter, setStateFilter] = useState<CurationLogState>("unreviewed");
  const [period, setPeriod] = useState<ActivityPeriod>("month");
  const [selectedIds, setSelectedIds] = useState<number[]>([]);
  const [confirmingDeleteId, setConfirmingDeleteId] = useState<number | null>(null);
  const [regeneratingIds, setRegeneratingIds] = useState<number[]>([]);
  const [proposal, setProposal] = useState<CurationProposal | null>(null);
  const [groups, setGroups] = useState<CurationProposalGroup[]>([]);
  const [appliedWorkIds, setAppliedWorkIds] = useState<number[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const draggedLogId = useRef<number | null>(null);
  const selectAllRef = useRef<HTMLInputElement>(null);
  const logs = useQuery({
    queryKey: ["curation-logs", project.id, stateFilter, period],
    queryFn: () => client.curationLogs(project.id, stateFilter, period),
    retry: false,
    refetchInterval: (query) => {
      if (step !== "select") return false;
      const hasPending = (query.state.data ?? []).some(
        ({ result_summary }) => result_summary.status === "pending",
      );
      return hasPending || regeneratingIds.length > 0 ? 500 : 2_000;
    },
  });
  const visibleLogs = logs.data ?? [];
  const selectedById = useMemo(
    () => new Map((logs.data ?? []).map((log) => [log.id, log])),
    [logs.data],
  );
  const selectableLogs = useMemo(
    () => visibleLogs.filter(({ state }) => state === "unreviewed").slice(0, 20),
    [visibleLogs],
  );
  const selectedSelectableCount = selectableLogs.filter(({ id }) =>
    selectedIds.includes(id)).length;
  const allSelectableSelected = selectableLogs.length > 0
    && selectedSelectableCount === selectableLogs.length;
  const partiallySelected = selectedSelectableCount > 0 && !allSelectableSelected;
  useLayoutEffect(() => {
    if (!selectAllRef.current) return;
    selectAllRef.current.indeterminate = partiallySelected;
  }, [logs.data, partiallySelected]);
  useEffect(() => {
    if (!logs.data) return;
    const selectable = new Set(
      logs.data.filter(({ state }) => state === "unreviewed").map(({ id }) => id),
    );
    setSelectedIds((current) => {
      const next = current.filter((id) => selectable.has(id));
      return next.length === current.length ? current : next;
    });
  }, [logs.data]);
  useEffect(() => {
    if (!logs.data) return;
    const pending = new Set(
      logs.data
        .filter(({ result_summary }) => result_summary.status === "pending")
        .map(({ id }) => id),
    );
    setRegeneratingIds((current) => current.filter((id) => pending.has(id)));
  }, [logs.data]);

  const updateExcluded = async (log: CurationLog) => {
    setError("");
    try {
      await client.setCurationLogExcluded(log.id, log.state !== "excluded");
      setSelectedIds((current) => current.filter((id) => id !== log.id));
      await logs.refetch();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "로그 상태를 변경하지 못했습니다.");
    }
  };
  const deleteLog = async (log: CurationLog) => {
    if (confirmingDeleteId !== log.id) {
      setConfirmingDeleteId(log.id);
      return;
    }
    setError("");
    try {
      await client.deleteCurationLog(log.id);
      setSelectedIds((current) => current.filter((id) => id !== log.id));
      setConfirmingDeleteId(null);
      await logs.refetch();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "로그를 삭제하지 못했습니다.");
    }
  };
  const regenerateResultSummary = async (log: CurationLog) => {
    if (!log.result_summary.can_regenerate || regeneratingIds.includes(log.id)) return;
    setError("");
    setRegeneratingIds((current) => [...current, log.id]);
    try {
      await client.regenerateResultSummary(log.id);
      await logs.refetch();
    } catch (cause) {
      setRegeneratingIds((current) => current.filter((id) => id !== log.id));
      setError(
        cause instanceof Error
          ? cause.message
          : "보관 중인 응답으로 결과 요약을 다시 만들지 못했습니다.",
      );
    }
  };
  const requestProposal = async () => {
    if (selectedIds.length === 0 || selectedIds.length > 20) return;
    setBusy(true);
    setError("");
    try {
      const next = await client.createCurationProposal(project.id, selectedIds);
      setProposal(next);
      setGroups(next.groups);
      setStep("review");
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "AI 작업 제안을 만들지 못했습니다.");
    } finally {
      setBusy(false);
    }
  };
  const applyProposal = async () => {
    if (!proposal) return;
    const activeGroups = groups.filter((group) => group.log_ids.length > 0);
    if (activeGroups.some((group) => group.title.trim().length === 0)) {
      setError("모든 작업 이름을 입력하세요.");
      return;
    }
    setBusy(true);
    setError("");
    try {
      const result = await client.applyCurationProposal(proposal.id, activeGroups);
      setAppliedWorkIds(result.work_ids);
      await onApplied(result);
      setStep("complete");
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "작업 제안을 적용하지 못했습니다.");
    } finally {
      setBusy(false);
    }
  };

  return (
    <section
      className="curation-workspace"
      aria-labelledby="curation-title"
      data-testid="curation-workspace"
    >
      <header className="curation-workspace__header">
        <div>
          <h2 id="curation-title">{project.name} 로그 정리</h2>
          <p>원본 기록은 보존하고, 확인한 묶음만 작업 노드로 승격합니다.</p>
        </div>
        <ol className="curation-steps" aria-label="정리 단계">
          <li aria-current={step === "select" ? "step" : undefined}>로그 선택</li>
          <li aria-current={step === "review" ? "step" : undefined}>AI 제안 검토</li>
          <li aria-current={step === "complete" ? "step" : undefined}>작업 확정</li>
        </ol>
        <button type="button" onClick={onCancel} aria-label="로그 정리 닫기">
          <UiIcon name="close" />
        </button>
      </header>

      {step === "select" && (
        <div className="curation-select">
          <div className="curation-filters" aria-label="로그 필터">
            <label>
              기간
              <select
                value={period}
                onChange={(event) => {
                  setPeriod(event.target.value as ActivityPeriod);
                  setSelectedIds([]);
                }}
              >
                <option value="today">오늘</option>
                <option value="day">24시간 동안</option>
                <option value="week">최근 7일</option>
                <option value="month">최근 30일</option>
                <option value="quarter">최근 90일</option>
                <option value="all">전체 기간</option>
              </select>
            </label>
            <div role="group" aria-label="정리 상태">
              {(["unreviewed", "excluded", "organized"] as CurationLogState[]).map((state) => (
                <button
                  key={state}
                  type="button"
                  className={stateFilter === state ? "is-active" : undefined}
                  aria-pressed={stateFilter === state}
                  onClick={() => {
                    setStateFilter(state);
                    setSelectedIds([]);
                  }}
                >
                  {state === "unreviewed" ? "정리 대기" : state === "excluded" ? "제외됨" : "정리됨"}
                </button>
              ))}
            </div>
            <span>{visibleLogs.length}개 표시</span>
          </div>
          {logs.isError && (
            <div className="curation-state" role="alert">
              <p>정리할 로그를 불러오지 못했습니다.</p>
              <button type="button" onClick={() => void logs.refetch()}>다시 시도</button>
            </div>
          )}
          {!logs.data && !logs.isError && <p className="curation-state">로그를 불러오는 중입니다.</p>}
          {logs.data && visibleLogs.length === 0 && (
            <div className="curation-state" role="status">
              <strong>이 조건에 맞는 로그가 없습니다.</strong>
              <span>기간이나 정리 상태를 바꿔보세요.</span>
            </div>
          )}
          <ul className="curation-log-list">
            {stateFilter === "unreviewed" && visibleLogs.length > 0 && (
              <li className="curation-log-list__bulk">
                <label>
                  <input
                    ref={selectAllRef}
                    type="checkbox"
                    checked={allSelectableSelected}
                    aria-checked={partiallySelected ? "mixed" : allSelectableSelected}
                    disabled={selectableLogs.length === 0}
                    onChange={(event) => setSelectedIds(
                      event.target.checked ? selectableLogs.map(({ id }) => id) : [],
                    )}
                  />
                  <span>
                    <strong>전체 선택</strong>
                    <small>
                      {visibleLogs.length > 20
                        ? `현재 목록의 앞 20개 · ${visibleLogs.length}개 중 최대 선택 수`
                        : `현재 목록 ${selectableLogs.length}개`}
                    </small>
                  </span>
                </label>
              </li>
            )}
            {visibleLogs.map((log) => (
              <CurationLogRow
                key={log.id}
                log={log}
                checked={selectedIds.includes(log.id)}
                disabled={log.state !== "unreviewed" || (!selectedIds.includes(log.id) && selectedIds.length >= 20)}
                confirmingDelete={confirmingDeleteId === log.id}
                regenerating={regeneratingIds.includes(log.id)}
                onChecked={(checked) => setSelectedIds((current) => checked
                  ? [...current, log.id]
                  : current.filter((id) => id !== log.id))}
                onExclude={() => void updateExcluded(log)}
                onDelete={() => void deleteLog(log)}
                onCancelDelete={() => setConfirmingDeleteId(null)}
                onRegenerate={() => void regenerateResultSummary(log)}
              />
            ))}
          </ul>
          <footer className="curation-selection-dock">
            <div>
              <strong>{selectedIds.length}개 선택</strong>
              <span>최대 96자 요청·3줄 결과만 전송 · 전체 원문/응답 제외 · 최대 20개</span>
            </div>
            <button
              type="button"
              disabled={busy || selectedIds.length === 0}
              onClick={() => void requestProposal()}
            >
              <UiIcon name="spark" size={16} />
              {busy ? "작업 후보를 만드는 중…" : `선택한 ${selectedIds.length}개 자동 묶기`}
            </button>
          </footer>
        </div>
      )}

      {step === "review" && proposal && (
        <div className="curation-review">
          <header className="curation-review__intro">
            <div>
              <h3>AI가 제안한 작업 묶음</h3>
              <p>이름을 바꾸거나 로그를 드래그·선택해 옮긴 뒤 적용하세요.</p>
            </div>
            <span>{proposal.cached ? "저장된 동일 제안 재사용" : "Spark 1회 호출"}</span>
          </header>
          <div className="curation-groups">
            {groups.map((group, groupIndex) => (
              <section
                key={`${group.target_work_id ?? "new"}-${groupIndex}`}
                className="curation-group"
                onDragOver={(event) => event.preventDefault()}
                onDrop={(event) => {
                  event.preventDefault();
                  const activityId = draggedLogId.current
                    ?? Number(event.dataTransfer.getData("text/plain"));
                  if (Number.isInteger(activityId)) {
                    setGroups((current) => moveLog(current, activityId, groupIndex));
                  }
                  draggedLogId.current = null;
                }}
              >
                <header>
                  <span className={group.target_work_id === null ? "is-new" : "is-existing"}>
                    {group.target_work_id === null ? "신규 작업" : "기존 작업에 편입"}
                  </span>
                  <label>
                    <span className="sr-only">{groupIndex + 1}번째 작업 이름</span>
                    <input
                      value={group.title}
                      maxLength={80}
                      onChange={(event) => setGroups((current) => current.map((candidate, index) =>
                        index === groupIndex ? { ...candidate, title: event.target.value } : candidate))}
                    />
                  </label>
                  <span className="curation-group__confidence">
                    {group.confidence}%{group.uncertain ? " · 확인 필요" : ""}
                  </span>
                </header>
                <ul>
                  {group.log_ids.map((activityId) => {
                    const log = selectedById.get(activityId);
                    return (
                      <li
                        key={activityId}
                        draggable
                        onDragStart={(event) => {
                          draggedLogId.current = activityId;
                          event.dataTransfer.setData("text/plain", String(activityId));
                        }}
                      >
                        <span><UiIcon name="logs" size={15} /></span>
                        <strong>{log ? visibleRequest(log) : `로그 ${activityId}`}</strong>
                        <label>
                          <span className="sr-only">로그를 옮길 작업</span>
                          <select
                            value={groupIndex}
                            onChange={(event) => setGroups((current) =>
                              moveLog(current, activityId, Number(event.target.value)))}
                          >
                            {groups.map((target, index) => (
                              <option key={`${index}-${target.title}`} value={index}>
                                {target.title || `작업 ${index + 1}`}
                              </option>
                            ))}
                          </select>
                        </label>
                      </li>
                    );
                  })}
                  {group.log_ids.length === 0 && <li className="curation-group__empty">이곳에 로그를 옮기세요.</li>}
                </ul>
              </section>
            ))}
            <button
              type="button"
              className="curation-add-group"
              onClick={() => setGroups((current) => [...current, {
                target_work_id: null,
                title: "새 작업",
                log_ids: [],
                confidence: 0,
                uncertain: true,
              }])}
            >
              <UiIcon name="plus" size={16} /> 새 작업으로 분리
            </button>
          </div>
          <footer className="curation-review__footer">
            <p>
              <strong>{selectedIds.length}개 로그 · {groups.filter((group) => group.log_ids.length > 0).length}개 작업</strong>
              <span>AI는 관계선을 만들거나 로그를 삭제하지 않습니다.</span>
            </p>
            <div>
              <button type="button" disabled={busy} onClick={() => setStep("select")}>선택 다시 보기</button>
              <button type="button" disabled={busy} onClick={() => void applyProposal()}>
                {busy ? "적용 중…" : "검토한 제안 적용"}
              </button>
            </div>
          </footer>
        </div>
      )}

      {step === "complete" && (
        <div className="curation-complete" role="status">
          <UiIcon name="work" size={28} />
          <h3>{appliedWorkIds.length}개 작업을 Project Memory에 반영했습니다.</h3>
          <p>
            원본 로그는 각 작업의 근거로 남았습니다. 새 작업은 관계선 없이 시작하며,
            필요한 관계만 캔버스에서 직접 연결할 수 있습니다.
          </p>
          <div>
            <button type="button" onClick={() => onFinish(appliedWorkIds)}>작업 지도에서 보기</button>
          </div>
        </div>
      )}
      {error && <p className="curation-error" role="alert">{error}</p>}
    </section>
  );
}
