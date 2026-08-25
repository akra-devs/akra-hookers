import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { type Node, type NodeTypes } from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { createApiClient, type ActivityPeriod } from "./api";
import {
  loadActivityVisibility,
  saveActivityVisibility,
} from "./activity-visibility";
import { getAssignmentSelection } from "./assignment-selection";
import type { ActivityNodeData } from "./canvas";
import { AppCommandBar } from "./components/AppCommandBar";
import { ActivityAssignmentBar } from "./components/ActivityAssignmentBar";
import { ActivityCanvas } from "./components/ActivityCanvas";
import { ActivityDetailPanel } from "./components/ActivityDetailPanel";
import { ActivityNode } from "./components/ActivityNode";
import { ClearCanvasDialog } from "./components/ClearCanvasDialog";
import { CurationWorkspace } from "./components/CurationWorkspace";
import { OriginSetupDialog } from "./components/OriginSetupDialog";
import { ProjectDialog } from "./components/ProjectDialog";
import { ProjectRail, type ProjectFilter } from "./components/ProjectRail";
import { ProjectMemoryCanvas } from "./components/ProjectMemoryCanvas";
import { UiIcon } from "./components/UiIcon";
import { WorkDetailPanel } from "./components/WorkDetailPanel";
import type { CollectorOperation } from "./components/CollectorEndpointControl";
import { useDashboardData } from "./hooks/useDashboardData";
import { useWorkMemoryData } from "./hooks/useWorkMemoryData";
import "./desktop";
const nodeTypes = { activity: ActivityNode } satisfies NodeTypes;
export function App() {
  const [codexEnabled, setCodexEnabled] = useState(false);
  const [codexPending, setCodexPending] = useState(false);
  const [promptSummaryMode, setPromptSummaryMode] = useState<"off" | "smart">("off");
  const [promptSummaryPending, setPromptSummaryPending] = useState(false);
  const [promptSummaryError, setPromptSummaryError] = useState<string | null>(null);
  const [pendingCodexTargetIds, setPendingCodexTargetIds] = useState<string[]>([]);
  const [captureError, setCaptureError] = useState<string | null>(null);
  const [collectorOperation, setCollectorOperation] = useState<CollectorOperation>(null);
  const [filter, setFilter] = useState<ProjectFilter>("all");
  const [activityPeriod, setActivityPeriod] = useState<ActivityPeriod>("all");
  const [hideEmptyProjects, setHideEmptyProjects] = useState(false);
  const [activityVisibility, setActivityVisibility] = useState(
    loadActivityVisibility,
  );
  const [projectDialog, setProjectDialog] = useState<"new" | number | null>(null);
  const [originDialog, setOriginDialog] = useState<number | null>(null);
  const [detailActivityId, setDetailActivityId] = useState<number | null>(null);
  const [detailWorkId, setDetailWorkId] = useState<number | null>(null);
  const [canvasMode, setCanvasMode] = useState<"activity" | "memory">("activity");
  const [curationProjectId, setCurationProjectId] = useState<number | null>(null);
  const [clearConfirmOpen, setClearConfirmOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const detailTriggerRef = useRef<HTMLElement | null>(null);
  const clearCanvasTriggerRef = useRef<HTMLButtonElement | null>(null);
  const focusCanvasAfterClearRef = useRef(false);
  const curationTriggerRef = useRef<HTMLButtonElement | null>(null);
  const environmentClient = useMemo(() => {
    if (window.akraDesktop) return null;
    const url = import.meta.env.VITE_AKRA_URL;
    const token = import.meta.env.VITE_AKRA_TOKEN;
    return url && token ? createApiClient(url, token) : null;
  }, []);
  const [desktopClient, setDesktopClient] = useState<ReturnType<typeof createApiClient> | null>(null);
  useEffect(() => {
    if (environmentClient || !window.akraDesktop) return;
    let cancelled = false;
    window.akraDesktop.bootstrap().then(({ apiUrl, token }) => {
      if (!cancelled) setDesktopClient(createApiClient(apiUrl, token));
    }).catch(() => {
      if (!cancelled) setError("Desktop runtime에 연결하지 못했습니다.");
    });
    return () => { cancelled = true; };
  }, [environmentClient]);
  const client = desktopClient ?? environmentClient;
  const activityScope = filter === "all"
    ? { scope: "all" } as const
    : filter === "inbox"
      ? { scope: "inbox" } as const
      : { scope: "project", projectId: Number(filter.slice("project:".length)) } as const;
  const filteredProjectId = filter.startsWith("project:")
    ? Number(filter.slice("project:".length))
    : undefined;
  const {
    activities, allCount, inboxCount, projects, origins, provider, canvas,
    nodes, setNodes, canvasFitViewKey, edges, onNodesChange, commitNodePosition,
    selectedActivityIds, setSelectedActivityIds, assignmentDetails,
    refreshProjectContext, refreshCanvas, refreshCanvasAuthoritatively,
    refreshAfterActivityDeletion,
    bootstrapError, retryBootstrap,
    hasOlderActivities, loadOlderActivities, loadingOlderActivities, olderActivitiesError,
  } = useDashboardData(client, activityScope, activityVisibility, activityPeriod);
  const workMemory = useWorkMemoryData(client, filteredProjectId);
  useEffect(() => {
    saveActivityVisibility(activityVisibility);
  }, [activityVisibility]);
  useEffect(() => {
    if (provider.data && !provider.isError) {
      setCodexEnabled(provider.data.enabled);
      setPromptSummaryMode(provider.data.prompt_summary_mode);
      setCaptureError(null);
    }
  }, [provider.data, provider.isError]);
  useEffect(() => {
    if (
      detailActivityId !== null
      && nodes.every(({ data }) => data.activityId !== detailActivityId)
    ) {
      setDetailActivityId(null);
    }
  }, [detailActivityId, nodes]);
  useEffect(() => {
    if (
      detailWorkId !== null
      && workMemory.works.data !== undefined
      && workMemory.works.data.every(({ id }) => id !== detailWorkId)
    ) {
      setDetailWorkId(null);
    }
  }, [detailWorkId, workMemory.works.data]);
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
      return false;
    }
    try {
      await client.clearCanvas();
      await refreshCanvasAuthoritatively();
      return true;
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not clear canvas.");
      return false;
    }
  }, [client, refreshCanvasAuthoritatively]);
  const openActivity = useCallback((activityId: number) => {
    const content = document.querySelector<HTMLElement>(
      `[data-testid="activity-node-${activityId}"]`,
    );
    detailTriggerRef.current = content?.closest<HTMLElement>(".react-flow__node") ?? null;
    setCanvasMode("activity");
    setDetailWorkId(null);
    setDetailActivityId(activityId);
  }, []);
  const closeActivity = useCallback(() => {
    setDetailActivityId(null);
    requestAnimationFrame(() => detailTriggerRef.current?.focus());
  }, []);
  const openWork = useCallback((workId: number) => {
    detailTriggerRef.current = document
      .querySelector<HTMLElement>(`[data-testid="work-node-${workId}"]`)
      ?.closest<HTMLElement>(".react-flow__node") ?? null;
    setCanvasMode("memory");
    setDetailActivityId(null);
    setDetailWorkId(workId);
  }, []);
  const closeWork = useCallback(() => {
    setDetailWorkId(null);
    requestAnimationFrame(() => detailTriggerRef.current?.focus());
  }, []);
  const closeCuration = useCallback(() => {
    setCurationProjectId(null);
    requestAnimationFrame(() => curationTriggerRef.current?.focus());
  }, []);
  const finishCuration = useCallback((workIds: number[]) => {
    setCurationProjectId(null);
    setCanvasMode("memory");
    const firstWorkId = workIds[0];
    if (firstWorkId === undefined) return;
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        document
          .querySelector<HTMLElement>(`[data-testid="work-node-${firstWorkId}"]`)
          ?.closest<HTMLElement>(".react-flow__node")
          ?.focus();
      });
    });
  }, []);
  const focusCanvasStage = useCallback(() => {
    // React Flow can restore its own viewport focus on the same frame as an
    // empty-canvas render. Wait one extra frame so the explicit completion
    // target wins after the dialog has unmounted and the graph is settled.
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        document.querySelector<HTMLElement>(".flow-stage")?.focus();
      });
    });
  }, []);
  useEffect(() => {
    if (clearConfirmOpen || nodes.length > 0 || !focusCanvasAfterClearRef.current) return;
    focusCanvasAfterClearRef.current = false;
    focusCanvasStage();
  }, [clearConfirmOpen, focusCanvasStage, nodes.length]);
  const confirmClearCanvas = useCallback(async () => {
    if (!await clearCanvas()) return false;
    focusCanvasAfterClearRef.current = true;
    setClearConfirmOpen(false);
    return true;
  }, [clearCanvas]);
  const cancelClearCanvas = useCallback(() => {
    setClearConfirmOpen(false);
    requestAnimationFrame(() => clearCanvasTriggerRef.current?.focus());
  }, []);
  const changeProvider = useCallback(async (enabled: boolean) => {
    if (!client || provider.isError || codexPending || pendingCodexTargetIds.length > 0) {
      return;
    }
    const previous = codexEnabled;
    setCaptureError(null);
    setCodexPending(true);
    setCodexEnabled(enabled);
    try {
      await client.setProviderEnabled("codex", enabled);
    } catch (cause) {
      setCodexEnabled(previous);
      setCaptureError(cause instanceof Error ? cause.message : "Codex capture를 변경하지 못했습니다.");
      setCodexPending(false);
      return;
    }
    try {
      await provider.refetch({ throwOnError: true });
    } catch {
      setCaptureError(
        "설정은 변경되었지만 최신 상태를 확인하지 못했습니다. 대시보드에서 다시 시도하세요.",
      );
    } finally {
      setCodexPending(false);
    }
  }, [client, codexEnabled, codexPending, pendingCodexTargetIds.length, provider]);
  const changePromptSummaryMode = useCallback(async (mode: "off" | "smart") => {
    if (!client || provider.isError || promptSummaryPending) return;
    const previous = promptSummaryMode;
    setPromptSummaryError(null);
    setPromptSummaryPending(true);
    setPromptSummaryMode(mode);
    try {
      await client.setPromptSummaryMode(mode);
    } catch (cause) {
      setPromptSummaryMode(previous);
      setPromptSummaryError(
        cause instanceof Error ? cause.message : "프롬프트 요약 설정을 변경하지 못했습니다.",
      );
      setPromptSummaryPending(false);
      return;
    }
    try {
      await provider.refetch({ throwOnError: true });
    } catch {
      setPromptSummaryError(
        "설정은 변경되었지만 최신 상태를 확인하지 못했습니다. 대시보드에서 다시 확인하세요.",
      );
    } finally {
      setPromptSummaryPending(false);
    }
  }, [client, promptSummaryMode, promptSummaryPending, provider]);
  const changeProviderTarget = useCallback(async (targetId: string, enabled: boolean) => {
    if (
      !client
      || provider.isError
      || codexPending
      || pendingCodexTargetIds.includes(targetId)
    ) {
      return;
    }
    setCaptureError(null);
    setPendingCodexTargetIds((current) => [...current, targetId]);
    try {
      await client.setProviderTargetEnabled("codex", targetId, enabled);
    } catch (cause) {
      setCaptureError(cause instanceof Error ? cause.message : "Codex 설치 설정을 변경하지 못했습니다.");
      setPendingCodexTargetIds((current) => current.filter((id) => id !== targetId));
      return;
    }
    try {
      await provider.refetch({ throwOnError: true });
    } catch {
      setCaptureError(
        "설정은 변경되었지만 최신 상태를 확인하지 못했습니다. 대시보드에서 다시 시도하세요.",
      );
    } finally {
      setPendingCodexTargetIds((current) => current.filter((id) => id !== targetId));
    }
  }, [client, codexPending, pendingCodexTargetIds, provider]);
  const configureCollector = useCallback(async (endpoint: string, token?: string) => {
    if (!client) throw new Error("대시보드 연결을 확인하지 못했습니다.");
    if (collectorOperation !== null) return;
    setCollectorOperation("configure");
    try {
      await client.configureCollector(endpoint, token);
      try {
        await provider.refetch({ throwOnError: true });
      } catch {
        throw new Error("설정은 저장되었지만 최신 상태를 확인하지 못했습니다.");
      }
    } finally {
      setCollectorOperation(null);
    }
  }, [client, collectorOperation, provider]);
  const verifyCollector = useCallback(async () => {
    if (!client) throw new Error("대시보드 연결을 확인하지 못했습니다.");
    if (collectorOperation !== null) return;
    setCollectorOperation("verify");
    try {
      await client.verifyCollector();
      try {
        await provider.refetch({ throwOnError: true });
      } catch {
        throw new Error("연결은 확인되었지만 최신 상태를 확인하지 못했습니다.");
      }
    } finally {
      setCollectorOperation(null);
    }
  }, [client, collectorOperation, provider]);
  const currentProjects = projects.data ?? [];
  const visibleProjects = useMemo(() => {
    const items = projects.data ?? [];
    return hideEmptyProjects
      ? items.filter((project) => project.activity_count > 0)
      : items;
  }, [hideEmptyProjects, projects.data]);
  useEffect(() => {
    if (
      !hideEmptyProjects
      || projects.data === undefined
      || !filter.startsWith("project:")
    ) return;
    const projectId = Number(filter.slice("project:".length));
    if (!visibleProjects.some((project) => project.id === projectId)) {
      setFilter("all");
    }
  }, [filter, hideEmptyProjects, projects.data, visibleProjects]);
  const managedProject = projectDialog === null || projectDialog === "new"
    ? undefined
    : currentProjects.find((project) => project.id === projectDialog);
  const managedOrigin = originDialog === null
    ? undefined
    : origins.data?.find((origin) => origin.id === originDialog);
  const assignmentSelection = getAssignmentSelection(
    assignmentDetails,
    origins.data ?? [],
  );
  const assignedProjectId = assignmentDetails?.[0]?.project?.id;
  const currentProjectId = assignedProjectId !== undefined
    && assignmentDetails?.every(({ project }) => project?.id === assignedProjectId)
    ? assignedProjectId
    : null;
  const canvasDataIsReady = !bootstrapError
    && activities.data !== undefined
    && canvas.data !== undefined;
  const showCanvasEmptyState = canvasDataIsReady && nodes.length === 0;
  const curationProject = curationProjectId === null
    ? undefined
    : currentProjects.find((project) => project.id === curationProjectId);
  const currentOrigins = origins.data ?? [];
  const currentTargets = provider.data?.targets ?? [];
  const currentCollector = provider.data?.collector ?? {
    mode: "local" as const,
    endpoint: "http://127.0.0.1:42130",
    configured: false,
    token_configured: false,
    connected: null,
    last_delivery_at_us: null,
    pending_count: 0,
    last_error: null,
  };
  const focusRailSection = (selector: string) => {
    const target = document.querySelector<HTMLElement>(selector);
    target?.scrollIntoView({ behavior: "smooth", block: "nearest" });
    target?.focus({ preventScroll: true });
  };
  return (
    <main className={
      detailActivityId === null && detailWorkId === null
        ? "app-shell"
        : "app-shell app-shell--detail"
    }>
      <AppCommandBar
        filter={filter}
        projects={visibleProjects}
        inboxCount={inboxCount.data ?? 0}
        originCount={currentOrigins.length}
        activityPeriod={activityPeriod}
        codexAvailable={provider.data !== undefined && !provider.isError}
        codexTargets={currentTargets}
        collector={provider.data?.collector}
        collectorOperation={collectorOperation}
        onFilterChange={setFilter}
        onActivityPeriodChange={setActivityPeriod}
        onOpenWorkLocations={() => focusRailSection("#work-locations-heading")}
        onOpenCaptureSettings={() => focusRailSection("#codex-capture-control")}
      />
      <ProjectRail
        nodeCount={allCount.data ?? 0} codexEnabled={codexEnabled}
        codexAvailable={provider.data !== undefined && !provider.isError}
        codexPending={codexPending}
        promptSummaryMode={promptSummaryMode}
        promptSummaryPending={promptSummaryPending}
        promptSummaryError={promptSummaryError}
        codexTargets={currentTargets}
        pendingCodexTargetIds={pendingCodexTargetIds}
        captureError={captureError}
        collector={currentCollector}
        collectorOperation={collectorOperation}
        activityVisibility={activityVisibility}
        projects={visibleProjects} totalProjectCount={currentProjects.length} origins={currentOrigins}
        inboxCount={inboxCount.data ?? 0} filter={filter}
        hideEmptyProjects={hideEmptyProjects}
        onCodexChange={(enabled) => void changeProvider(enabled)}
        onPromptSummaryModeChange={(mode) => void changePromptSummaryMode(mode)}
        onCodexTargetChange={(targetId, enabled) =>
          void changeProviderTarget(targetId, enabled)}
        onCollectorConfigure={configureCollector}
        onCollectorVerify={verifyCollector}
        onActivityVisibilityChange={(kind, visible) =>
          setActivityVisibility((current) => ({ ...current, [kind]: visible }))}
        onHideEmptyProjectsChange={setHideEmptyProjects}
        onFilterChange={setFilter} onNewProject={() => setProjectDialog("new")}
        onManageProject={setProjectDialog} onManageOrigin={setOriginDialog}
      />
      <section className="canvas-panel">
        <header className="canvas-toolbar">
          <div className="canvas-context">
            <span>{curationProject ? "Log curation" : canvasMode === "memory" ? "Project memory" : "Project activity"}</span>
            <strong>
              {curationProject
                ? curationProject.name
                : canvasMode === "memory"
                  ? `${workMemory.works.data?.length ?? 0} works`
                  : `${nodes.length} activities`}
            </strong>
          </div>
          <div className="canvas-header-actions">
            {!curationProject && (
              <>
                <div className="canvas-mode-switch" role="group" aria-label="캔버스 보기">
                  <button
                    type="button"
                    aria-pressed={canvasMode === "activity"}
                    className={canvasMode === "activity" ? "is-active" : undefined}
                    onClick={() => {
                      setCanvasMode("activity");
                      setDetailWorkId(null);
                    }}
                  >
                    활동 로그
                  </button>
                  <button
                    type="button"
                    aria-pressed={canvasMode === "memory"}
                    className={canvasMode === "memory" ? "is-active" : undefined}
                    onClick={() => {
                      setCanvasMode("memory");
                      setDetailActivityId(null);
                    }}
                  >
                    작업 지도
                  </button>
                </div>
                <button
                  ref={curationTriggerRef}
                  type="button"
                  className="curation-launch"
                  disabled={filteredProjectId === undefined}
                  title={filteredProjectId === undefined ? "프로젝트를 먼저 선택하세요" : undefined}
                  onClick={() => {
                    if (filteredProjectId !== undefined) {
                      setDetailActivityId(null);
                      setDetailWorkId(null);
                      setCurationProjectId(filteredProjectId);
                    }
                  }}
                >
                  로그 정리
                </button>
                {canvasMode === "activity" && hasOlderActivities && (
                  <button
                    type="button"
                    disabled={loadingOlderActivities}
                    onClick={() => void loadOlderActivities()}
                  >
                    {loadingOlderActivities ? "불러오는 중…" : "이전 활동 불러오기"}
                  </button>
                )}
                {canvasMode === "activity" && nodes.length > 0 && (
                  <button
                    ref={clearCanvasTriggerRef}
                    className="canvas-clear"
                    type="button"
                    onClick={() => setClearConfirmOpen(true)}
                  >
                    Clear canvas
                  </button>
                )}
              </>
            )}
          </div>
        </header>
        {curationProject && client ? (
          <CurationWorkspace
            client={client}
            project={curationProject}
            onCancel={closeCuration}
            onApplied={async () => {
              setCanvasMode("memory");
              await Promise.all([workMemory.refresh(), refreshProjectContext()]);
            }}
            onFinish={finishCuration}
          />
        ) : canvasMode === "activity" ? (
          <>
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
              fitViewKey={canvasFitViewKey}
              onNodesChange={onNodesChange} onPositionCommit={commitNodePosition}
              onActivityOpen={openActivity} onError={setError}
              onPersistedChange={refreshCanvas}
              onActivityDeleted={refreshAfterActivityDeletion}
              onAuthoritativeRefresh={refreshCanvasAuthoritatively}
              onSelectionChange={changeSelection}
            />
            {showCanvasEmptyState && (
              <div className="empty-state">
                <strong>No activity on this canvas</strong>
                <span>Enable Codex capture and submit a prompt to add your first activity.</span>
              </div>
            )}
          </>
        ) : (
          <>
            {workMemory.isError && (
              <div className="dashboard-error" role="alert">
                <span>작업 지도를 불러오지 못했습니다.</span>
                <button type="button" onClick={() => void workMemory.refresh()}>다시 시도</button>
              </div>
            )}
            {client && workMemory.isReady && (
              <ProjectMemoryCanvas
                client={client}
                works={workMemory.works.data ?? []}
                persistedEdges={workMemory.edges.data ?? []}
                onOpenWork={openWork}
                onRefresh={workMemory.refresh}
                onError={setError}
              />
            )}
            {workMemory.isReady && (workMemory.works.data?.length ?? 0) === 0 && (
              <div className="empty-state work-memory-empty">
                <UiIcon name="work" size={24} />
                <strong>아직 확정된 작업이 없습니다.</strong>
                <span>프로젝트를 선택하고 로그 정리를 시작해 작업 단위를 만드세요.</span>
                {filteredProjectId !== undefined && (
                  <button type="button" onClick={() => setCurationProjectId(filteredProjectId)}>
                    첫 로그 정리 시작
                  </button>
                )}
              </div>
            )}
          </>
        )}
        {error && <p className="error-message" role="alert">{error}</p>}
        {client && canvasMode === "activity" && curationProject === undefined && (
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
          activityVisibility={activityVisibility}
          client={client}
          onClose={closeActivity}
          onDeleted={(activityId) => {
            setDetailActivityId(null);
            void refreshAfterActivityDeletion(activityId)
              .then(focusCanvasStage)
              .catch((cause) => setError(
                cause instanceof Error
                  ? cause.message
                  : "삭제된 활동 목록을 새로고침하지 못했습니다.",
              ));
          }}
          onSelectActivity={openActivity}
        />
      )}
      {client && detailWorkId !== null && (
        <WorkDetailPanel
          key={detailWorkId}
          client={client}
          workId={detailWorkId}
          onClose={closeWork}
          onOpenActivity={openActivity}
          onChanged={workMemory.refresh}
        />
      )}
      {clearConfirmOpen && (
        <ClearCanvasDialog
          nodeCount={nodes.length}
          onCancel={cancelClearCanvas}
          onConfirm={confirmClearCanvas}
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
