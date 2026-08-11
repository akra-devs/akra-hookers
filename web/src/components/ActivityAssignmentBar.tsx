import { useState, type FormEvent } from "react";

import type {
  ActivityAssignmentRequest,
  ProjectDestination,
  ProjectSummary,
} from "../api";
import type { AssignmentSelection } from "../assignment-selection";

type Props = {
  selection: AssignmentSelection;
  projects: ProjectSummary[];
  currentProjectId: number | null;
  onAssign: (request: ActivityAssignmentRequest) => Promise<void>;
  onMoveOrigin: (originId: number) => void;
};

export function ActivityAssignmentBar({
  selection,
  projects,
  currentProjectId,
  onAssign,
  onMoveOrigin,
}: Props) {
  const [destinationKind, setDestinationKind] = useState<"existing" | "new" | "inbox">(
    projects.length > 0 ? "existing" : "new",
  );
  const [projectId, setProjectId] = useState(projects[0]?.id ?? 0);
  const [newProjectName, setNewProjectName] = useState("");
  const [futureRoute, setFutureRoute] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (selection.state === "empty" || selection.state === "loading") return null;
  if (selection.state === "dedicated") {
    return (
      <aside className="assignment-bar assignment-bar--guardrail">
        <strong>전용 작업 위치 활동입니다.</strong>
        <span>개별 활동 대신 작업 위치의 전체 기록을 이동합니다.</span>
        <button type="button" onClick={() => onMoveOrigin(selection.originId)}>
          작업 위치 이동
        </button>
      </aside>
    );
  }
  if (selection.state === "blocked") {
    return (
      <aside className="assignment-bar assignment-bar--guardrail">
        <strong>선택한 활동을 함께 배정할 수 없습니다.</strong>
        <span>공유 작업 위치의 활동만 다시 선택해 주세요.</span>
      </aside>
    );
  }

  const destination = (): ProjectDestination | null => {
    if (destinationKind === "inbox") return null;
    if (destinationKind === "new") return { new_project_name: newProjectName };
    return { project_id: projectId };
  };
  const save = async (request: ActivityAssignmentRequest) => {
    setBusy(true);
    setError(null);
    try {
      await onAssign(request);
      setFutureRoute(false);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "활동을 배정하지 못했습니다.");
    } finally {
      setBusy(false);
    }
  };
  const submit = (event: FormEvent) => {
    event.preventDefault();
    void save({
        activity_ids: [...selection.activityIds].sort((left, right) => left - right),
      destination: destination(),
      future_route: futureRoute && destinationKind !== "inbox" ? "set" : "unchanged",
    });
  };
  const invalid = busy
    || (destinationKind === "existing" && projectId === 0)
    || (destinationKind === "new" && newProjectName.trim().length === 0);

  return (
    <aside className="assignment-bar" role="region" aria-label="프로젝트에 배정">
      <form onSubmit={submit}>
        <div className="assignment-bar__heading">
          <strong>선택한 활동 {selection.activityIds.length}개</strong>
          <span>공유 작업 위치에서 선택한 기록만 이동합니다.</span>
          {error && <p className="inline-error assignment-bar__error" role="alert">{error}</p>}
        </div>
        <fieldset className="assignment-options">
          <legend>배정 대상</legend>
          {projects.length > 0 && (
            <label>
              <input
                type="radio"
                name="assignment-destination"
                checked={destinationKind === "existing"}
                onChange={() => setDestinationKind("existing")}
              />
              기존 프로젝트
            </label>
          )}
          <label>
            <input
              type="radio"
              name="assignment-destination"
              checked={destinationKind === "new"}
              onChange={() => setDestinationKind("new")}
            />
            새 프로젝트
          </label>
          <label>
            <input
              type="radio"
              name="assignment-destination"
              checked={destinationKind === "inbox"}
              onChange={() => {
                setDestinationKind("inbox");
                setFutureRoute(false);
              }}
            />
            분류 필요
          </label>
        </fieldset>
        {destinationKind === "existing" && (
          <div className="assignment-field">
            <label htmlFor="assignment-project">프로젝트</label>
            <select
              id="assignment-project"
              value={projectId}
              onChange={(event) => setProjectId(Number(event.target.value))}
            >
              {projects.map((project) => (
                <option key={project.id} value={project.id}>{project.name}</option>
              ))}
            </select>
          </div>
        )}
        {destinationKind === "new" && (
          <label className="assignment-field">
            새 프로젝트 이름
            <input value={newProjectName} onChange={(event) => setNewProjectName(event.target.value)} />
          </label>
        )}
        {selection.futureRoute && destinationKind !== "inbox" && (
          <label className="assignment-route">
            <input
              type="checkbox"
              checked={futureRoute}
              onChange={(event) => setFutureRoute(event.target.checked)}
            />
            이 대화의 이후 활동도 이 프로젝트에 배정
          </label>
        )}
        <div className="assignment-actions">
          <button type="submit" disabled={invalid}>배정 저장</button>
          {selection.futureRoute && currentProjectId !== null && (
            <button
              type="button"
              disabled={busy}
              onClick={() => void save({
      activity_ids: [...selection.activityIds].sort((left, right) => left - right),
                destination: { project_id: currentProjectId },
                future_route: "clear",
              })}
            >
              이후 활동 배정 해제
            </button>
          )}
        </div>
      </form>
    </aside>
  );
}
