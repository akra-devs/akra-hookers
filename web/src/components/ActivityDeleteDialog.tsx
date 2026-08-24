import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

import { useDialogFocus } from "./useDialogFocus";

type ActivityDeleteDialogProps = {
  activityId: number;
  prompt: string;
  onCancel: () => void;
  onConfirm: () => Promise<void>;
};

let bodyScrollLockCount = 0;
let bodyOverflowBeforeLock = "";

function useBodyScrollLock() {
  useEffect(() => {
    if (bodyScrollLockCount === 0) {
      bodyOverflowBeforeLock = document.body.style.overflow;
      document.body.style.overflow = "hidden";
    }
    bodyScrollLockCount += 1;
    return () => {
      bodyScrollLockCount = Math.max(0, bodyScrollLockCount - 1);
      if (bodyScrollLockCount === 0) {
        document.body.style.overflow = bodyOverflowBeforeLock;
      }
    };
  }, []);
}

function ExpandablePrompt({
  text,
  className = "",
}: {
  text: string;
  className?: string;
}) {
  const contentId = useId();
  const contentRef = useRef<HTMLParagraphElement>(null);
  const [expanded, setExpanded] = useState(false);
  const [canExpand, setCanExpand] = useState(false);

  useLayoutEffect(() => {
    setExpanded(false);
    const content = contentRef.current;
    if (!content) return;
    const measure = () => {
      const lineHeight = Number.parseFloat(window.getComputedStyle(content).lineHeight);
      const collapsedHeight = Number.isFinite(lineHeight)
        ? lineHeight * 4
        : content.clientHeight;
      setCanExpand(content.scrollHeight > collapsedHeight + 1);
    };
    const frame = window.requestAnimationFrame(measure);
    const observer = new ResizeObserver(measure);
    observer.observe(content);
    return () => {
      window.cancelAnimationFrame(frame);
      observer.disconnect();
    };
  }, [text]);

  return (
    <div className={`expandable-prompt${expanded ? " is-expanded" : ""}${className ? ` ${className}` : ""}`}>
      <p ref={contentRef} id={contentId}>{text}</p>
      {canExpand && (
        <button
          type="button"
          className="expandable-prompt__toggle"
          aria-controls={contentId}
          aria-expanded={expanded}
          onClick={() => setExpanded((current) => !current)}
        >
          {expanded ? "접기" : "더 보기"}
        </button>
      )}
    </div>
  );
}

export function ActivityDeleteDialog({
  activityId,
  prompt,
  onCancel,
  onConfirm,
}: ActivityDeleteDialogProps) {
  const dialogRef = useRef<HTMLElement>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  useDialogFocus(dialogRef, "[data-cancel-activity-delete]");
  useBodyScrollLock();

  const confirm = async () => {
    setBusy(true);
    setError("");
    try {
      await onConfirm();
      setBusy(false);
    } catch (cause) {
      setError(cause instanceof Error
        ? cause.message
        : "활동 기록을 삭제하지 못했습니다. 다시 시도하세요.");
      setBusy(false);
    }
  };

  return createPortal(
    <div className="dialog-backdrop activity-delete-dialog__backdrop">
      <section
        ref={dialogRef}
        className="dialog-card activity-delete-dialog"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="activity-delete-title"
        aria-describedby="activity-delete-description"
        onKeyDown={(event) => {
          if (event.key !== "Escape" || busy) return;
          event.stopPropagation();
          onCancel();
        }}
      >
        <div className="dialog-heading">
          <h2 id="activity-delete-title">활동 기록을 삭제할까요?</h2>
        </div>
        <p id="activity-delete-description" className="activity-delete-dialog__description">
          기록 #{activityId}가 활동 목록, 대화 흐름, Canvas에서 사라집니다. 이 작업은 화면에서 되돌릴 수 없습니다.
        </p>
        <ExpandablePrompt className="activity-delete-dialog__prompt" text={prompt} />
        {error && <p className="inline-error" role="alert">{error}</p>}
        <footer className="dialog-actions activity-delete-dialog__actions">
          <button
            data-cancel-activity-delete
            type="button"
            disabled={busy}
            onClick={onCancel}
          >
            취소
          </button>
          <button
            type="button"
            className="danger-button"
            disabled={busy}
            onClick={() => void confirm()}
          >
            {busy ? "삭제하는 중…" : "기록 삭제"}
          </button>
        </footer>
      </section>
    </div>,
    document.body,
  );
}
