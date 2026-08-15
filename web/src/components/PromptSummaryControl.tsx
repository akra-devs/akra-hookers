type PromptSummaryControlProps = {
  mode: "off" | "smart";
  available: boolean;
  collectorManaged: boolean;
  pending: boolean;
  error: string | null;
  onChange: (mode: "off" | "smart") => void;
};

export function PromptSummaryControl({
  mode,
  available,
  collectorManaged,
  pending,
  error,
  onChange,
}: PromptSummaryControlProps) {
  const smart = mode === "smart";
  return (
    <section
      className="prompt-summary-control"
      aria-labelledby="prompt-summary-heading"
      aria-busy={pending || undefined}
    >
      <div className="prompt-summary-control__heading">
        <div>
          <h3 id="prompt-summary-heading">문맥 기반 프롬프트 요약</h3>
          <p>
            {collectorManaged
              ? "원격 collector에 저장되는 활동은 collector 대시보드에서 정리합니다."
              : "현재 사용자 요청과 필요할 때 바로 이전 3줄 결과 요약만 짧게 정리합니다."}
          </p>
        </div>
        <label className="prompt-summary-control__toggle">
          <span className="sr-only">문맥 기반 프롬프트 요약</span>
          <input
            type="checkbox"
            aria-label="문맥 기반 프롬프트 요약"
            checked={smart}
            disabled={!available || collectorManaged || pending}
            onChange={(event) => onChange(event.target.checked ? "smart" : "off")}
          />
        </label>
      </div>
      <p className="prompt-summary-control__status" role="status" aria-live="polite">
        {collectorManaged
          ? "원격 수집 중 · collector 대시보드에서 요약 설정"
          : pending
          ? "요약 설정을 변경하는 중…"
          : smart
            ? "Smart · 앞선 결과 요약만 문맥으로 사용"
            : "Off · 제출한 원문을 그대로 표시"}
      </p>
      {error && <p className="prompt-summary-control__error" role="alert">{error}</p>}
    </section>
  );
}
