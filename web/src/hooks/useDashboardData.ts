import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  applyNodeChanges, type Edge, type NodeChange, type XYPosition,
} from "@xyflow/react";
import { useQueries, useQuery } from "@tanstack/react-query";

import type { ActivityPeriod, ActivityScope, ApiClient } from "../api";
import {
  isActivityKindVisible,
  type ActivityVisibility,
} from "../activity-visibility";
import {
  toCanvasNodes,
  toPersistedCanvasPosition,
  toVisibleEdges,
} from "../canvas";
import type { ActivityFlowNode } from "../components/ActivityNode";

const ACTIVITY_PAGE_SIZE = 100;

function reconcileNodes(
  current: ActivityFlowNode[],
  fresh: ActivityFlowNode[],
  dirtyActivityIds: ReadonlySet<number>,
): ActivityFlowNode[] {
  const currentById = new Map(current.map((node) => [node.data.activityId, node]));
  const reconciled = fresh.map((node) => {
    const existing = currentById.get(node.data.activityId);
    if (!existing) return node;
    const position = dirtyActivityIds.has(node.data.activityId)
      ? existing.position
      : node.position;
    if (
      existing.position.x === position.x
      && existing.position.y === position.y
      && JSON.stringify(existing.data) === JSON.stringify(node.data)
    ) {
      return existing;
    }
    return {
      ...existing,
      ...node,
      position,
      selected: existing.selected,
      dragging: existing.dragging,
    };
  });
  return reconciled.length === current.length
    && reconciled.every((node, index) => node === current[index])
    ? current
    : reconciled;
}

export function useDashboardData(
  client: ApiClient | null,
  activityScope: ActivityScope,
  activityVisibility: ActivityVisibility,
  activityPeriod: ActivityPeriod,
) {
  const [nodes, setNodes] = useState<ActivityFlowNode[]>([]);
  const [canvasFitViewKey, setCanvasFitViewKey] = useState("pending");
  const [selectedActivityIds, setSelectedActivityIds] = useState<number[]>([]);
  const dirtyPositions = useRef(new Map<number, number>());
  const revision = useRef(0);
  const positionQueues = useRef(new Map<number, Promise<void>>());
  const edgeCache = useRef<Edge[]>([]);
  const [olderActivities, setOlderActivities] = useState<NonNullable<
    Awaited<ReturnType<ApiClient["activities"]>>
  >>([]);
  const [olderActivitiesHaveMore, setOlderActivitiesHaveMore] =
    useState<boolean | null>(null);
  const [olderActivityPageCursors, setOlderActivityPageCursors] = useState<number[]>([]);
  const [loadingOlderActivities, setLoadingOlderActivities] = useState(false);
  const [olderActivitiesError, setOlderActivitiesError] = useState("");
  const scopeKey = activityScope.scope === "project"
    ? `project:${activityScope.projectId}`
    : activityScope.scope;
  const activityFilters = useMemo(() => ({
    includeInternal: activityVisibility.internal,
    period: activityPeriod,
  }), [activityPeriod, activityVisibility.internal]);
  const visibilityQuery = useMemo(() => ({
    includeInternal: activityVisibility.internal,
  }), [activityVisibility.internal]);

  const activities = useQuery({
    queryKey: ["activities", activityScope, activityFilters],
    queryFn: () => client!.activities(activityScope, {
      limit: ACTIVITY_PAGE_SIZE,
      order: "newest",
      ...activityFilters,
    }),
    enabled: client !== null,
    refetchInterval: 500,
  });
  const inboxCount = useQuery({
    queryKey: ["activity-count", { scope: "inbox" }, activityFilters],
    queryFn: () => client!.activityCount({ scope: "inbox" }, activityFilters),
    enabled: client !== null,
    refetchInterval: 500,
  });
  const allCount = useQuery({
    queryKey: ["activity-count", { scope: "all" }, activityFilters],
    queryFn: () => client!.activityCount({ scope: "all" }, activityFilters),
    enabled: client !== null,
    refetchInterval: 500,
  });
  const projects = useQuery({
    queryKey: ["projects", activityFilters],
    queryFn: () => client!.projects(activityFilters),
    enabled: client !== null,
    refetchInterval: 500,
  });
  const origins = useQuery({
    queryKey: ["origins", activityFilters],
    queryFn: () => client!.origins(activityFilters),
    enabled: client !== null,
    refetchInterval: 500,
  });
  const provider = useQuery({
    queryKey: ["provider", "codex"],
    queryFn: () => client!.provider("codex"),
    enabled: client !== null,
    refetchInterval: 2_000,
  });
  const canvasRevision = useQuery({
    queryKey: ["canvas-revision"],
    queryFn: () => client!.canvasRevision(),
    enabled: client !== null,
    refetchInterval: 500,
  });
  const canvas = useQuery({
    queryKey: ["canvas", canvasRevision.data],
    queryFn: () => client!.canvas(),
    enabled: client !== null && canvasRevision.data !== undefined,
    placeholderData: (previous) => previous,
  });
  const persistedEdges = useQuery({
    queryKey: ["canvas-edges", canvasRevision.data],
    queryFn: () => client!.edges(),
    enabled: client !== null && canvasRevision.data !== undefined,
    placeholderData: (previous) => previous,
  });
  const selectedDetails = useQueries({
    queries: selectedActivityIds.map((activityId) => ({
      queryKey: ["activity", activityId, visibilityQuery],
      queryFn: () => client!.activity(activityId, visibilityQuery),
      enabled: client !== null,
    })),
  });
  useEffect(() => {
    setOlderActivities([]);
    setOlderActivitiesHaveMore(null);
    setOlderActivityPageCursors([]);
    setOlderActivitiesError("");
  }, [activityFilters, client, scopeKey]);
  const allActivityItems = useMemo(() => {
    const byId = new Map(
      [...olderActivities, ...(activities.data ?? [])]
        .map((activity) => [activity.id, activity]),
    );
    return [...byId.values()];
  }, [activities.data, olderActivities]);
  const activityItems = useMemo(
    () => allActivityItems.filter((activity) =>
      isActivityKindVisible(activity.activity_kind, activityVisibility)),
    [activityVisibility, allActivityItems],
  );
  const mapCanvasNodes = useCallback(
    (currentCanvasNodes: Parameters<typeof toCanvasNodes>[1]) => toCanvasNodes(
      activityItems,
      currentCanvasNodes,
      activityPeriod === "all" ? "persisted" : "compact-filtered",
    ),
    [activityItems, activityPeriod],
  );
  const hasOlderActivities = olderActivitiesHaveMore
    ?? activities.data?.length === ACTIVITY_PAGE_SIZE;
  const loadOlderActivities = useCallback(async () => {
    const cursor = olderActivities.at(-1)?.id ?? activities.data?.at(-1)?.id;
    if (!client || cursor === undefined || !hasOlderActivities) return;
    setLoadingOlderActivities(true);
    setOlderActivitiesError("");
    try {
      const page = await client.activities(activityScope, {
        limit: ACTIVITY_PAGE_SIZE,
        afterId: cursor,
        order: "newest",
        ...activityFilters,
      });
      setOlderActivities((current) => {
        const byId = new Map(
          [...(activities.data ?? []), ...current, ...page]
            .map((activity) => [activity.id, activity]),
        );
        return [...byId.values()];
      });
      setOlderActivityPageCursors((current) =>
        current.includes(cursor) ? current : [...current, cursor]
      );
      setOlderActivitiesHaveMore(page.length === ACTIVITY_PAGE_SIZE);
    } catch (cause) {
      setOlderActivitiesError(
        cause instanceof Error ? cause.message : "이전 활동을 불러오지 못했습니다.",
      );
    } finally {
      setLoadingOlderActivities(false);
    }
  }, [
    activities.data,
    activityScope,
    client,
    hasOlderActivities,
    olderActivities,
    activityFilters,
  ]);

  useEffect(() => {
    if (!client || olderActivityPageCursors.length === 0) return;
    let cancelled = false;
    let refreshInFlight = false;
    const refreshOlderPages = async () => {
      if (refreshInFlight) return;
      refreshInFlight = true;
      try {
        const pages = await Promise.all(olderActivityPageCursors.map((afterId) =>
          client.activities(activityScope, {
            limit: ACTIVITY_PAGE_SIZE,
            afterId,
            order: "newest",
            ...activityFilters,
          })
        ));
        if (cancelled) return;
        setOlderActivities((current) => {
          const byId = new Map(current.map((activity) => [activity.id, activity]));
          for (const activity of pages.flat()) byId.set(activity.id, activity);
          return [...byId.values()];
        });
        setOlderActivitiesHaveMore(
          pages.at(-1)?.length === ACTIVITY_PAGE_SIZE,
        );
      } catch {
        // The primary activity poll remains authoritative and the next interval retries.
      } finally {
        refreshInFlight = false;
      }
    };
    void refreshOlderPages();
    const interval = window.setInterval(() => void refreshOlderPages(), 500);
    return () => {
      cancelled = true;
      window.clearInterval(interval);
    };
  }, [activityFilters, activityScope, client, olderActivityPageCursors]);

  useEffect(() => {
    if (!activities.data || !canvas.data) return;
    const fresh = mapCanvasNodes(canvas.data);
    const visible = new Set(fresh.map(({ data }) => data.activityId));
    for (const activityId of dirtyPositions.current.keys()) {
      if (!visible.has(activityId)) dirtyPositions.current.delete(activityId);
    }
    setNodes((current) => reconcileNodes(
      current,
      fresh,
      new Set(dirtyPositions.current.keys()),
    ));
    setCanvasFitViewKey(
      `${activityPeriod}:${fresh.map(({ id }) => id).sort().join(":")}`,
    );
    setSelectedActivityIds((current) => {
      const retained = current.filter((id) => visible.has(id));
      return retained.length === current.length ? current : retained;
    });
  }, [activities.data, activityPeriod, canvas.data, mapCanvasNodes]);

  const edges = useMemo(() => {
    const fresh = activities.data && canvas.data && persistedEdges.data
      ? toVisibleEdges(activityItems, canvas.data, persistedEdges.data)
      : [];
    const unchanged = fresh.length === edgeCache.current.length
      && fresh.every((edge, index) => {
        const existing = edgeCache.current[index];
        return existing?.id === edge.id
          && existing.source === edge.source
          && existing.target === edge.target;
      });
    if (!unchanged) edgeCache.current = fresh;
    return edgeCache.current;
  }, [activities.data, activityItems, canvas.data, persistedEdges.data]);
  const assignmentDetails = selectedActivityIds.length === 0
    ? []
    : selectedDetails.every(({ data }) => data !== undefined)
      ? selectedDetails.flatMap(({ data }) => data ? [data] : [])
      : undefined;

  const refreshProjectContext = useCallback(async () => {
    setOlderActivities([]);
    setOlderActivitiesHaveMore(null);
    setOlderActivityPageCursors([]);
    setOlderActivitiesError("");
    await Promise.all([
      activities.refetch(),
      allCount.refetch(),
      inboxCount.refetch(),
      projects.refetch(),
      origins.refetch(),
      ...selectedDetails.map((detail) => detail.refetch()),
    ]);
  }, [activities, allCount, inboxCount, origins, projects, selectedDetails]);
  const refreshCanvas = useCallback(async () => {
    await Promise.all([canvas.refetch(), persistedEdges.refetch()]);
  }, [canvas, persistedEdges]);
  const refreshCanvasAuthoritatively = useCallback(async () => {
    const [result] = await Promise.all([canvas.refetch(), persistedEdges.refetch()]);
    if (activities.data && result.data) {
      setNodes((current) => reconcileNodes(
        current,
        mapCanvasNodes(result.data!),
        new Set(),
      ));
    }
  }, [activities.data, canvas, mapCanvasNodes, persistedEdges]);
  const refreshAfterActivityDeletion = useCallback(async (activityId: number) => {
    setSelectedActivityIds((current) => current.filter((id) => id !== activityId));
    setOlderActivities([]);
    setOlderActivitiesHaveMore(null);
    setOlderActivityPageCursors([]);
    setOlderActivitiesError("");
    await Promise.all([
      activities.refetch(),
      allCount.refetch(),
      inboxCount.refetch(),
      projects.refetch(),
      origins.refetch(),
      canvasRevision.refetch(),
      canvas.refetch(),
      persistedEdges.refetch(),
    ]);
  }, [
    activities,
    allCount,
    canvas,
    canvasRevision,
    inboxCount,
    origins,
    persistedEdges,
    projects,
  ]);

  const onNodesChange = useCallback((changes: NodeChange<ActivityFlowNode>[]) => {
    for (const change of changes) {
      if (change.type !== "position" || !change.position) continue;
      const activityId = Number(change.id.slice("activity-".length));
      dirtyPositions.current.set(activityId, ++revision.current);
    }
    setNodes((current) => applyNodeChanges(
      changes.filter((change) => change.type !== "remove"),
      current,
    ));
  }, []);

  const commitNodePosition = useCallback((
    activityId: number,
    position: XYPosition,
    displayedToPersistedOffset?: XYPosition,
  ) => {
    const currentRevision = dirtyPositions.current.get(activityId) ?? ++revision.current;
    dirtyPositions.current.set(activityId, currentRevision);
    const previous = positionQueues.current.get(activityId) ?? Promise.resolve();
    let queued: Promise<void>;
    queued = previous.catch(() => undefined).then(async () => {
      const canvasNode = canvas.data?.find((node) => node.activity_event_id === activityId);
      if (!client || !canvasNode) return;
      const displayedNode = mapCanvasNodes(canvas.data ?? [])
        .find((node) => node.data.activityId === activityId);
      const persistedPosition = toPersistedCanvasPosition(
        position,
        canvasNode,
        displayedNode,
        displayedToPersistedOffset,
      );
      try {
        await client.updateCanvasPosition(canvasNode.id, persistedPosition);
        const result = await canvas.refetch();
        if (dirtyPositions.current.get(activityId) !== currentRevision) return;
        dirtyPositions.current.delete(activityId);
        if (activities.data && result.data) {
          setNodes((current) => reconcileNodes(
            current,
            mapCanvasNodes(result.data!),
            new Set(),
          ));
        }
      } catch (cause) {
        const result = await canvas.refetch();
        if (dirtyPositions.current.get(activityId) === currentRevision) {
          dirtyPositions.current.delete(activityId);
          if (activities.data && result.data) {
            setNodes((current) => reconcileNodes(
              current,
              mapCanvasNodes(result.data!),
              new Set(),
            ));
          }
        }
        throw cause;
      }
    }).finally(() => {
      if (positionQueues.current.get(activityId) === queued) {
        positionQueues.current.delete(activityId);
      }
    });
    positionQueues.current.set(activityId, queued);
    return queued;
  }, [activities.data, canvas, client, mapCanvasNodes]);

  const bootstrapQueries = [
    activities,
    allCount,
    inboxCount,
    projects,
    origins,
    provider,
    canvasRevision,
    canvas,
    persistedEdges,
  ];
  const bootstrapError = bootstrapQueries.some((query) => query.isError);
  const bootstrapReady = !bootstrapError && bootstrapQueries.every(
    (query) => query.data !== undefined,
  );
  const retryBootstrap = useCallback(async () => {
    await Promise.all([
      activities.refetch(),
      allCount.refetch(),
      inboxCount.refetch(),
      projects.refetch(),
      origins.refetch(),
      provider.refetch(),
      canvasRevision.refetch(),
      canvas.refetch(),
      persistedEdges.refetch(),
    ]);
  }, [
    activities,
    allCount,
    canvas,
    canvasRevision,
    inboxCount,
    origins,
    persistedEdges,
    projects,
    provider,
  ]);

  return {
    activities, allCount, inboxCount, projects, origins, provider, canvas,
    nodes, setNodes, canvasFitViewKey, edges, onNodesChange, commitNodePosition,
    selectedActivityIds, setSelectedActivityIds, assignmentDetails,
    refreshProjectContext, refreshCanvas, refreshCanvasAuthoritatively,
    refreshAfterActivityDeletion,
    bootstrapError, bootstrapReady, retryBootstrap,
    hasOlderActivities, loadOlderActivities, loadingOlderActivities, olderActivitiesError,
  };
}
