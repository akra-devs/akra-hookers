import { createContext, useContext } from "react";
import { Handle, Position, type Node, type NodeProps } from "@xyflow/react";

import type { WorkItem } from "../api";
import { UiIcon } from "./UiIcon";

export type WorkNodeData = {
  work: WorkItem;
};

export type WorkFlowNode = Node<WorkNodeData, "work">;

export const WorkNodeActionsContext = createContext<{
  removeWork: (workId: number) => void;
} | null>(null);

function promptText(work: WorkItem) {
  return work.preview_logs.map((log) => log.prompt_summary.text ?? log.prompt);
}

export function WorkNode({ data, isConnectable }: NodeProps<WorkFlowNode>) {
  const actions = useContext(WorkNodeActionsContext);
  const work = data.work;
  const previews = promptText(work);
  return (
    <article
      className="work-node"
      data-work-id={work.id}
      data-testid={`work-node-${work.id}`}
      aria-label={`${work.project.name}: ${work.title}, 로그 ${work.log_count}개`}
    >
      <Handle
        id="target"
        type="target"
        position={Position.Left}
        isConnectable={isConnectable}
        aria-label="작업 관계 받기"
      />
      <div className="work-node__context">
        <span>{work.project.name}</span>
        <span>LOG {work.log_count}</span>
      </div>
      {actions && (
        <button
          className="work-node__remove nodrag nopan"
          type="button"
          aria-label={`${work.title} 작업 노드 제거`}
          title="작업 노드 제거 · 원본 로그는 다시 정리 대기 상태가 됩니다"
          onPointerDown={(event) => event.stopPropagation()}
          onDoubleClick={(event) => event.stopPropagation()}
          onClick={(event) => {
            event.stopPropagation();
            actions.removeWork(work.id);
          }}
        >
          <UiIcon name="trash" size={15} />
        </button>
      )}
      <h3>{work.title}</h3>
      <ol className="work-node__preview" aria-label="포함된 로그 미리보기">
        {previews.map((preview, index) => (
          <li key={`${work.preview_logs[index]?.id ?? index}-${preview}`}>{preview}</li>
        ))}
      </ol>
      <div className="work-node__meta">
        <span><UiIcon name="work" size={14} /> 사용자 확인</span>
        {work.log_count > previews.length && <span>+{work.log_count - previews.length}</span>}
      </div>
      <Handle
        id="source"
        type="source"
        position={Position.Right}
        isConnectable={isConnectable}
        aria-label="작업 관계 시작"
      />
    </article>
  );
}
