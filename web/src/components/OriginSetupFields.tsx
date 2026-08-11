import type { RefObject } from "react";

import type { ProjectSummary } from "../api";

export type OriginMode = "dedicated" | "shared";
export type DestinationKind = "suggested" | "new" | "existing";

type OriginModeChoicesProps = {
  initialMode: OriginMode;
  mode: OriginMode;
  onChange: (mode: OriginMode) => void;
};

export function OriginModeChoices({
  initialMode,
  mode,
  onChange,
}: OriginModeChoicesProps) {
  return (
    <fieldset className="choice-grid">
      <legend>사용 방식</legend>
      <label>
        <input
          data-dialog-initial={initialMode === "dedicated" ? "true" : undefined}
          type="radio"
          name="origin-mode"
          checked={mode === "dedicated"}
          onChange={() => onChange("dedicated")}
        />
        <span>
          <strong>한 프로젝트 전용 폴더</strong>
          <small>기존 활동과 이후 활동을 한 프로젝트에 연결합니다.</small>
        </span>
      </label>
      <label>
        <input
          data-dialog-initial={initialMode === "shared" ? "true" : undefined}
          type="radio"
          name="origin-mode"
          checked={mode === "shared"}
          onChange={() => onChange("shared")}
        />
        <span>
          <strong>여러 프로젝트가 함께 쓰는 위치</strong>
          <small>활동마다 직접 분류하며 기본 프로젝트를 두지 않습니다.</small>
        </span>
      </label>
    </fieldset>
  );
}

type ProjectNameFieldsProps = {
  inputRef: RefObject<HTMLInputElement | null>;
  projectName: string;
  resetName: string;
  onProjectNameChange: (name: string) => void;
};

function ProjectNameFields({
  inputRef,
  projectName,
  resetName,
  onProjectNameChange,
}: ProjectNameFieldsProps) {
  return (
    <div className="nested-fields">
      <label className="field-label">
        새 프로젝트 이름
        <input
          ref={inputRef}
          value={projectName}
          onChange={(event) => onProjectNameChange(event.target.value)}
        />
      </label>
      <div className="inline-actions">
        <button type="button" onClick={() => onProjectNameChange(resetName)}>
          추천 이름 사용
        </button>
        <button
          type="button"
          onClick={() => {
            inputRef.current?.focus();
            inputRef.current?.select();
          }}
        >
          이름 직접 바꾸기
        </button>
      </div>
    </div>
  );
}

type DedicatedDestinationFieldsProps = {
  defaultProjectName: string | null;
  destinationKind: DestinationKind;
  hasSuggestedProject: boolean;
  inputRef: RefObject<HTMLInputElement | null>;
  projectId: number;
  projectName: string;
  projects: ProjectSummary[];
  suggestedName: string;
  onDestinationKindChange: (kind: DestinationKind) => void;
  onProjectIdChange: (projectId: number) => void;
  onProjectNameChange: (name: string) => void;
};

export function DedicatedDestinationFields({
  defaultProjectName,
  destinationKind,
  hasSuggestedProject,
  inputRef,
  projectId,
  projectName,
  projects,
  suggestedName,
  onDestinationKindChange,
  onProjectIdChange,
  onProjectNameChange,
}: DedicatedDestinationFieldsProps) {
  return (
    <fieldset className="choice-grid">
      <legend>프로젝트 연결</legend>
      {hasSuggestedProject && (
        <>
          <label>
            <input
              type="radio"
              name="destination-kind"
              checked={destinationKind === "suggested"}
              onChange={() => onDestinationKindChange("suggested")}
            />
            추천 프로젝트 사용
          </label>
          {destinationKind === "suggested" && (
            <ProjectNameFields
              inputRef={inputRef}
              projectName={projectName}
              resetName={defaultProjectName ?? suggestedName}
              onProjectNameChange={onProjectNameChange}
            />
          )}
        </>
      )}
      <label>
        <input
          type="radio"
          name="destination-kind"
          checked={destinationKind === "new"}
          onChange={() => onDestinationKindChange("new")}
        />
        새 프로젝트 만들기
      </label>
      {destinationKind === "new" && (
        <ProjectNameFields
          inputRef={inputRef}
          projectName={projectName}
          resetName={suggestedName}
          onProjectNameChange={onProjectNameChange}
        />
      )}
      {projects.length > 0 && (
        <label>
          <input
            type="radio"
            name="destination-kind"
            checked={destinationKind === "existing"}
            onChange={() => onDestinationKindChange("existing")}
          />
          기존 프로젝트 연결
        </label>
      )}
      {destinationKind === "existing" && (
        <label className="field-label nested-fields">
          연결할 프로젝트
          <select
            value={projectId}
            onChange={(event) => onProjectIdChange(Number(event.target.value))}
          >
            {projects.map((project) => (
              <option key={project.id} value={project.id}>{project.name}</option>
            ))}
          </select>
        </label>
      )}
    </fieldset>
  );
}
