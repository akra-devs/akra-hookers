import { useEffect, useState } from "react";
import type { Edge, Node, ReactFlowInstance } from "@xyflow/react";

export function useFitFlow<NodeType extends Node, EdgeType extends Edge>(
  flow: ReactFlowInstance<NodeType, EdgeType> | null,
  fitKey: string,
  maxZoom?: number,
) {
  const [element, setElement] = useState<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!element || !flow) return;
    let frame = 0;
    let pointerActive = false;
    let pendingResize = false;
    const scheduleFit = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        frame = 0;
        void flow.fitView({ padding: 0.12, minZoom: 0.64, maxZoom });
      });
    };
    const onResize = () => {
      if (pointerActive) {
        pendingResize = true;
        return;
      }
      scheduleFit();
    };
    const onPointerDown = () => {
      pointerActive = true;
      if (frame !== 0) {
        cancelAnimationFrame(frame);
        frame = 0;
        pendingResize = true;
      }
    };
    const onPointerEnd = () => {
      pointerActive = false;
      if (!pendingResize) return;
      pendingResize = false;
      scheduleFit();
    };
    scheduleFit();
    const observer = new ResizeObserver(onResize);
    observer.observe(element);
    element.addEventListener("pointerdown", onPointerDown, true);
    window.addEventListener("pointerup", onPointerEnd, true);
    window.addEventListener("pointercancel", onPointerEnd, true);
    return () => {
      observer.disconnect();
      element.removeEventListener("pointerdown", onPointerDown, true);
      window.removeEventListener("pointerup", onPointerEnd, true);
      window.removeEventListener("pointercancel", onPointerEnd, true);
      cancelAnimationFrame(frame);
    };
  }, [element, fitKey, flow, maxZoom]);

  return setElement;
}
