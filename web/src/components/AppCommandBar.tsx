import type { CodexCaptureTarget, CollectorIntegration, ProjectSummary } from "../api";
import type { CollectorOperation } from "./CollectorEndpointControl";
import type { ProjectFilter } from "./ProjectRail";
import { UiIcon } from "./UiIcon";

type AppCommandBarProps = {
  filter: ProjectFilter;
  projects: ProjectSummary[];
  inboxCount: number;
  originCount: number;
  codexAvailable: boolean;
  codexTargets: CodexCaptureTarget[];
  collector: CollectorIntegration | undefined;
  collectorOperation: CollectorOperation;
  onFilterChange: (filter: ProjectFilter) => void;
  onOpenWorkLocations: () => void;
  onOpenCaptureSettings: () => void;
};

export function AppCommandBar({
  filter,
  projects,
  inboxCount,
  originCount,
  codexAvailable,
  codexTargets,
  collector,
  collectorOperation,
  onFilterChange,
  onOpenWorkLocations,
  onOpenCaptureSettings,
}: AppCommandBarProps) {
  const availableTargets = codexTargets.filter((target) => target.available);
  const enabledTargets = availableTargets.filter((target) => target.enabled);
  let health = { label: "Healthy", tone: "healthy" };
  if (!codexAvailable || collectorOperation !== null) {
    health = { label: "Checking", tone: "pending" };
  } else if (enabledTargets.length === 0) {
    health = { label: "Off", tone: "off" };
  } else if (collector?.last_error) {
    health = { label: "Delivery issue", tone: "error" };
  } else if ((collector?.pending_count ?? 0) > 0) {
    health = { label: `${collector?.pending_count} queued`, tone: "partial" };
  } else if (collector?.mode === "remote" && collector.connected !== true) {
    health = { label: "Needs check", tone: "partial" };
  } else if (enabledTargets.length < availableTargets.length) {
    health = { label: "Partial", tone: "partial" };
  }

  return (
    <header className="command-bar">
      <div className="command-bar__brand">
        <span className="brand-mark"><UiIcon name="brand" size={22} /></span>
        <strong>akra-hookers</strong>
      </div>
      <h1>Prompt canvas</h1>
      <label className="command-control command-control--project">
        <UiIcon name="folder" />
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
      <button
        className="command-control command-control--locations"
        type="button"
        aria-label={`Work locations: ${originCount}`}
        onClick={onOpenWorkLocations}
      >
        <UiIcon name="location" />
        <span>Work locations</span>
        <small>{originCount}</small>
      </button>
      <button
        className={`capture-health capture-health--${health.tone}`}
        type="button"
        onClick={onOpenCaptureSettings}
        aria-label={`Capture health: ${health.label}`}
      >
        <span>Codex capture</span>
        <i aria-hidden="true" />
        <strong>{health.label}</strong>
      </button>
      <button
        className="icon-button command-bar__settings"
        type="button"
        onClick={onOpenCaptureSettings}
        aria-label="Capture 설정으로 이동"
        title="Capture 설정으로 이동"
      >
        <UiIcon name="settings" />
      </button>
    </header>
  );
}
