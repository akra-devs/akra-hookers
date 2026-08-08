import { useCallback, useEffect, useMemo, useState } from "react";
import {
  Background,
  Controls,
  ReactFlow,
  applyNodeChanges,
  type Edge,
  type Node,
} from "@xyflow/react";
import { useQuery } from "@tanstack/react-query";
import "@xyflow/react/dist/style.css";

import { createApiClient } from "./api";
import { toCanvasNodes, type ActivityNodeData } from "./canvas";

const initialNodes: Node<ActivityNodeData>[] = [];

export function App() {
  const [nodes, setNodes] = useState(initialNodes);
  const [edges, setEdges] = useState<Edge[]>([]);
  const [codexEnabled, setCodexEnabled] = useState(true);
  const [selectedProject, setSelectedProject] = useState<string>();
  const [error, setError] = useState<string | null>(null);
  const client = useMemo(() => {
    const url = import.meta.env.VITE_AKRA_URL;
    const token = import.meta.env.VITE_AKRA_TOKEN;
    return url && token ? createApiClient(url, token) : null;
  }, []);
  const activities = useQuery({
    queryKey: ["activities", selectedProject],
    queryFn: () => client!.activities(selectedProject),
    enabled: client !== null,
  });
  const projects = useQuery({
    queryKey: ["projects"],
    queryFn: () => client!.projects(),
    enabled: client !== null,
  });
  const provider = useQuery({
    queryKey: ["provider", "codex"],
    queryFn: () => client!.provider("codex"),
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
    if (provider.data) {
      setCodexEnabled(provider.data.enabled);
    }
  }, [provider.data]);
  useEffect(() => {
    if (activities.data && canvas.data && persistedEdges.data) {
      const visibleActivityIds = new Set(activities.data.map((activity) => activity.id));
      const idByCanvasNode = new Map(
        canvas.data
          .filter((node) => visibleActivityIds.has(node.activity_event_id))
          .map((node) => [node.id, `activity-${node.activity_event_id}`]),
      );
      setEdges(
        persistedEdges.data.flatMap((edge) => {
          const source = idByCanvasNode.get(edge.source_node_id);
          const target = idByCanvasNode.get(edge.target_node_id);
          return source && target ? [{ id: `edge-${edge.id}`, source, target }] : [];
        }),
      );
    }
  }, [activities.data, canvas.data, persistedEdges.data]);
  const nodeCount = useMemo(() => nodes.length, [nodes.length]);
  const removeSelectedNode = useCallback(async (nodeId: string) => {
    const canvasNode = canvas.data?.find((node) => `activity-${node.activity_event_id}` === nodeId);
    if (client && canvasNode) {
      try {
        await client.deleteCanvasNode(canvasNode.id);
        setNodes((current) => current.filter((node) => node.id !== nodeId));
        await Promise.all([canvas.refetch(), persistedEdges.refetch()]);
      } catch (cause) {
        setError(cause instanceof Error ? cause.message : "Could not remove canvas node.");
      }
    }
  }, [canvas, client, persistedEdges]);
  const clearCanvas = useCallback(async () => {
    if (!client) {
      return;
    }
    try {
      await client.clearCanvas();
      setNodes([]);
      setEdges([]);
      await Promise.all([canvas.refetch(), persistedEdges.refetch()]);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not clear canvas.");
    }
  }, [canvas, client, persistedEdges]);
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
                void changeProvider(event.target.checked);
              }}
            />
          </label>
          <small>Changes future capture only.</small>
        </section>
        <section className="provider-control" aria-label="Project filter">
          <p className="eyebrow">PROJECT</p>
          <select
            value={selectedProject ?? ""}
            onChange={(event) => setSelectedProject(event.target.value || undefined)}
          >
            <option value="">All projects</option>
            {projects.data?.map((project) => (
              <option key={project.identity} value={project.identity}>
                {project.display_path}
              </option>
            ))}
          </select>
        </section>
      </aside>
      <section className="canvas-panel">
        <header>
          <div>
            <p className="eyebrow">PROJECT ACTIVITY</p>
            <h2>Prompt canvas</h2>
          </div>
          <button type="button" onClick={() => void clearCanvas()}>Clear canvas</button>
        </header>
        <ReactFlow
          nodes={nodes}
          edges={edges}
          onNodesChange={(changes) => {
            setNodes((current) => applyNodeChanges(
              changes.filter((change) => change.type !== "remove"),
              current,
            ));
          }}
          onNodesDelete={(deleted) => {
            deleted.forEach((node) => {
              void removeSelectedNode(node.id);
            });
          }}
          onNodeDragStop={(_, node) => {
            const canvasNode = canvas.data?.find(
              (candidate) => `activity-${candidate.activity_event_id}` === node.id,
            );
            if (client && canvasNode) {
              void client.updateCanvasPosition(canvasNode.id, node.position).catch((cause: unknown) => {
                setError(cause instanceof Error ? cause.message : "Could not save canvas position.");
                void canvas.refetch();
              });
            }
          }}
          onConnect={(connection) => {
            const sourceNode = canvas.data?.find((node) => `activity-${node.activity_event_id}` === connection.source);
            const targetNode = canvas.data?.find((node) => `activity-${node.activity_event_id}` === connection.target);
            if (client && sourceNode && targetNode) {
              void client.createCanvasEdge(sourceNode.id, targetNode.id)
                .then(() => persistedEdges.refetch())
                .catch((cause: unknown) => {
                  setError(cause instanceof Error ? cause.message : "Could not create canvas edge.");
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
        {error && <p className="error-message" role="alert">{error}</p>}
      </section>
    </main>
  );
}
