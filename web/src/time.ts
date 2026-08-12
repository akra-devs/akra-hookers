import type { ActivityTime } from "./api";

export type ActivityTimeFormatOptions = {
  locale?: string | string[];
  timeZone?: string;
};

export function formatActivityTime(
  time: ActivityTime,
  options: ActivityTimeFormatOptions = {},
): string {
  if (time.value === null || time.provenance === "unknown") {
    return "시간 정보 없음";
  }
  const formatted = new Intl.DateTimeFormat(options.locale, {
    dateStyle: "medium",
    timeStyle: "short",
    timeZone: options.timeZone,
  }).format(new Date(time.value));
  return formatted;
}
