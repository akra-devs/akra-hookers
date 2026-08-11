import {
  Handle,
  Position,
  type Node,
  type NodeProps,
} from "@xyflow/react";

import type { ActivityNodeData } from "../canvas";
import { formatActivityTime } from "../time";

export type ActivityFlowNode = Node<ActivityNodeData, "activity">;

export function ActivityNode({
  data,
  isConnectable,
}: NodeProps<ActivityFlowNode>) {
  const projectName = data.project?.name ?? "분류 필요";
  const displayTime = formatActivityTime(data.time);

  return (
    <article
      className="activity-node"
      data-activity-id={data.activityId}
      data-testid={`activity-node-${data.activityId}`}
      aria-label={`${projectName}: ${data.prompt}`}
    >
      <Handle
        id="target"
        type="target"
        position={Position.Left}
        isConnectable={isConnectable}
        aria-label="연결 받기"
      />
      <div className="activity-node__context">
        <span className="activity-node__project">{projectName}</span>
        <span className="activity-node__conversation">
          {data.conversationIndex}/{data.conversationTotal}
        </span>
      </div>
      <p className="activity-node__prompt">{data.prompt}</p>
      <div className="activity-node__meta">
        <span className="activity-node__provider">
          <span className="activity-node__provider-dot" aria-hidden="true" />
          {data.provider}
        </span>
        <time dateTime={data.time.value ?? undefined}>{displayTime}</time>
      </div>
      <Handle
        id="source"
        type="source"
        position={Position.Right}
        isConnectable={isConnectable}
        aria-label="연결 시작"
      />
    </article>
  );
}
