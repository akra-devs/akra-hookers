import { useEffect, useRef } from "react";
import type {
  CodexCaptureTarget,
  CollectorIntegration,
  OriginSummary,
  ProjectSummary,
} from "../api";
import type { ActivityVisibility } from "../activity-visibility";
import {
  CollectorEndpointControl,
  type CollectorOperation,
} from "./CollectorEndpointControl";
import { PromptSummaryControl } from "./PromptSummaryControl";
import { UiIcon } from "./UiIcon";

export type ProjectFilter = "all" | "inbox" | `project:${number}`;

type ProjectRailProps = {
  nodeCount: number;
  codexEnabled: boolean;
  codexAvailable: boolean;
  codexPending: boolean;
  promptSummaryMode: "off" | "smart";
  promptSummaryPending: boolean;
  promptSummaryError: string | null;
  codexTargets: CodexCaptureTarget[];
  pendingCodexTargetIds: string[];
  captureError: string | null;
  collector: CollectorIntegration;
  collectorOperation: CollectorOperation;
  activityVisibility: ActivityVisibility;
  projects: ProjectSummary[];
  totalProjectCount: number;
  origins: OriginSummary[];
  inboxCount: number;
  filter: ProjectFilter;
  hideEmptyProjects: boolean;
  onCodexChange: (enabled: boolean) => void;
  onPromptSummaryModeChange: (mode: "off" | "smart") => void;
  onCodexTargetChange: (targetId: string, enabled: boolean) => void;
  onCollectorConfigure: (endpoint: string, token?: string) => Promise<void>;
  onCollectorVerify: () => Promise<void>;
  onActivityVisibilityChange: (
    kind: keyof ActivityVisibility,
    visible: boolean,
  ) => void;
  onHideEmptyProjectsChange: (hide: boolean) => void;
  onFilterChange: (filter: ProjectFilter) => void;
  onNewProject: () => void;
  onManageProject: (projectId: number) => void;
  onManageOrigin: (originId: number) => void;
};

function conciseOriginLabel(path: string) {
  const segments = path.split(/[\\/]/).filter(Boolean);
  return segments[segments.length - 1] ?? path;
}

function contextualOriginLabel(path: string) {
  const segments = path.split(/[\\/]/).filter(Boolean);
  return segments.length >= 3
    ? segments.slice(-2).join("/")
    : conciseOriginLabel(path);
}

function environmentLabel(environment: string) {
  switch (environment) {
    case "windows": return "WINDOWS";
    case "wsl": return "WSL";
    case "posix": return "POSIX";
    default: return environment.toUpperCase();
  }
}

function captureTime(capturedAtUs: number) {
  return new Intl.DateTimeFormat("ko-KR", {
    month: "numeric",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(capturedAtUs / 1_000));
}

export function ProjectRail({
  nodeCount,
  codexEnabled,
  codexAvailable,
  codexPending,
  promptSummaryMode,
  promptSummaryPending,
  promptSummaryError,
  codexTargets,
  pendingCodexTargetIds,
  captureError,
  collector,
  collectorOperation,
  activityVisibility,
  projects,
  totalProjectCount,
  origins,
  inboxCount,
  filter,
  hideEmptyProjects,
  onCodexChange,
  onPromptSummaryModeChange,
  onCodexTargetChange,
  onCollectorConfigure,
  onCollectorVerify,
  onActivityVisibilityChange,
  onHideEmptyProjectsChange,
  onFilterChange,
  onNewProject,
  onManageProject,
  onManageOrigin,
}: ProjectRailProps) {
  const masterCapture = useRef<HTMLInputElement>(null);
  const selectedProjectId = filter.startsWith("project:")
    ? Number(filter.slice("project:".length))
    : null;
  const originLabels = origins.map((origin) =>
    conciseOriginLabel(origin.display_path));
  const contextualLabels = origins.map((origin) =>
    contextualOriginLabel(origin.display_path));
  const availableTargetCount = codexTargets.filter((target) => target.available).length;
  const enabledTargetCount = codexTargets.filter((target) => target.enabled).length;
  const captureIsPartial = enabledTargetCount > 0
    && enabledTargetCount < availableTargetCount;
  useEffect(() => {
    if (masterCapture.current) {
      masterCapture.current.indeterminate = captureIsPartial;
    }
  }, [captureIsPartial]);

  return (
    <aside className="rail" aria-label="Workspace navigation">
      <nav className="rail-overview" aria-label="활동 보기">
        <button
          type="button"
          className={filter === "all" ? "is-active" : undefined}
          aria-current={filter === "all" ? "page" : undefined}
          onClick={() => onFilterChange("all")}
        >
          <UiIcon name="activity" />
          <span>All activity</span>
          <small>{nodeCount}</small>
        </button>
        <button
          type="button"
          className={filter === "inbox" ? "is-active" : undefined}
          aria-current={filter === "inbox" ? "page" : undefined}
          onClick={() => onFilterChange("inbox")}
        >
          <UiIcon name="inbox" />
          <span>Inbox</span>
          <small>{inboxCount}</small>
        </button>
      </nav>

      <section className="rail-section" aria-label="프로젝트">
        <header className="rail-section__heading">
          <h2>Projects</h2>
          <div>
            {selectedProjectId !== null && (
              <button
                className="icon-button"
                type="button"
                onClick={() => onManageProject(selectedProjectId)}
                aria-label="프로젝트 관리"
                title="프로젝트 관리"
              >
                <UiIcon name="settings" size={16} />
              </button>
            )}
            <button
              className="icon-button"
              type="button"
              onClick={onNewProject}
              aria-label="새 프로젝트"
              title="새 프로젝트"
            >
              <UiIcon name="plus" size={17} />
            </button>
          </div>
        </header>
        <label className="project-result-toggle">
          <input
            type="checkbox"
            checked={hideEmptyProjects}
            onChange={(event) => onHideEmptyProjectsChange(event.target.checked)}
          />
          <span>결과 없는 프로젝트 숨기기</span>
        </label>
        <ul className="rail-list rail-projects">
          {hideEmptyProjects && totalProjectCount > 0 && projects.length === 0 && (
            <li className="rail-filter-empty" role="status">
              선택한 기간에 결과가 있는 프로젝트가 없습니다.
            </li>
          )}
          {projects.map((project) => {
            const projectFilter = `project:${project.id}` as const;
            return (
              <li key={project.id}>
                <button
                  type="button"
                  className={filter === projectFilter ? "is-active" : undefined}
                  aria-current={filter === projectFilter ? "page" : undefined}
                  onClick={() => onFilterChange(projectFilter)}
                >
                  <UiIcon name="folder" />
                  <span>{project.name}</span>
                  <small>{project.activity_count}</small>
                </button>
              </li>
            );
          })}
        </ul>
      </section>

      <section className="rail-section" aria-label="작업 위치">
        <header className="rail-section__heading">
          <h2 id="work-locations-heading" tabIndex={-1}>Work locations</h2>
        </header>
        <nav className="origin-navigation" aria-label="작업 위치">
          <ul>
            {origins.length === 0 && (
              <li className="origin-empty-state" role="status">
                <strong>No work locations yet</strong>
                <span>Enable Codex capture and submit a prompt to discover your first location.</span>
              </li>
            )}
            {origins.map((origin, index) => {
              const label = originLabels[index] ?? "작업 위치";
              const matchingLabels = originLabels.filter(
                (candidate) => candidate === label,
              );
              const contextualLabel = contextualLabels[index] ?? label;
              const matchingContexts = contextualLabels.filter(
                (candidate) => candidate === contextualLabel,
              );
              const ordinal = contextualLabels
                .slice(0, index + 1)
                .filter((candidate) => candidate === contextualLabel)
                .length;
              const displayLabel = matchingLabels.length > 1
                ? matchingContexts.length > 1
                  ? `${contextualLabel} · ${ordinal}`
                  : contextualLabel
                : label;
              return (
                <li key={origin.id}>
                  <button type="button" onClick={() => onManageOrigin(origin.id)}>
                    <UiIcon name="location" />
                    <span>
                      <strong>
                        {origin.setup_state === "unconfirmed"
                          ? "작업 위치 설정"
                          : "작업 위치 관리"}
                      </strong>
                      <span>{displayLabel}</span>
                      <small>
                        {origin.setup_state === "unconfirmed" ? "설정 필요" : "확인됨"}
                        {" · "}
                        {origin.routing_mode === "shared"
                          ? "공유 위치"
                          : origin.default_project_name ?? "전용 프로젝트"}
                      </small>
                    </span>
                    <b>{origin.activity_count}</b>
                  </button>
                </li>
              );
            })}
          </ul>
        </nav>
      </section>

      <section className="rail-section activity-visibility" aria-labelledby="activity-visibility-heading">
        <header className="rail-section__heading">
          <div>
            <h2 id="activity-visibility-heading">Canvas visibility</h2>
            <p>Show or hide Codex internal activity nodes.</p>
          </div>
        </header>
        <div className="activity-visibility__options">
          <label>
            <span>
              <strong>Codex internal activity</strong>
              <small id="internal-visibility-description">
                Ambient suggestions and background checks
              </small>
            </span>
            <input
              type="checkbox"
              checked={activityVisibility.internal}
              aria-describedby="internal-visibility-description"
              onChange={(event) =>
                onActivityVisibilityChange("internal", event.target.checked)}
            />
          </label>
        </div>
      </section>

      <section
        id="capture-settings"
        className="provider-control"
        aria-label="Provider settings"
        aria-busy={codexPending || promptSummaryPending || pendingCodexTargetIds.length > 0}
      >
        <header className="rail-section__heading">
          <div>
            <h2>Codex capture</h2>
            <p className="capture-summary" role="status">
              {codexAvailable
                ? `${availableTargetCount}개 중 ${enabledTargetCount}개 hook 설치`
                : "Codex 설치를 확인하는 중…"}
            </p>
          </div>
          <label className="provider-master">
            <span className="sr-only">Codex capture</span>
            <input
              id="codex-capture-control"
              ref={masterCapture}
              type="checkbox"
              checked={codexEnabled}
              aria-checked={captureIsPartial ? "mixed" : codexEnabled}
              disabled={
                !codexAvailable
                || codexPending
                || pendingCodexTargetIds.length > 0
              }
              onChange={(event) => onCodexChange(event.target.checked)}
            />
          </label>
        </header>
        <PromptSummaryControl
          mode={promptSummaryMode}
          available={codexAvailable}
          collectorManaged={collector.mode === "remote"}
          pending={promptSummaryPending}
          error={promptSummaryError}
          onChange={onPromptSummaryModeChange}
        />
        <CollectorEndpointControl
          collector={collector}
          available={codexAvailable}
          operation={collectorOperation}
          onConfigure={onCollectorConfigure}
          onVerify={onCollectorVerify}
        />
        <div className="capture-target-list" role="list" aria-label="감지된 Codex 설치">
          {codexAvailable && codexTargets.length === 0 && (
            <p className="capture-target-empty">감지된 Codex 설치가 없습니다.</p>
          )}
          {codexTargets.map((target) => {
            const pending = pendingCodexTargetIds.includes(target.id);
            const path = target.hook_path ?? target.codex_home ?? "Hook 경로 정보 없음";
            const activation = target.activation
              ?? (target.enabled ? "awaiting_capture" : "disabled");
            const clients = target.clients ?? [];
            const status = pending
              ? "변경하는 중…"
              : !target.available
                ? "설정 확인 필요"
                : target.enabled
                  ? activation === "verified"
                    ? "Capture verified"
                    : "Hook installed · 캡처 미확인"
                  : "Hook not installed";
            const statusTone = target.enabled
              ? activation === "verified" ? " is-verified" : " is-awaiting"
              : "";
            return (
              <div className="capture-target" role="listitem" key={target.id}>
                <label className="capture-target__toggle">
                  <UiIcon name="terminal" />
                  <span className="capture-target__identity">
                    <strong>{target.label}</strong>
                    <small>{environmentLabel(target.environment)}</small>
                  </span>
                  <input
                    type="checkbox"
                    aria-label={`${target.label} capture`}
                    checked={target.enabled}
                    disabled={
                      !codexAvailable
                      || !target.available
                      || pending
                      || codexPending
                    }
                    onChange={(event) =>
                      onCodexTargetChange(target.id, event.target.checked)}
                  />
                </label>
                <p className={`capture-target__status${statusTone}`}>
                  <span aria-hidden="true" />
                  {status}
                </p>
                {target.enabled && clients.length > 0 && (
                  <ul
                    className="capture-client-list"
                    aria-label={`${target.label} 클라이언트별 캡처 상태`}
                  >
                    {clients.map((client) => {
                      const capturedAt = client.last_captured_at_us;
                      const isoTime = capturedAt === null
                        ? undefined
                        : new Date(capturedAt / 1_000).toISOString();
                      return (
                        <li className={client.verified ? "is-verified" : undefined} key={client.id}>
                          <strong>{client.label}</strong>
                          <span>
                            {client.verified && capturedAt !== null ? (
                              <>
                                캡처 확인 · <time dateTime={isoTime}>{captureTime(capturedAt)}</time>
                              </>
                            ) : "설치 후 캡처 없음"}
                          </span>
                        </li>
                      );
                    })}
                  </ul>
                )}
                <code title={path}>{path}</code>
                {target.detail && (
                  <small className="capture-target__detail">{target.detail}</small>
                )}
              </div>
            );
          })}
        </div>
        {captureError && (
          <p className="capture-control-error" role="alert">{captureError}</p>
        )}
        {codexTargets.length > 0 && (
          <small className="capture-restart-note">
            Windows App과 CLI는 같은 hook 파일을 공유합니다. Akra가 현재 hook
            정의만 자동으로 신뢰하므로 설치 후 각 Codex만 재시작하세요.
          </small>
        )}
      </section>
    </aside>
  );
}
