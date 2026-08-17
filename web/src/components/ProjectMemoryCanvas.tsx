import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Background,
  BackgroundVariant,
  Controls,
  Panel,
  ReactFlow,
  applyNodeChanges,
  type Edge,
  type NodeChange,
  type NodeTypes,
  type ReactFlowInstance,
} from "@xyflow/react";

import type { ApiClient, WorkEdge, WorkItem } from "../api";
import { useFitFlow } from "../useFitFlow";
import {
  WorkNode,
  WorkNodeActionsContext,
  type WorkFlowNode,
} from "./WorkNode";

const nodeTypes = { work: WorkNode } satisfies NodeTypes;

type ProjectMemoryCanvasProps = {
  client: ApiClient;
  works: WorkItem[];
  persistedEdges: WorkEdge[];
  onOpenWork: (workId: number) => void;
  onRefresh: () => Promise<unknown>;
  onError: (message: string) => void;
};

function flowNodes(works: WorkItem[]): WorkFlowNode[] {
  return works.map((work) => ({
    id: `work-${work.id}`,
    type: "work",
    position: { x: work.position_x, y: work.position_y },
    data: { work },
    ariaLabel: `${work.title}, 로그 ${work.log_count}개`,
  }));
}

function flowEdges(edges: WorkEdge[]): Edge[] {
  return edges.map((edge) => ({
    id: `work-edge-${edge.id}`,
    source: `work-${edge.source_work_item_id}`,
    target: `work-${edge.target_work_item_id}`,
    type: "default",
    ariaLabel: `작업 ${edge.source_work_item_id}에서 ${edge.target_work_item_id} 관계`,
  }));
}

export function ProjectMemoryCanvas({
  client,
  works,
  persistedEdges,
  onOpenWork,
  onRefresh,
  onError,
}: ProjectMemoryCanvasProps) {
  const [nodes, setNodes] = useState<WorkFlowNode[]>(() => flowNodes(works));
  const [selectedEdgeId, setSelectedEdgeId] = useState<string | null>(null);
  const [hiddenEdgeIds, setHiddenEdgeIds] = useState<string[]>([]);
  const [flow, setFlow] = useState<ReactFlowInstance<WorkFlowNode, Edge> | null>(null);
  const [zoom, setZoom] = useState(1);
  const pendingWorkIds = useRef(new Set<number>());
  const pendingEdgeIds = useRef(new Set<number>());
  const fitKey = works.map(({ id }) => id).sort((a, b) => a - b).join(":");
  const stageRef = useFitFlow(flow, fitKey, 1.15);
  const edges = useMemo(() => flowEdges(persistedEdges), [persistedEdges]);
  const displayedEdges = edges
    .filter((edge) => !hiddenEdgeIds.includes(edge.id))
    .map((edge) => ({ ...edge, selected: edge.id === selectedEdgeId }));

  useEffect(() => {
    const fresh = flowNodes(works);
    setNodes((current) => fresh.map((node) => {
      const existing = current.find((candidate) => candidate.id === node.id);
      return existing
        ? { ...node, selected: existing.selected, dragging: existing.dragging }
        : node;
    }));
  }, [works]);
  useEffect(() => {
    setHiddenEdgeIds((current) => current.filter((id) => edges.some((edge) => edge.id === id)));
  }, [edges]);

  const removeWork = useCallback((workId: number) => {
    if (pendingWorkIds.current.has(workId)) return;
    pendingWorkIds.current.add(workId);
    void client.deleteWork(workId).then(
      async () => {
        await onRefresh();
      },
      (cause: unknown) => {
        onError(cause instanceof Error ? cause.message : "작업 노드를 제거하지 못했습니다.");
      },
    ).finally(() => pendingWorkIds.current.delete(workId));
  }, [client, onError, onRefresh]);
  const removeEdge = useCallback((edge: Edge) => {
    const edgeId = Number(edge.id.slice("work-edge-".length));
    if (!Number.isInteger(edgeId) || pendingEdgeIds.current.has(edgeId)) return;
    pendingEdgeIds.current.add(edgeId);
    setHiddenEdgeIds((current) => [...new Set([...current, edge.id])]);
    setSelectedEdgeId(null);
    void client.deleteWorkEdge(edgeId).then(
      async () => {
        await onRefresh();
      },
      (cause: unknown) => {
        setHiddenEdgeIds((current) => current.filter((id) => id !== edge.id));
        onError(cause instanceof Error ? cause.message : "작업 관계를 제거하지 못했습니다.");
      },
    ).finally(() => pendingEdgeIds.current.delete(edgeId));
  }, [client, onError, onRefresh]);
  const actions = useMemo(() => ({ removeWork }), [removeWork]);

  return (
    <div
      ref={stageRef}
      className="flow-stage project-memory-stage"
      tabIndex={-1}
      onKeyDownCapture={(event) => {
        if ((event.key === "Delete" || event.key === "Backspace") && selectedEdgeId !== null) {
          const edge = displayedEdges.find(({ id }) => id === selectedEdgeId);
          if (edge) {
            event.preventDefault();
            removeEdge(edge);
          }
          return;
        }
        if (event.key !== "Enter" && event.key !== " ") return;
        const target = event.target;
        if (!(target instanceof HTMLElement) || target.closest("button, input, select, a")) return;
        const workId = Number(
          target.closest(".react-flow__node")?.querySelector<HTMLElement>("[data-work-id]")
            ?.dataset.workId,
        );
        if (Number.isInteger(workId)) onOpenWork(workId);
      }}
    >
      <WorkNodeActionsContext.Provider value={actions}>
        <ReactFlow
          nodes={nodes}
          edges={displayedEdges}
          nodeTypes={nodeTypes}
          deleteKeyCode={null}
          onInit={setFlow}
          onMove={(_, viewport) => setZoom(viewport.zoom)}
          onNodesChange={(changes: NodeChange<WorkFlowNode>[]) => {
            setNodes((current) => applyNodeChanges(
              changes.filter((change) => change.type !== "remove"),
              current,
            ));
          }}
          onNodeDragStop={(_, node) => {
            void client.updateWork(node.data.work.id, {
              position_x: node.position.x,
              position_y: node.position.y,
            }).then(onRefresh).catch((cause: unknown) => {
              onError(cause instanceof Error ? cause.message : "작업 위치를 저장하지 못했습니다.");
              void onRefresh();
            });
          }}
          onConnect={(connection) => {
            const source = Number(connection.source?.slice("work-".length));
            const target = Number(connection.target?.slice("work-".length));
            if (!Number.isInteger(source) || !Number.isInteger(target)) return;
            void client.createWorkEdge(source, target).then(onRefresh).catch((cause: unknown) => {
              onError(cause instanceof Error ? cause.message : "작업 관계를 만들지 못했습니다.");
            });
          }}
          onNodeClick={(_, node) => onOpenWork(node.data.work.id)}
          onEdgeClick={(_, edge) => setSelectedEdgeId(edge.id)}
          onEdgeDoubleClick={(event, edge) => {
            event.stopPropagation();
            removeEdge(edge);
          }}
          onPaneClick={() => setSelectedEdgeId(null)}
          minZoom={0.5}
          maxZoom={2}
          zoomOnScroll
          zoomOnPinch
        >
          <Background
            id="project-memory-grid"
            variant={BackgroundVariant.Lines}
            gap={48}
            color="#1f2b30"
          />
          <Controls
            className="canvas-controls"
            fitViewOptions={{ padding: 0.14, minZoom: 0.62, maxZoom: 1.15 }}
            orientation="horizontal"
            position="bottom-center"
          >
            <span className="canvas-zoom-level" aria-label={`현재 확대 ${Math.round(zoom * 100)}%`}>
              {Math.round(zoom * 100)}%
            </span>
          </Controls>
          <Panel className="canvas-interaction-hint" position="bottom-right">
            선을 더블클릭해 제거 <span aria-hidden="true">·</span> 노드를 연결해 관계 표현
          </Panel>
        </ReactFlow>
      </WorkNodeActionsContext.Provider>
    </div>
  );
}
