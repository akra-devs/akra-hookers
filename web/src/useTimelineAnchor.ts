import { useEffect, useLayoutEffect, useRef } from "react";

export function useTimelineAnchor(selectedActivityId: number, timelineKey: string) {
  const listRef = useRef<HTMLOListElement | null>(null);
  const previousTop = useRef<number | null>(null);
  const previousSelectedId = useRef(selectedActivityId);

  useLayoutEffect(() => {
    const list = listRef.current;
    const selected = list?.querySelector<HTMLElement>(
      `[data-activity-id="${selectedActivityId}"]`,
    );
    if (list && selected) {
      const turnBounds = selected.getBoundingClientRect();
      const listBounds = list.getBoundingClientRect();
      if (previousTop.current !== null && previousSelectedId.current === selectedActivityId) {
        list.scrollTop += turnBounds.top - previousTop.current;
      } else if (turnBounds.bottom > listBounds.bottom) {
        list.scrollTop += turnBounds.bottom - listBounds.bottom;
      } else if (turnBounds.top < listBounds.top) {
        list.scrollTop -= listBounds.top - turnBounds.top;
      }
    }
    previousSelectedId.current = selectedActivityId;
    previousTop.current = selected?.getBoundingClientRect().top ?? null;
  }, [selectedActivityId, timelineKey]);

  useEffect(() => {
    const list = listRef.current;
    if (!list) return;
    const rememberTop = () => {
      previousTop.current = list.querySelector<HTMLElement>(
        `[data-activity-id="${selectedActivityId}"]`,
      )?.getBoundingClientRect().top ?? null;
    };
    list.addEventListener("scroll", rememberTop, { passive: true });
    return () => {
      list.removeEventListener("scroll", rememberTop);
    };
  }, [selectedActivityId]);

  return listRef;
}
