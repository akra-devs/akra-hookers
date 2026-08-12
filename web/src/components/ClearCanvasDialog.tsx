import { useRef, useState } from "react";

import { useDialogFocus } from "./useDialogFocus";

type ClearCanvasDialogProps = {
  nodeCount: number;
  onCancel: () => void;
  onConfirm: () => Promise<boolean>;
};

export function ClearCanvasDialog({
  nodeCount,
  onCancel,
  onConfirm,
}: ClearCanvasDialogProps) {
  const dialogRef = useRef<HTMLElement>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  useDialogFocus(dialogRef, "[data-cancel-clear]");

  const confirm = async () => {
    setBusy(true);
    setError("");
    try {
      if (!await onConfirm()) {
        setError("Canvas를 비우지 못했습니다. 다시 시도하세요.");
      }
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="dialog-backdrop">
      <section
        ref={dialogRef}
        className="dialog-card clear-canvas-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="clear-canvas-title"
        aria-describedby="clear-canvas-description"
      >
        <div className="dialog-heading">
          <div>
            <p className="eyebrow">CANVAS ACTION</p>
            <h2 id="clear-canvas-title">Canvas를 비울까요?</h2>
          </div>
        </div>
        <p id="clear-canvas-description" className="clear-canvas-dialog__description">
          활동 {nodeCount}개의 배치와 연결만 제거합니다. 저장된 prompt history는 유지됩니다.
        </p>
        {error && <p className="inline-error" role="alert">{error}</p>}
        <footer className="dialog-actions clear-canvas-dialog__actions">
          <button data-cancel-clear type="button" disabled={busy} onClick={onCancel}>
            취소
          </button>
          <button
            className="danger-button"
            type="button"
            disabled={busy}
            onClick={() => void confirm()}
          >
            {busy ? "비우는 중…" : "Canvas 비우기"}
          </button>
        </footer>
      </section>
    </div>
  );
}
