import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type FormEvent,
} from "react";

import type { CollectorIntegration } from "../api";
import { validateCollectorEndpoint } from "../collector-endpoint";

export type CollectorOperation = "configure" | "verify" | null;

type CollectorEndpointControlProps = {
  collector: CollectorIntegration;
  available: boolean;
  operation: CollectorOperation;
  onConfigure: (endpoint: string, token?: string) => Promise<void>;
  onVerify: () => Promise<void>;
};

const TOKEN_REQUIRED_ERROR = "새 원격 수집 주소에는 collector access token이 필요합니다.";

function deliveryTime(capturedAtUs: number) {
  return new Intl.DateTimeFormat("ko-KR", {
    month: "numeric",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(capturedAtUs / 1_000));
}

function statusFor(
  collector: CollectorIntegration,
  operation: CollectorOperation,
  available: boolean,
) {
  if (!available || operation !== null) {
    return { label: operation === "configure" ? "Saving" : "Checking", tone: "pending" };
  }
  if (collector.last_error) return { label: "Delivery issue", tone: "error" };
  if (collector.pending_count > 0) return { label: `${collector.pending_count} queued`, tone: "warning" };
  if (collector.mode === "local") return { label: "Local", tone: "healthy" };
  if (collector.connected === true) return { label: "Connected", tone: "healthy" };
  return { label: "Not verified", tone: "warning" };
}

export function CollectorEndpointControl({
  collector,
  available,
  operation,
  onConfigure,
  onVerify,
}: CollectorEndpointControlProps) {
  const [editing, setEditing] = useState(false);
  const [endpoint, setEndpoint] = useState(collector.endpoint);
  const [token, setToken] = useState("");
  const [revealToken, setRevealToken] = useState(false);
  const [touched, setTouched] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const changeButton = useRef<HTMLButtonElement>(null);
  const endpointField = useRef<HTMLInputElement>(null);
  const previousEndpoint = useRef(collector.endpoint);

  const validation = useMemo(() => validateCollectorEndpoint(endpoint), [endpoint]);
  const persistedValidation = useMemo(
    () => validateCollectorEndpoint(collector.endpoint),
    [collector.endpoint],
  );
  const isRemote = validation.ok && validation.value.mode === "remote";
  const sameEndpoint = validation.ok
    && persistedValidation.ok
    && validation.value.endpoint === persistedValidation.value.endpoint;
  const originChanged = isRemote && (!sameEndpoint || collector.mode !== "remote");
  const tokenRequired = isRemote && (originChanged || !collector.token_configured);
  const busy = operation !== null || !available;
  const status = statusFor(collector, operation, available);
  const endpointError = touched && !validation.ok ? validation.error : null;

  useEffect(() => {
    if (previousEndpoint.current === collector.endpoint) return;
    previousEndpoint.current = collector.endpoint;
    if (!editing) setEndpoint(collector.endpoint);
  }, [collector.endpoint, editing]);

  useEffect(() => {
    if (editing) endpointField.current?.focus();
  }, [editing]);

  const openEditor = () => {
    setEndpoint(collector.endpoint);
    setToken("");
    setRevealToken(false);
    setTouched(false);
    setError(null);
    setNotice(null);
    setEditing(true);
  };

  const cancelEditor = () => {
    setEndpoint(collector.endpoint);
    setToken("");
    setRevealToken(false);
    setTouched(false);
    setError(null);
    setEditing(false);
    requestAnimationFrame(() => changeButton.current?.focus());
  };

  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setTouched(true);
    setError(null);
    setNotice(null);
    if (!validation.ok) {
      setError(validation.error);
      return;
    }
    if (tokenRequired && !token.trim()) {
      setError(TOKEN_REQUIRED_ERROR);
      return;
    }
    try {
      await onConfigure(
        validation.value.endpoint,
        isRemote && token.trim() ? token.trim() : undefined,
      );
      setToken("");
      setRevealToken(false);
      setEditing(false);
      setNotice(
        validation.value.mode === "local"
          ? "로컬 수집 위치가 적용되었습니다. 캡처 데이터는 이 장치에 남습니다."
          : "원격 수집 위치가 적용되었습니다. Capture hooks를 다시 시작할 필요가 없습니다.",
      );
      requestAnimationFrame(() => changeButton.current?.focus());
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "수집 위치를 저장하지 못했습니다.");
    }
  };

  const verify = async () => {
    setError(null);
    setNotice(null);
    try {
      await onVerify();
      setNotice("Collector 연결을 확인했습니다.");
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Collector 연결을 확인하지 못했습니다.");
    }
  };

  const tokenHint = tokenRequired
    ? "외부 수집기에서 발급한 access token을 입력하세요."
    : "Access token saved. 비워 두면 같은 수집기의 저장된 token을 유지합니다.";

  return (
    <section
      id="collector-settings"
      className="collector-control"
      aria-labelledby="collector-settings-heading"
    >
      <header className="collector-control__header">
        <div>
          <h3 id="collector-settings-heading">Collection destination</h3>
          <p>
            {collector.mode === "local"
              ? "Captured data stays in this device's Akra data directory."
              : "Remote collection sends captured prompts, work paths, session/turn metadata, and final assistant results to this HTTPS address."}
          </p>
        </div>
        <span
          className={`collector-status collector-status--${status.tone}`}
          data-testid="collector-status"
          role="status"
          aria-live="polite"
        >
          <i aria-hidden="true" />
          {status.label}
        </span>
      </header>

      <div className="collector-control__readout">
        <span className={`collector-mode collector-mode--${collector.mode}`} data-testid="collector-mode">
          {collector.mode.toUpperCase()}
        </span>
        <code data-testid="collector-endpoint" dir="ltr" title={collector.endpoint}>
          {collector.endpoint}
        </code>
        {!editing && (
          <button
            ref={changeButton}
            type="button"
            disabled={busy}
            onClick={openEditor}
          >
            Change destination
          </button>
        )}
      </div>

      {!editing && collector.mode === "remote" && (
        <div className="collector-control__actions">
          <button type="button" disabled={busy} onClick={() => void verify()}>
            {operation === "verify" ? "Checking…" : "Verify connection"}
          </button>
          {collector.token_configured && <small>Access token saved</small>}
        </div>
      )}

      {editing && (
        <form
          className="collector-form"
          data-testid="collector-form"
          aria-busy={operation === "configure"}
          noValidate
          onSubmit={(event) => void submit(event)}
        >
          <label className="collector-field" htmlFor="collector-endpoint">
            <span>Collector URL</span>
            <input
              id="collector-endpoint"
              ref={endpointField}
              type="url"
              inputMode="url"
              autoCapitalize="none"
              autoCorrect="off"
              spellCheck={false}
              autoComplete="url"
              maxLength={2048}
              value={endpoint}
              disabled={busy}
              aria-invalid={endpointError ? "true" : undefined}
              aria-errormessage={endpointError ? "collector-form-error" : undefined}
              aria-describedby="collector-endpoint-hint"
              placeholder="http://127.0.0.1:42130"
              onBlur={() => setTouched(true)}
              onChange={(event) => {
                setEndpoint(event.target.value);
                setError(null);
                setNotice(null);
              }}
            />
          </label>
          <small id="collector-endpoint-hint" className="collector-field__hint">
            Local: <code>http://127.0.0.1:port</code> · Remote: <code>https://host</code>
          </small>

          {isRemote && (
            <label className="collector-field" htmlFor="collector-token">
              <span>Collector access token</span>
              <span className="collector-token-row">
                <input
                  id="collector-token"
                  type={revealToken ? "text" : "password"}
                  autoComplete="new-password"
                  maxLength={512}
                  value={token}
                  disabled={busy}
                  required={tokenRequired}
                  aria-invalid={error === TOKEN_REQUIRED_ERROR ? "true" : undefined}
                  aria-errormessage={error === TOKEN_REQUIRED_ERROR ? "collector-form-error" : undefined}
                  aria-describedby="collector-token-hint"
                  placeholder={tokenRequired ? "Paste access token" : "Leave blank to keep saved token"}
                  onChange={(event) => {
                    setToken(event.target.value);
                    setError(null);
                    setNotice(null);
                  }}
                />
                <button
                  type="button"
                  disabled={busy || token.length === 0}
                  aria-pressed={revealToken}
                  onClick={() => setRevealToken((current) => !current)}
                >
                  {revealToken ? "Hide collector access token" : "Show collector access token"}
                </button>
              </span>
              <small id="collector-token-hint">{tokenHint}</small>
            </label>
          )}

          <div className="collector-actions">
            <button className="collector-save" type="submit" disabled={busy}>
              {operation === "configure" ? "Saving…" : "Save destination"}
            </button>
            <button type="button" disabled={busy} onClick={cancelEditor}>Cancel</button>
          </div>
        </form>
      )}

      {(collector.pending_count > 0 || collector.last_delivery_at_us !== null) && (
        <p className="collector-delivery" role="status">
          {collector.pending_count > 0
            ? `${collector.pending_count} capture${collector.pending_count === 1 ? " is" : "s are"} waiting to send.`
            : `Last delivered ${deliveryTime(collector.last_delivery_at_us!)}.`}
        </p>
      )}

      <div className="collector-feedback" aria-live="polite">
        {(endpointError || error) && (
          <p id="collector-form-error" className="capture-control-error" role="alert">
            {endpointError ?? error}
          </p>
        )}
        {!endpointError && !error && notice && <p className="collector-notice">{notice}</p>}
        {!endpointError && !error && !notice && collector.last_error && (
          <p className="capture-control-error" role="alert">{collector.last_error}</p>
        )}
      </div>
    </section>
  );
}
