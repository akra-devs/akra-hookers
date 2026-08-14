export type CollectorEndpointMode = "local" | "remote";

export type ParsedCollectorEndpoint = {
  endpoint: string;
  mode: CollectorEndpointMode;
  origin: string;
};

export type CollectorEndpointValidation =
  | { ok: true; value: ParsedCollectorEndpoint }
  | { ok: false; error: string };

function rawAuthorityHost(input: string): string | null {
  const scheme = input.indexOf("://");
  if (scheme <= 0) return null;
  const authority = input.slice(scheme + 3).split(/[/?#]/, 1)[0] ?? "";
  if (!authority || authority.includes("@")) return null;
  if (authority.startsWith("[")) {
    const end = authority.indexOf("]");
    return end > 0 ? authority.slice(1, end) : null;
  }
  const lastColon = authority.lastIndexOf(":");
  if (lastColon >= 0) {
    const port = authority.slice(lastColon + 1);
    if (!/^\d+$/.test(port)) return null;
    return authority.slice(0, lastColon);
  }
  return authority;
}

function isLoopbackV4(host: string) {
  const octets = host.split(".");
  return octets.length === 4
    && octets[0] === "127"
    && octets.every((octet) => /^(0|[1-9]\d{0,2})$/.test(octet)
      && Number(octet) <= 255);
}

export function validateCollectorEndpoint(input: string): CollectorEndpointValidation {
  const candidate = input.trim();
  if (!candidate) {
    return { ok: false, error: "Collector URL을 입력하세요." };
  }

  let url: URL;
  try {
    url = new URL(candidate);
  } catch {
    return {
      ok: false,
      error: "http://127.0.0.1:port 또는 https://collector.example.com 형식으로 입력하세요.",
    };
  }

  if (url.username || url.password || candidate.includes("@")) {
    return { ok: false, error: "주소에 사용자 이름이나 비밀번호를 포함할 수 없습니다." };
  }
  if (url.port === "0") {
    return { ok: false, error: "포트 0은 사용할 수 없습니다." };
  }
  if (
    url.pathname !== "/"
    || url.search
    || url.hash
    || candidate.includes("?")
    || candidate.includes("#")
  ) {
    return { ok: false, error: "Collector URL에는 경로, query 또는 hash를 추가할 수 없습니다." };
  }

  const rawHost = rawAuthorityHost(candidate);
  if (!rawHost) {
    return { ok: false, error: "Collector URL의 host를 확인하세요." };
  }
  const host = rawHost.toLowerCase();
  if (
    host === "localhost."
    || /^\d+$/.test(host)
  ) {
    return { ok: false, error: "Collector URL의 host를 확인하세요." };
  }
  const isLoopback = host === "localhost" || host === "::1" || isLoopbackV4(host);

  if (isLoopback && url.protocol !== "http:") {
    return { ok: false, error: "로컬 수집 주소는 http://localhost 또는 http://127.x.x.x를 사용하세요." };
  }
  if (!isLoopback && url.protocol !== "https:") {
    return { ok: false, error: "외부 수집 주소는 HTTPS를 사용해야 합니다." };
  }

  return {
    ok: true,
    value: {
      endpoint: url.origin,
      mode: isLoopback ? "local" : "remote",
      origin: url.origin,
    },
  };
}
