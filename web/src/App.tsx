import { useCallback, useEffect, useMemo, useState } from "react";
import { Background, Controls, ReactFlow, type Edge, type Node } from "@xyflow/react";
import { useQuery } from "@tanstack/react-query";
import "@xyflow/react/dist/style.css";

import { createApiClient } from "./api";
import { toCanvasNodes, type ActivityNodeData } from "./canvas";

const initialNodes: Node<ActivityNodeData>[] = [];

export function App() {
  const [nodes, setNodes] = useState(initialNodes);
  const [edges, setEdges] = useState<Edge[]>([]);
  const [codexEnabled, setCodexEnabled] = useState(true);
  const client = useMemo(() => {
    const url = import.meta.env.VITE_AKRA_URL;
    const token = import.meta.env.VITE_AKRA_TOKEN;
    return url && token ? createApiClient(url, token) : null;
  }, []);
  const activities = useQuery({
    queryKey: ["activities"],
    queryFn: () => client!.activities(),
    enabled: client !== null,
  });
  const canvas = useQuery({
    queryKey: ["canvas"],
    queryFn: () => client!.canvas(),
    enabled: client !== null,
  });
  const persistedEdges = useQuery({
    queryKey: ["canvas-edges"],
    queryFn: () => client!.edges(),
    enabled: client !== null,
  });
  useEffect(() => {
    if (activities.data && canvas.data) {
      setNodes(toCanvasNodes(activities.data, canvas.data));
    }
  }, [activities.data, canvas.data]);
  useEffect(() => {
    if (canvas.data && persistedEdges.data) {
      const idByCanvasNode = new Map(canvas.data.map((node) => [node.id, `activity-${node.activity_event_id}`]));
      setEdges(
        persistedEdges.data.flatMap((edge) => {
          const source = idByCanvasNode.get(edge.source_node_id);
          const target = idByCanvasNode.get(edge.target_node_id);
          return source && target ? [{ id: `edge-${edge.id}`, source, target }] : [];
        }),
      );
    }
  }, [canvas.data, persistedEdges.data]);
  const nodeCount = useMemo(() => nodes.length, [nodes.length]);
  const removeSelectedNode = useCallback((nodeId: string) => {
    setNodes((current) => current.filter((node) => node.id !== nodeId));
    const canvasNode = canvas.data?.find((node) => `activity-${node.activity_event_id}` === nodeId);
    if (client && canvasNode) {
      void client.deleteCanvasNode(canvasNode.id);
    }
  }, [canvas.data, client]);

  return (
    <main className="app-shell">
      <aside className="rail">
        <p className="eyebrow">LOCAL ACTIVITY MAP</p>
        <h1>akra-hookers</h1>
        <p className="muted">{nodeCount} canvas nodes</p>
        <section className="provider-control" aria-label="Provider settings">
          <p className="eyebrow">SETTINGS</p>
          <label>
            <span>Codex capture</span>
            <input
              type="checkbox"
              checked={codexEnabled}
              onChange={(event) => {
                const enabled = event.target.checked;
                setCodexEnabled(enabled);
                if (client) {
                  void client.setProviderEnabled("codex", enabled);
                }
              }}
            />
          </label>
          <small>Changes future capture only.</small>
        </section>
      </aside>
      <section className="canvas-panel">
        <header>
          <div>
            <p className="eyebrow">PROJECT ACTIVITY</p>
            <h2>Prompt canvas</h2>
          </div>
          <button type="button" onClick={() => setNodes([])}>Clear canvas</button>
        </header>
        <ReactFlow
          nodes={nodes}
          edges={edges}
          onNodesDelete={(deleted) => deleted.forEach((node) => removeSelectedNode(node.id))}
          onNodeDragStop={(_, node) => {
            const canvasNode = canvas.data?.find(
              (candidate) => `activity-${candidate.activity_event_id}` === node.id,
            );
            if (client && canvasNode) {
              void client.updateCanvasPosition(canvasNode.id, node.position);
            }
          }}
          onConnect={(connection) => {
            const sourceNode = canvas.data?.find((node) => `activity-${node.activity_event_id}` === connection.source);
            const targetNode = canvas.data?.find((node) => `activity-${node.activity_event_id}` === connection.target);
            if (client && sourceNode && targetNode) {
              void client.createCanvasEdge(sourceNode.id, targetNode.id).then(() => {
                void persistedEdges.refetch();
              });
            }
          }}
          fitView
        >
          <Background />
          <Controls />
        </ReactFlow>
        {nodes.length === 0 && (
          <div className="empty-state">
            <strong>No activity on this canvas</strong>
            <span>Submitted provider prompts appear here; removing a node never deletes its activity record.</span>
          </div>
        )}
      </section>
    </main>
  );
}
