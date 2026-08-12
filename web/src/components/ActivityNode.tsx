import { createContext, useContext } from "react";
import {
  Handle,
  Position,
  type Node,
  type NodeProps,
} from "@xyflow/react";

import type { ActivityNodeData } from "../canvas";
import { formatActivityTime } from "../time";
import { UiIcon } from "./UiIcon";

export type ActivityFlowNode = Node<ActivityNodeData, "activity">;

export const ActivityNodeActionsContext = createContext<{
  removeNode: (nodeId: string) => void;
} | null>(null);

export function ActivityNode({
  data,
  isConnectable,
}: NodeProps<ActivityFlowNode>) {
  const actions = useContext(ActivityNodeActionsContext);
  const projectName = data.project?.name ?? "분류 필요";
  const displayTime = formatActivityTime(data.time);
  const nodeId = `activity-${data.activityId}`;
  const resultStatus = data.resultSummaryStatus === "ready"
    ? "요약 있음"
    : data.resultSummaryStatus === "pending"
      ? "요약 중"
      : data.resultSummaryStatus === "failed"
        ? "요약 실패"
        : null;

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
      {actions && (
        <button
          className="activity-node__remove nodrag nopan"
          type="button"
          aria-label="캔버스에서 제거"
          title="캔버스에서 제거"
          onPointerDown={(event) => event.stopPropagation()}
          onDoubleClick={(event) => event.stopPropagation()}
          onClick={(event) => {
            event.stopPropagation();
            actions.removeNode(nodeId);
          }}
        >
          <UiIcon name="trash" size={15} />
        </button>
      )}
      <p className="activity-node__prompt">{data.prompt}</p>
      <div className="activity-node__meta">
        <span className="activity-node__source">
          <span className="activity-node__provider">
            <span className="activity-node__provider-dot" aria-hidden="true" />
            {data.provider}
          </span>
          {data.activityKind !== "user" && (
            <span className={`activity-node__kind activity-node__kind--${data.activityKind}`}>
              {data.activityKind}
            </span>
          )}
          {resultStatus && (
            <span
              className={`activity-node__result activity-node__result--${data.resultSummaryStatus}`}
            >
              {resultStatus}
            </span>
          )}
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
