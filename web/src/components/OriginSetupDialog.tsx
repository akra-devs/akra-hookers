import { useRef, useState, type FormEvent } from "react";

import type {
  ApiClient,
  OriginSummary,
  ProjectSummary,
} from "../api";
import {
  DedicatedDestinationFields,
  OriginModeChoices,
  type DestinationKind,
  type OriginMode,
} from "./OriginSetupFields";
import { useDialogFocus } from "./useDialogFocus";

type OriginSetupDialogProps = {
  client: ApiClient;
  origin: OriginSummary;
  projects: ProjectSummary[];
  onClose: () => void;
  onChanged: () => Promise<void>;
};

function suggestedProjectName(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, "");
  return trimmed.split(/[\\/]/).at(-1) || "새 프로젝트";
}

export function OriginSetupDialog({
  client,
  origin,
  projects,
  onClose,
  onChanged,
}: OriginSetupDialogProps) {
  const initialMode = origin.setup_state === "unconfirmed"
    ? origin.recommended_mode
    : origin.routing_mode;
  const suggestion = suggestedProjectName(origin.display_path);
  const hasSuggestedProject =
    origin.setup_state === "unconfirmed"
    && origin.routing_mode === "dedicated"
    && origin.default_project_id !== null;
  const [mode, setMode] = useState<OriginMode>(initialMode);
  const [destinationKind, setDestinationKind] = useState<DestinationKind>(
    hasSuggestedProject
      ? "suggested"
      : origin.default_project_id === null
        ? "new"
        : "existing",
  );
  const [projectName, setProjectName] = useState(
    origin.default_project_name ?? suggestion,
  );
  const projectNameInput = useRef<HTMLInputElement>(null);
  const [projectId, setProjectId] = useState(
    origin.default_project_id ?? projects[0]?.id ?? 0,
  );
  const [confirmedMove, setConfirmedMove] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const dialogRef = useRef<HTMLElement>(null);
  useDialogFocus(dialogRef, "[data-dialog-initial]");
  const changesDedicatedDestination =
    destinationKind === "new"
    || (
      destinationKind === "existing"
      && projectId !== origin.default_project_id
    );
  const requiresMoveConfirmation =
    mode === "dedicated"
    && origin.activity_count > 0
    && (
      origin.routing_mode !== "dedicated"
      || changesDedicatedDestination
    );

  async function save(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      if (mode === "shared") {
        await client.configureOrigin(origin.id, {
          mode: "shared",
          confirm: true,
        });
      } else if (
        destinationKind === "suggested"
        && origin.default_project_id !== null
      ) {
        await client.configureOrigin(origin.id, {
          mode: "dedicated",
          destination: { new_project_name: projectName },
          confirm: true,
        });
      } else {
        await client.configureOrigin(origin.id, {
          mode: "dedicated",
          destination: destinationKind === "existing"
            ? { project_id: projectId }
            : { new_project_name: projectName },
          confirm: true,
        });
      }
      await onChanged();
      onClose();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "작업 위치를 저장하지 못했습니다.");
    } finally {
      setBusy(false);
    }
  }

  const invalidDestination = mode === "dedicated" && (
    destinationKind === "new" || destinationKind === "suggested"
      ? projectName.trim().length === 0
      : projectId === 0
  );

  return (
    <div className="dialog-backdrop">
      <section
        ref={dialogRef}
        className="dialog-card dialog-card--wide"
        role="dialog"
        aria-modal="true"
        aria-labelledby="origin-dialog-title"
      >
        <div className="dialog-heading">
          <div>
            <p className="eyebrow">WORK LOCATION</p>
            <h2 id="origin-dialog-title">이 작업 위치를 어떻게 사용할까요?</h2>
          </div>
          <button type="button" onClick={onClose}>닫기</button>
        </div>
        <div className="origin-path">
          <span>감지한 전체 경로</span>
          <code>{origin.display_path}</code>
        </div>
        <form className="dialog-form" onSubmit={(event) => void save(event)}>
          <OriginModeChoices
            initialMode={initialMode}
            mode={mode}
            onChange={setMode}
          />
          {mode === "dedicated" && (
            <DedicatedDestinationFields
              defaultProjectName={origin.default_project_name}
              destinationKind={destinationKind}
              hasSuggestedProject={hasSuggestedProject}
              inputRef={projectNameInput}
              projectId={projectId}
              projectName={projectName}
              projects={projects}
              suggestedName={suggestion}
              onDestinationKindChange={setDestinationKind}
              onProjectIdChange={setProjectId}
              onProjectNameChange={setProjectName}
            />
          )}
          {requiresMoveConfirmation && (
            <div className="confirmation-box">
              <p>기존 활동 {origin.activity_count}개와 이후 활동이 이동합니다.</p>
              <label>
                <input
                  type="checkbox"
                  checked={confirmedMove}
                  onChange={(event) => setConfirmedMove(event.target.checked)}
                />
                기존 활동 이동을 확인합니다
              </label>
            </div>
          )}
          {error && <p className="inline-error" role="alert">{error}</p>}
          <footer className="dialog-actions">
            <button
              type="submit"
              disabled={busy || invalidDestination || (requiresMoveConfirmation && !confirmedMove)}
            >
              설정 저장
            </button>
          </footer>
        </form>
      </section>
    </div>
  );
}
