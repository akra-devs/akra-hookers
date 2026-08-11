import {
  useRef,
  useState,
  type Dispatch,
  type KeyboardEvent as ReactKeyboardEvent,
  type SetStateAction,
} from "react";
import {
  Background,
  Controls,
  ReactFlow,
  type Edge,
  type NodeChange,
  type NodeTypes,
  type ReactFlowInstance,
  type XYPosition,
} from "@xyflow/react";

import type { ApiClient, CanvasNode } from "../api";
import type { ActivityFlowNode } from "./ActivityNode";
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
  onSelectionChange,
}: ActivityCanvasProps) {
  const [flow, setFlow] = useState<ReactFlowInstance<ActivityFlowNode, Edge> | null>(null);
  const [selectedEdgeId, setSelectedEdgeId] = useState<string | null>(null);
  const dragState = useRef<DragState | null>(null);
  const fitKey = nodes.map(({ id }) => id).sort().join(":");
  const flowStageRef = useFitFlow(flow, fitKey);
  const displayedEdges = edges.map((edge) => ({
    ...edge,
    selected: edge.id === selectedEdgeId,
  }));
  const persistedNode = (nodeId: string) =>
    canvasNodes.find(({ activity_event_id }) => `activity-${activity_event_id}` === nodeId);
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
    <div className="flow-stage" ref={flowStageRef} onKeyDownCapture={openKeyboardNode}>
      <ReactFlow
        nodes={nodes}
        edges={displayedEdges}
        nodeTypes={nodeTypes}
        deleteKeyCode={["Backspace", "Delete"]}
        onInit={setFlow}
        onNodesChange={(changes) => onNodesChange(
          dragState.current
            ? changes.filter((change) => change.type !== "position")
            : changes,
        )}
        onNodesDelete={(deleted) => {
          deleted.forEach((node) => {
            const canvasNode = persistedNode(node.id);
            if (!client || !canvasNode) return;
            void client.deleteCanvasNode(canvasNode.id)
              .then(() => {
                setNodes((current) => current.filter((currentNode) => currentNode.id !== node.id));
                return onPersistedChange();
              })
              .catch((cause: unknown) => {
                onError(cause instanceof Error ? cause.message : "Could not remove canvas node.");
              });
          });
        }}
        onEdgesDelete={(deleted) => {
          deleted.forEach((edge) => {
            const edgeId = Number(edge.id.slice("edge-".length));
            if (!client || !Number.isInteger(edgeId)) return;
            void client.deleteCanvasEdge(edgeId)
              .then(() => {
                setSelectedEdgeId((current) => current === edge.id ? null : current);
                return onPersistedChange();
              })
              .catch((cause: unknown) => {
                onError(cause instanceof Error ? cause.message : "Could not remove canvas edge.");
              });
          });
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
        onNodeClick={(_, node) => onActivityOpen(node.data.activityId)}
        onEdgeClick={(_, edge) => setSelectedEdgeId(edge.id)}
        onPaneClick={() => setSelectedEdgeId(null)}
        onSelectionChange={onSelectionChange}
        minZoom={1}
        maxZoom={1}
      >
        <Background />
        <Controls />
      </ReactFlow>
    </div>
  );
}
