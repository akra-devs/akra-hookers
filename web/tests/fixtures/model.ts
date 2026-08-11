import type {
  ActivityAssignmentRequest,
  ActivityProject,
  OriginRoutingRequest,
  ProjectDestination,
  ProjectSummary,
} from "../../src/api";
import type { FixtureState } from "./state";

export class FixtureModel {
  constructor(readonly state: FixtureState) {}

  createProject(name: string) {
    const project = {
      id: this.state.nextProjectId++,
      name,
      origin_count: 0,
      activity_count: 0,
      needs_setup: false,
      latest_activity_at_us: null,
    };
    this.state.projects.push(project);
    return project;
  }

  renameProject(id: number, name: string) {
    const project = required(this.state.projects.find((candidate) => candidate.id === id));
    project.name = name;
    for (const origin of this.state.origins) {
      if (origin.default_project_id === id) {
        origin.default_project_name = name;
      }
    }
    this.replaceProject(id, id, name);
    return project;
  }

  mergeProject(sourceId: number, targetId: number) {
    const target = required(this.state.projects.find((project) => project.id === targetId));
    this.replaceProject(sourceId, target.id, target.name);
    for (const origin of this.state.origins) {
      if (origin.default_project_id === sourceId) {
        origin.default_project_id = target.id;
        origin.default_project_name = target.name;
      }
    }
    for (const [conversation, projectId] of Object.entries(this.state.conversationRoutes)) {
      if (projectId === sourceId) this.state.conversationRoutes[conversation] = target.id;
    }
    this.state.projects = this.state.projects.filter((project) => project.id !== sourceId);
    this.recount();
    return target;
  }

  configureOrigin(id: number, request: OriginRoutingRequest) {
    const origin = required(this.state.origins.find((candidate) => candidate.id === id));
    const wasUnconfirmed = origin.setup_state === "unconfirmed";
    const suggestedProjectId = origin.default_project_id;
    origin.setup_state = "confirmed";
    origin.routing_mode = request.mode;
    if (request.mode === "shared") {
      origin.default_project_id = null;
      origin.default_project_name = null;
    } else {
      const project = "new_project_name" in request.destination
        && wasUnconfirmed
        && suggestedProjectId !== null
        ? this.renameProject(suggestedProjectId, request.destination.new_project_name)
        : this.destination(request.destination);
      origin.default_project_id = project.id;
      origin.default_project_name = project.name;
      for (const [activityId, activityOriginId] of Object.entries(this.state.activityOrigins)) {
        if (activityOriginId === id) this.moveActivity(Number(activityId), project);
      }
    }
    this.recount();
    return origin;
  }

  assign(request: ActivityAssignmentRequest) {
    const project = request.destination === null ? null : this.destination(request.destination);
    for (const id of request.activity_ids) {
      this.moveActivity(id, project);
    }
    const action = request.future_route ?? "unchanged";
    if (action === "set" && project) {
      this.state.conversationRoutes["codex:fixture-session"] = project.id;
    } else if (action === "clear") {
      delete this.state.conversationRoutes["codex:fixture-session"];
    }
    this.recount();
    return {
      activity_ids: [...request.activity_ids].sort((left, right) => left - right),
      project_id: project?.id ?? null,
      future_route: action,
    };
  }

  syncCanvasState() {
    const visible = new Set(this.state.canvasNodes.map((node) => node.activity_event_id));
    for (const detail of Object.values(this.state.details)) {
      detail.on_canvas = visible.has(detail.id);
      for (const turn of detail.conversation) turn.on_canvas = visible.has(turn.id);
    }
  }

  private destination(destination: ProjectDestination) {
    return "project_id" in destination
      ? required(this.state.projects.find((project) => project.id === destination.project_id))
      : this.createProject(destination.new_project_name);
  }

  private moveActivity(id: number, project: ProjectSummary | null) {
    const reference: ActivityProject | null = project
      ? { id: project.id, name: project.name }
      : null;
    const activity = required(this.state.activities.find((candidate) => candidate.id === id));
    activity.project = reference;
    for (const detail of Object.values(this.state.details)) {
      if (detail.id === id) detail.project = reference;
      const turn = detail.conversation.find((candidate) => candidate.id === id);
      if (turn) turn.project = reference;
    }
  }

  private replaceProject(sourceId: number, targetId: number, targetName: string) {
    for (const activity of this.state.activities) {
      if (activity.project?.id === sourceId) {
        activity.project = { id: targetId, name: targetName };
      }
    }
    for (const detail of Object.values(this.state.details)) {
      if (detail.project?.id === sourceId) {
        detail.project = { id: targetId, name: targetName };
      }
      for (const turn of detail.conversation) {
        if (turn.project?.id === sourceId) {
          turn.project = { id: targetId, name: targetName };
        }
      }
    }
  }

  private recount() {
    for (const project of this.state.projects) {
      project.activity_count = this.state.activities.filter(
        (activity) => activity.project?.id === project.id,
      ).length;
      project.origin_count = this.state.origins.filter(
        (origin) => origin.default_project_id === project.id,
      ).length;
    }
  }
}

function required<T>(value: T | undefined): T {
  if (value === undefined) throw new Error("Fixture state invariant failed");
  return value;
}
