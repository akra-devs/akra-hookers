import type { ActivityDetail, OriginSummary } from "./api-contracts";

export type AssignmentSelection =
  | { state: "loading" }
  | { state: "empty" }
  | {
    state: "assignable";
    activityIds: number[];
    futureRoute: { defaultChecked: false } | null;
  }
  | { state: "dedicated"; originId: number }
  | { state: "blocked" };

export function getAssignmentSelection(
  details: ActivityDetail[] | undefined,
  origins: OriginSummary[],
): AssignmentSelection {
  if (details === undefined) return { state: "loading" };
  if (details.length === 0) return { state: "empty" };

  const matchedOrigins = details.map((detail) =>
    origins.find((origin) =>
      origin.id === detail.origin.id
    )
  );
  if (matchedOrigins.some((origin) => origin === undefined)) {
    return { state: "blocked" };
  }
  const resolvedOrigins = matchedOrigins.filter(
    (origin): origin is OriginSummary => origin !== undefined,
  );
  const dedicated = resolvedOrigins.filter(
    (origin) => origin.routing_mode === "dedicated",
  );
  if (dedicated.length > 0) {
    const originId = dedicated[0]?.id;
    return originId !== undefined
      && dedicated.length === resolvedOrigins.length
      && dedicated.every((origin) => origin.id === originId)
      ? { state: "dedicated", originId }
      : { state: "blocked" };
  }

  const first = details[0];
  const sameConversation = first !== undefined && details.every((detail) =>
    detail.provider === first.provider
    && detail.technical.session_id === first.technical.session_id
  );
  return {
    state: "assignable",
    activityIds: details.map(({ id }) => id),
    futureRoute: sameConversation ? { defaultChecked: false } : null,
  };
}
