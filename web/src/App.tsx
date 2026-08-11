import { useCallback, useEffect, useMemo, useState } from "react";
import { type Node, type NodeTypes } from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { createApiClient } from "./api";
import { getAssignmentSelection } from "./assignment-selection";
import type { ActivityNodeData } from "./canvas";
import { ActivityAssignmentBar } from "./components/ActivityAssignmentBar";
import { ActivityCanvas } from "./components/ActivityCanvas";
import { ActivityDetailPanel } from "./components/ActivityDetailPanel";
import { ActivityNode } from "./components/ActivityNode";
import { OriginSetupDialog } from "./components/OriginSetupDialog";
import { ProjectDialog } from "./components/ProjectDialog";
import { ProjectRail, type ProjectFilter } from "./components/ProjectRail";
import { useDashboardData } from "./hooks/useDashboardData";
const nodeTypes = { activity: ActivityNode } satisfies NodeTypes;
export function App() {
  const [codexEnabled, setCodexEnabled] = useState(false);
  const [filter, setFilter] = useState<ProjectFilter>("all");
  const [projectDialog, setProjectDialog] = useState<"new" | number | null>(null);
  const [originDialog, setOriginDialog] = useState<number | null>(null);
  const [detailActivityId, setDetailActivityId] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const client = useMemo(() => {
    const url = import.meta.env.VITE_AKRA_URL;
    const token = import.meta.env.VITE_AKRA_TOKEN;
    return url && token ? createApiClient(url, token) : null;
  }, []);
  const activityScope = filter === "all"
    ? { scope: "all" } as const
    : filter === "inbox"
      ? { scope: "inbox" } as const
      : { scope: "project", projectId: Number(filter.slice("project:".length)) } as const;
  const {
    activities, inboxCount, projects, origins, provider, canvas,
    nodes, setNodes, edges, onNodesChange, commitNodePosition,
    selectedActivityIds, setSelectedActivityIds, assignmentDetails,
    refreshProjectContext, refreshCanvas, refreshCanvasAuthoritatively,
    bootstrapError, bootstrapReady, retryBootstrap,
    hasOlderActivities, loadOlderActivities, loadingOlderActivities, olderActivitiesError,
  } = useDashboardData(client, activityScope);
  useEffect(() => {
    if (provider.data) {
      setCodexEnabled(provider.data.enabled);
    }
  }, [provider.data]);
  useEffect(() => {
    if (
      detailActivityId !== null
      && nodes.every(({ data }) => data.activityId !== detailActivityId)
    ) {
      setDetailActivityId(null);
    }
  }, [detailActivityId, nodes]);
  const changeSelection = useCallback((selection: { nodes: Node<ActivityNodeData>[] }) => {
    const selected = selection.nodes.map(({ data }) => data.activityId);
    setSelectedActivityIds((current) =>
      current.length === selected.length
      && current.every((activityId, index) => activityId === selected[index])
        ? current
        : selected
    );
  }, []);
  const clearCanvas = useCallback(async () => {
    if (!client) {
      return;
    }
    try {
      await client.clearCanvas();
      await refreshCanvasAuthoritatively();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not clear canvas.");
    }
  }, [client, refreshCanvasAuthoritatively]);
  const changeProvider = useCallback(async (enabled: boolean) => {
    if (!client) {
      return;
    }
    const previous = codexEnabled;
    setCodexEnabled(enabled);
    try {
      await client.setProviderEnabled("codex", enabled);
      await provider.refetch();
    } catch (cause) {
      setCodexEnabled(previous);
      setError(cause instanceof Error ? cause.message : "Could not update Codex capture.");
    }
  }, [client, codexEnabled, provider]);
  const managedProject = projectDialog === null || projectDialog === "new"
    ? undefined
    : projects.data?.find((project) => project.id === projectDialog);
  const managedOrigin = originDialog === null
    ? undefined
    : origins.data?.find((origin) => origin.id === originDialog);
  const assignmentSelection = getAssignmentSelection(
    assignmentDetails,
    origins.data ?? [],
  );
  const selectedProjectId = assignmentDetails?.[0]?.project?.id;
  const currentProjectId = selectedProjectId !== undefined
    && assignmentDetails?.every(({ project }) => project?.id === selectedProjectId)
    ? selectedProjectId
    : null;
  return (
    <main className={detailActivityId === null ? "app-shell" : "app-shell app-shell--detail"}>
      <ProjectRail
        nodeCount={nodes.length} codexEnabled={codexEnabled}
        codexAvailable={provider.data !== undefined && !provider.isError}
        projects={projects.data ?? []} origins={origins.data ?? []}
        inboxCount={inboxCount.data ?? 0} filter={filter}
        onCodexChange={(enabled) => void changeProvider(enabled)}
        onFilterChange={setFilter} onNewProject={() => setProjectDialog("new")}
        onManageProject={setProjectDialog} onManageOrigin={setOriginDialog}
      />
      <section className="canvas-panel">
        <header>
          <div className="canvas-actions">
            <p className="eyebrow">PROJECT ACTIVITY</p>
            <h2>Prompt canvas</h2>
          </div>
          <div>
            {hasOlderActivities && (
              <button
                type="button"
                disabled={loadingOlderActivities}
                onClick={() => void loadOlderActivities()}
              >
                {loadingOlderActivities ? "불러오는 중…" : "이전 활동 불러오기"}
              </button>
            )}
            <button type="button" onClick={() => void clearCanvas()}>Clear canvas</button>
          </div>
        </header>
        {bootstrapError && (
          <div className="dashboard-error" role="alert">
            <span>대시보드 데이터를 불러오지 못했습니다.</span>
            <button type="button" onClick={() => void retryBootstrap()}>
              다시 시도
            </button>
          </div>
        )}
        {olderActivitiesError && (
          <p className="dashboard-error" role="alert">{olderActivitiesError}</p>
        )}
        <ActivityCanvas
          client={client} canvasNodes={canvas.data ?? []}
          nodes={nodes} setNodes={setNodes} edges={edges} nodeTypes={nodeTypes}
          onNodesChange={onNodesChange} onPositionCommit={commitNodePosition}
          onActivityOpen={setDetailActivityId} onError={setError}
          onPersistedChange={refreshCanvas}
          onSelectionChange={changeSelection}
        />
        {bootstrapReady && nodes.length === 0 && (
          <div className="empty-state">
            <strong>No activity on this canvas</strong>
            <span>Submitted provider prompts appear here; removing a node never deletes its activity record.</span>
          </div>
        )}
        {error && <p className="error-message" role="alert">{error}</p>}
        {client && (
          <ActivityAssignmentBar
            key={assignmentSelection.state === "assignable"
              ? assignmentSelection.activityIds.join(":")
              : assignmentSelection.state}
            selection={assignmentSelection}
            projects={projects.data ?? []}
            currentProjectId={currentProjectId}
            onAssign={async (request) => {
              await client.assignActivities(request);
              await refreshProjectContext();
              if (detailActivityId !== null && request.activity_ids.includes(detailActivityId)) {
                const projectId = request.destination && "project_id" in request.destination
                  ? request.destination.project_id
                  : null;
                const remainsVisible = filter === "all"
                  || (filter === "inbox" && request.destination === null)
                  || filter === `project:${projectId}`;
                if (!remainsVisible) setDetailActivityId(null);
              }
            }}
            onMoveOrigin={setOriginDialog}
          />
        )}
      </section>
      {client && detailActivityId !== null && (
        <ActivityDetailPanel
          key={detailActivityId}
          activityId={detailActivityId}
          client={client}
          onClose={() => setDetailActivityId(null)}
        />
      )}
      {client && projectDialog !== null && (
        <ProjectDialog
          client={client} projects={projects.data ?? []} project={managedProject}
          onClose={() => setProjectDialog(null)}
          onChanged={async (projectId) => {
            if (projectId !== undefined) setFilter(`project:${projectId}`);
            await refreshProjectContext();
          }}
        />
      )}
      {client && managedOrigin && (
        <OriginSetupDialog
          client={client} origin={managedOrigin} projects={projects.data ?? []}
          onClose={() => setOriginDialog(null)}
          onChanged={refreshProjectContext}
        />
      )}
    </main>
  );
}
