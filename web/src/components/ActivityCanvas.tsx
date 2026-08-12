import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type Dispatch,
  type KeyboardEvent as ReactKeyboardEvent,
  type SetStateAction,
} from "react";
import {
  Background,
  BackgroundVariant,
  Controls,
  Panel,
  ReactFlow,
  type Edge,
  type NodeChange,
  type NodeTypes,
  type ReactFlowInstance,
  type XYPosition,
} from "@xyflow/react";

import type { ApiClient, CanvasNode } from "../api";
import {
  ActivityNodeActionsContext,
  type ActivityFlowNode,
} from "./ActivityNode";
import { useFitFlow } from "../useFitFlow";

type DragState = {
  pointer: XYPosition;
  positions: Map<string, XYPosition>;
};

function pointerPosition(event: MouseEvent | TouchEvent): XYPosition | null {
  if ("clientX" in event) return { x: event.clientX, y: event.clientY };
  const touch = event.touches[0] ?? event.changedTouches[0];
  return touch ? { x: touch.clientX, y: touch.clientY } : null;
}

type ActivityCanvasProps = {
  client: ApiClient | null;
  canvasNodes: CanvasNode[];
  nodes: ActivityFlowNode[];
  setNodes: Dispatch<SetStateAction<ActivityFlowNode[]>>;
  onNodesChange: (changes: NodeChange<ActivityFlowNode>[]) => void;
  edges: Edge[];
  nodeTypes: NodeTypes;
  onActivityOpen: (activityId: number) => void;
  onPositionCommit: (activityId: number, position: { x: number; y: number }) => Promise<void>;
  onError: (message: string) => void;
  onPersistedChange: () => Promise<unknown>;
  onAuthoritativeRefresh: () => Promise<unknown>;
  onSelectionChange: (selection: { nodes: ActivityFlowNode[] }) => void;
};

export function ActivityCanvas({
  client,
  canvasNodes,
  nodes,
  setNodes,
  onNodesChange,
  edges,
  nodeTypes,
  onActivityOpen,
  onPositionCommit,
  onError,
  onPersistedChange,
  onAuthoritativeRefresh,
  onSelectionChange,
}: ActivityCanvasProps) {
  const [flow, setFlow] = useState<ReactFlowInstance<ActivityFlowNode, Edge> | null>(null);
  const [selectedEdgeId, setSelectedEdgeId] = useState<string | null>(null);
  const [hiddenEdgeIds, setHiddenEdgeIds] = useState<string[]>([]);
  const [zoom, setZoom] = useState(1);
  const dragState = useRef<DragState | null>(null);
  const pendingNodeIds = useRef(new Set<string>());
  const pendingEdgeIds = useRef(new Set<string>());
  const openNodeTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const fitKey = nodes.map(({ id }) => id).sort().join(":");
  const flowStageRef = useFitFlow(flow, fitKey);
  const displayedEdges = edges
    .filter((edge) => !hiddenEdgeIds.includes(edge.id))
    .map((edge) => ({
      ...edge,
      selected: edge.id === selectedEdgeId,
    }));
  useEffect(() => {
    setHiddenEdgeIds((current) => current.filter(
      (id) => edges.some((edge) => edge.id === id),
    ));
  }, [edges]);
  const persistedNode = useCallback((nodeId: string) =>
    canvasNodes.find(({ activity_event_id }) => `activity-${activity_event_id}` === nodeId),
  [canvasNodes]);
  const removeNode = useCallback((nodeId: string) => {
    const canvasNode = persistedNode(nodeId);
    if (!client || !canvasNode || pendingNodeIds.current.has(nodeId)) return;

    pendingNodeIds.current.add(nodeId);
    const removedNode = nodes.find((node) => node.id === nodeId);
    const removedIndex = nodes.findIndex((node) => node.id === nodeId);
    const connectedEdgeIds = edges
      .filter((edge) => edge.source === nodeId || edge.target === nodeId)
      .map((edge) => edge.id);
    setNodes((current) => current.filter((node) => node.id !== nodeId));
    setHiddenEdgeIds((current) => [...new Set([...current, ...connectedEdgeIds])]);
    setSelectedEdgeId((current) => connectedEdgeIds.includes(current ?? "") ? null : current);
    onSelectionChange({
      nodes: nodes.filter((node) => node.id !== nodeId && node.selected),
    });

    void client.deleteCanvasNode(canvasNode.id).then(
      async () => {
        try {
          await onPersistedChange();
        } catch {
          onError("활동은 제거했지만 최신 캔버스 상태를 불러오지 못했습니다.");
        }
      },
      async (cause: unknown) => {
        if (removedNode) {
          setNodes((current) => {
            if (current.some((node) => node.id === nodeId)) return current;
            const restored = [...current];
            restored.splice(Math.min(Math.max(removedIndex, 0), restored.length), 0, removedNode);
            return restored;
          });
        }
        setHiddenEdgeIds((current) => current.filter((id) => !connectedEdgeIds.includes(id)));
        try {
          await onAuthoritativeRefresh();
        } catch {
          // The local rollback keeps the card recoverable when refresh also fails.
        }
        onError(cause instanceof Error ? cause.message : "캔버스에서 활동을 제거하지 못했습니다.");
      },
    ).finally(() => {
      pendingNodeIds.current.delete(nodeId);
    });
  }, [
    client,
    edges,
    nodes,
    onAuthoritativeRefresh,
    onError,
    onPersistedChange,
    onSelectionChange,
    persistedNode,
    setNodes,
  ]);
  const removeEdge = useCallback((edge: Edge) => {
    const edgeId = Number(edge.id.slice("edge-".length));
    if (!client || !Number.isInteger(edgeId) || pendingEdgeIds.current.has(edge.id)) return;

    pendingEdgeIds.current.add(edge.id);
    setHiddenEdgeIds((current) => current.includes(edge.id) ? current : [...current, edge.id]);
    setSelectedEdgeId((current) => current === edge.id ? null : current);
    void client.deleteCanvasEdge(edgeId).then(
      async () => {
        try {
          await onPersistedChange();
        } catch {
          onError("연결은 제거했지만 최신 캔버스 상태를 불러오지 못했습니다.");
        }
      },
      (cause: unknown) => {
        setHiddenEdgeIds((current) => current.filter((id) => id !== edge.id));
        onError(cause instanceof Error ? cause.message : "캔버스 연결을 제거하지 못했습니다.");
      },
    ).finally(() => {
      pendingEdgeIds.current.delete(edge.id);
    });
  }, [client, onError, onPersistedChange]);
  const nodeActions = useMemo(() => ({ removeNode }), [removeNode]);
  useEffect(() => () => {
    if (openNodeTimer.current !== null) clearTimeout(openNodeTimer.current);
  }, []);
  const openNode = useCallback((activityId: number) => {
    if (openNodeTimer.current !== null) clearTimeout(openNodeTimer.current);
    openNodeTimer.current = setTimeout(() => {
      openNodeTimer.current = null;
      onActivityOpen(activityId);
    }, 180);
  }, [onActivityOpen]);
  const openKeyboardNode = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "Enter" && event.key !== " ") return;
    const target = event.target;
    if (!(target instanceof HTMLElement)
      || target.closest("button, input, select, summary, a")) return;
    const activity = target
      .closest(".react-flow__node")
      ?.querySelector<HTMLElement>("[data-activity-id]");
    const activityId = Number(activity?.dataset.activityId);
    if (Number.isInteger(activityId)) onActivityOpen(activityId);
  };

  return (
    <div
      className="flow-stage"
      ref={flowStageRef}
      tabIndex={-1}
      onKeyDownCapture={openKeyboardNode}
    >
      <ActivityNodeActionsContext.Provider value={nodeActions}>
      <ReactFlow
        nodes={nodes}
        edges={displayedEdges}
        nodeTypes={nodeTypes}
        deleteKeyCode={["Backspace", "Delete"]}
        onInit={setFlow}
        onMove={(_, viewport) => setZoom(viewport.zoom)}
        onNodesChange={(changes) => onNodesChange(
          dragState.current
            ? changes.filter((change) => change.type !== "position")
            : changes,
        )}
        onNodesDelete={(deleted) => {
          deleted.forEach((node) => removeNode(node.id));
        }}
        onEdgesDelete={(deleted) => {
          deleted.forEach(removeEdge);
        }}
        onNodeDragStart={(event) => {
          const pointer = pointerPosition(event);
          dragState.current = pointer ? { pointer, positions: new Map() } : null;
        }}
        onNodeDrag={(event, _, draggedNodes) => {
          const current = dragState.current;
          const pointer = pointerPosition(event);
          if (!current || !pointer
            || (pointer.x === current.pointer.x && pointer.y === current.pointer.y)) return;
          current.pointer = pointer;
          current.positions = new Map(draggedNodes.map((draggedNode) => [
            draggedNode.id,
            { ...draggedNode.position },
          ]));
          onNodesChange(draggedNodes.map((draggedNode) => ({
            id: draggedNode.id,
            type: "position" as const,
            position: draggedNode.position,
            dragging: true,
          })));
        }}
        onNodeDragStop={() => {
          const accepted = dragState.current?.positions;
          dragState.current = null;
          if (!accepted?.size) return;
          onNodesChange(Array.from(accepted, ([id, position]) => ({
            id,
            type: "position" as const,
            position,
            dragging: false,
          })));
          const activityIds = new Map(nodes.map((currentNode) => [
            currentNode.id,
            currentNode.data.activityId,
          ]));
          const commits = Array.from(accepted, ([id, position]) => {
            const activityId = activityIds.get(id);
            return activityId === undefined
              ? Promise.reject(new Error(`Missing activity for dragged node ${id}`))
              : onPositionCommit(activityId, position);
          });
          void Promise.all(commits)
            .catch((cause: unknown) => {
              onError(cause instanceof Error ? cause.message : "Could not save canvas position.");
            });
        }}
        onConnect={(connection) => {
          const source = connection.source ? persistedNode(connection.source) : undefined;
          const target = connection.target ? persistedNode(connection.target) : undefined;
          if (!client || !source || !target) return;
          void client.createCanvasEdge(source.id, target.id)
            .then(onPersistedChange)
            .catch((cause: unknown) => {
              onError(cause instanceof Error ? cause.message : "Could not create canvas edge.");
            });
        }}
        onNodeClick={(_, node) => openNode(node.data.activityId)}
        onEdgeClick={(_, edge) => setSelectedEdgeId(edge.id)}
        onEdgeDoubleClick={(event, edge) => {
          event.stopPropagation();
          removeEdge(edge);
        }}
        onPaneClick={() => setSelectedEdgeId(null)}
        onSelectionChange={onSelectionChange}
        minZoom={0.5}
        maxZoom={2}
        zoomOnScroll
        zoomOnPinch
      >
        <Background
          id="spatial-studio-grid"
          variant={BackgroundVariant.Lines}
          gap={48}
          color="#1f2b30"
        />
        <Controls
          className="canvas-controls"
          fitViewOptions={{ padding: 0.12, minZoom: 0.64 }}
          orientation="horizontal"
          position="bottom-center"
        >
          <span className="canvas-zoom-level" aria-label={`현재 확대 ${Math.round(zoom * 100)}%`}>
            {Math.round(zoom * 100)}%
          </span>
        </Controls>
        <Panel className="canvas-interaction-hint" position="bottom-right">
          Wheel to zoom <span aria-hidden="true">·</span> Drag to pan
        </Panel>
      </ReactFlow>
      </ActivityNodeActionsContext.Provider>
    </div>
  );
}
