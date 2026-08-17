export type ActivityPeriod = "all" | "today" | "day" | "week" | "month" | "quarter";

const HOUR_MS = 60 * 60 * 1_000;

export function localDayStartUs(now = new Date()): number {
  return new Date(
    now.getFullYear(),
    now.getMonth(),
    now.getDate(),
  ).getTime() * 1_000;
}

export function appendActivityPeriodParameters(
  parameters: URLSearchParams,
  period: ActivityPeriod,
  now = new Date(),
) {
  if (period === "all") return;
  parameters.set("period", period);
  if (period === "today") {
    parameters.set("start_at_us", String(localDayStartUs(now)));
  }
}

export function activityIsInPeriod(
  value: string | null,
  period: ActivityPeriod,
  now = new Date(),
): boolean {
  if (period === "all") return true;
  if (value === null) return false;
  const activityMs = Date.parse(value);
  if (!Number.isFinite(activityMs)) return false;
  if (period === "today") return activityMs >= localDayStartUs(now) / 1_000;
  const hours = {
    day: 24,
    week: 24 * 7,
    month: 24 * 30,
    quarter: 24 * 90,
  }[period];
  return activityMs >= now.getTime() - hours * HOUR_MS;
}
