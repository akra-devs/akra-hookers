import { useEffect, type RefObject } from "react";

const focusableSelector = [
  "button:not(:disabled)",
  "input:not(:disabled)",
  "select:not(:disabled)",
  "textarea:not(:disabled)",
  "[href]",
  "[tabindex]:not([tabindex='-1'])",
].join(",");

export function useDialogFocus(
  dialogRef: RefObject<HTMLElement | null>,
  initialSelector: string,
) {
  useEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog) return;
    const previous = document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
    const focusable = () => Array.from(
      dialog.querySelectorAll<HTMLElement>(focusableSelector),
    );
    (dialog.querySelector<HTMLElement>(initialSelector) ?? focusable()[0])?.focus();

    function containFocus(event: KeyboardEvent) {
      if (event.key !== "Tab") return;
      const candidates = focusable();
      const first = candidates[0];
      const last = candidates.at(-1);
      if (!first || !last) return;
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    }

    dialog.addEventListener("keydown", containFocus);
    return () => {
      dialog.removeEventListener("keydown", containFocus);
      if (previous?.isConnected) previous.focus();
    };
  }, [dialogRef, initialSelector]);
}
