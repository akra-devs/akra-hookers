import { useRef, useState, type FormEvent } from "react";

import type { ApiClient, ProjectSummary } from "../api";
import { useDialogFocus } from "./useDialogFocus";

type ProjectDialogProps = {
  client: ApiClient;
  projects: ProjectSummary[];
  project?: ProjectSummary;
  onClose: () => void;
  onChanged: (selectedProjectId?: number) => Promise<void>;
};

export function ProjectDialog({
  client,
  projects,
  project,
  onClose,
  onChanged,
}: ProjectDialogProps) {
  const [name, setName] = useState(project?.name ?? "");
  const targets = projects.filter((candidate) => candidate.id !== project?.id);
  const [targetId, setTargetId] = useState(targets[0]?.id ?? 0);
  const [confirmingMerge, setConfirmingMerge] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const dialogRef = useRef<HTMLElement>(null);
  useDialogFocus(dialogRef, "[data-dialog-initial]");

  async function submitName(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      if (project) {
        await client.renameProject(project.id, name);
        await onChanged(project.id);
      } else {
        await client.createProject(name);
        await onChanged();
      }
      onClose();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "프로젝트를 저장하지 못했습니다.");
    } finally {
      setBusy(false);
    }
  }

  async function mergeProject() {
    if (!project || targetId === 0) return;
    setBusy(true);
    setError(null);
    try {
      await client.mergeProject(project.id, targetId);
      await onChanged(targetId);
      onClose();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "프로젝트를 병합하지 못했습니다.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="dialog-backdrop">
      <section
        ref={dialogRef}
        className="dialog-card"
        role="dialog"
        aria-modal="true"
        aria-labelledby="project-dialog-title"
      >
        <div className="dialog-heading">
          <div>
            <p className="eyebrow">PROJECT</p>
            <h2 id="project-dialog-title">
              {project ? "프로젝트 관리" : "새 프로젝트"}
            </h2>
          </div>
          <button type="button" onClick={onClose}>닫기</button>
        </div>
        <form className="dialog-form" onSubmit={(event) => void submitName(event)}>
          <label className="field-label">
            프로젝트 이름
            <input
              data-dialog-initial
              value={name}
              onChange={(event) => setName(event.target.value)}
            />
          </label>
          <button type="submit" disabled={busy || name.trim().length === 0}>
            {project ? "이름 저장" : "프로젝트 만들기"}
          </button>
        </form>
        {project && targets.length > 0 && (
          <section className="dialog-section" aria-label="프로젝트 병합">
            <h3>프로젝트 병합</h3>
            <label className="field-label">
              병합 대상
              <select
                value={targetId}
                onChange={(event) => {
                  setTargetId(Number(event.target.value));
                  setConfirmingMerge(false);
                }}
              >
                {targets.map((target) => (
                  <option key={target.id} value={target.id}>{target.name}</option>
                ))}
              </select>
            </label>
            {confirmingMerge ? (
              <div className="confirmation-box">
                <p>이 프로젝트를 선택한 대상에 영구적으로 합칩니다.</p>
                <button type="button" disabled={busy} onClick={() => void mergeProject()}>
                  병합 확인
                </button>
              </div>
            ) : (
              <button type="button" onClick={() => setConfirmingMerge(true)}>
                병합...
              </button>
            )}
          </section>
        )}
        {error && <p className="inline-error" role="alert">{error}</p>}
      </section>
    </div>
  );
}
