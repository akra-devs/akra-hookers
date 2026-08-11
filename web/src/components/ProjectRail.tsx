import type { OriginSummary, ProjectSummary } from "../api";

export type ProjectFilter = "all" | "inbox" | `project:${number}`;

type ProjectRailProps = {
  nodeCount: number;
  codexEnabled: boolean;
  codexAvailable: boolean;
  projects: ProjectSummary[];
  origins: OriginSummary[];
  inboxCount: number;
  filter: ProjectFilter;
  onCodexChange: (enabled: boolean) => void;
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

export function ProjectRail({
  nodeCount,
  codexEnabled,
  codexAvailable,
  projects,
  origins,
  inboxCount,
  filter,
  onCodexChange,
  onFilterChange,
  onNewProject,
  onManageProject,
  onManageOrigin,
}: ProjectRailProps) {
  const selectedProjectId = filter.startsWith("project:")
    ? Number(filter.slice("project:".length))
    : null;
  const originLabels = origins.map((origin) =>
    conciseOriginLabel(origin.display_path));
  const contextualLabels = origins.map((origin) =>
    contextualOriginLabel(origin.display_path));

  return (
    <aside className="rail">
      <p className="eyebrow">LOCAL ACTIVITY MAP</p>
      <h1>akra-hookers</h1>
      <p className="muted">캔버스 활동 {nodeCount}개</p>
      <section className="provider-control" aria-label="Provider settings">
        <p className="eyebrow">SETTINGS</p>
        <label>
          <span>Codex capture</span>
          <input
            type="checkbox"
            checked={codexEnabled}
            disabled={!codexAvailable}
            onChange={(event) => onCodexChange(event.target.checked)}
          />
        </label>
        <small>다음 Codex 프롬프트부터 수집 설정을 적용합니다.</small>
      </section>
      <section className="rail-section" aria-label="프로젝트">
        <p className="eyebrow">PROJECT</p>
        <label className="field-label">
          <span className="sr-only">프로젝트 필터</span>
          <select
            aria-label="프로젝트 필터"
            value={filter}
            onChange={(event) => onFilterChange(event.target.value as ProjectFilter)}
          >
            <option value="all">All projects</option>
            <option value="inbox">분류 필요 ({inboxCount})</option>
            {projects.map((project) => (
              <option key={project.id} value={`project:${project.id}`}>
                {project.name}
              </option>
            ))}
          </select>
        </label>
        <div className="rail-actions">
          <button type="button" onClick={onNewProject}>새 프로젝트</button>
          {selectedProjectId !== null && (
            <button
              type="button"
              onClick={() => onManageProject(selectedProjectId)}
            >
              프로젝트 관리
            </button>
          )}
        </div>
      </section>
      <section className="rail-section" aria-label="작업 위치">
        <p className="eyebrow">WORK LOCATIONS</p>
        <nav className="origin-navigation" aria-label="작업 위치">
          <ul>
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
                </button>
              </li>
              );
            })}
          </ul>
        </nav>
      </section>
    </aside>
  );
}
