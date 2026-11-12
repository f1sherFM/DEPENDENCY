import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

type Context = { projectId: string | null; snapshotId: string | null; view: string; selectedNodeId: string | null; busy: boolean };
export function useAgentObservation(context: Context) {
  const latest = useRef(context); latest.current = context;
  const [status, setStatus] = useState<"off" | "active" | "error">("off");
  useEffect(() => {
    let live = true, enabled = false, pending = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const errors: { message: string; at: number }[] = [];
    const record = (message: string) => { if (enabled) { errors.push({ message: message.slice(0, 1000), at: Date.now() }); if (errors.length > 20) errors.shift(); } };
    const onError = (event: ErrorEvent) => record(`${event.message} · ${event.filename}:${event.lineno}:${event.colno}`);
    const onRejection = (event: PromiseRejectionEvent) => record(event.reason instanceof Error ? event.reason.stack ?? event.reason.message : String(event.reason));
    const rect = (element: Element | null) => { if (!element) return null; const { x, y, width, height } = element.getBoundingClientRect(); return { x, y, width, height }; };
    const publish = async () => {
      if (!live || pending) return;
      pending = true;
      try {
        enabled = await invoke<boolean>("agent_access_enabled");
        if (!live) return;
        if (!enabled) { setStatus("off"); return; }
        const main = document.querySelector<HTMLElement>(".main-content");
        const canvas = document.querySelector(".graph-canvas");
        const bounds = canvas?.getBoundingClientRect();
        const mainBounds = main?.getBoundingClientRect();
        const nodes = Array.from(document.querySelectorAll<SVGElement>(".graph-node[data-node-id]"));
        const state = { ...latest.current, viewport: { width: innerWidth, height: innerHeight, devicePixelRatio },
          layout: { header: rect(document.querySelector(".topbar")), main: rect(main), canvas: rect(canvas), graphFooter: rect(document.querySelector(".graph-footer")), rootScrollTop: document.scrollingElement?.scrollTop, rootScrollWidth: document.scrollingElement?.scrollWidth, mainScrollTop: main?.scrollTop, mainScrollHeight: main?.scrollHeight },
          filters: Array.from(main?.querySelectorAll<HTMLInputElement | HTMLSelectElement>("input,select") ?? []).filter(element => element.getClientRects().length).slice(0, 30).map(element => ({ label: element.getAttribute("aria-label") ?? element.closest("label")?.textContent?.slice(0, 100) ?? element.getAttribute("placeholder"), value: element.value, checked: element instanceof HTMLInputElement ? element.checked : undefined })),
          graph: canvas ? { transform: canvas.querySelector("svg > g")?.getAttribute("transform"), renderedNodeIds: nodes.map(node => node.dataset.nodeId), onScreenNodeIds: nodes.filter(node => { const r = node.getBoundingClientRect(); return bounds && mainBounds && r.right > Math.max(0, bounds.left, mainBounds.left) && r.left < Math.min(innerWidth, bounds.right, mainBounds.right) && r.bottom > Math.max(0, bounds.top, mainBounds.top) && r.top < Math.min(innerHeight, bounds.bottom, mainBounds.bottom); }).map(node => node.dataset.nodeId), coverage: document.querySelector(".graph-coverage")?.textContent?.slice(0, 2000) } : null,
          renderedText: main?.innerText.slice(0, 20000), textTruncated: (main?.innerText.length ?? 0) > 20000,
          errorBanner: document.querySelector(".error-banner")?.textContent?.slice(0, 2000) ?? null, runtimeErrors: [...errors] };
        await invoke("publish_agent_state", { state });
        if (live) setStatus("active");
      } catch { if (live) setStatus("error"); }
      finally { pending = false; }
    };
    const schedule = () => { if (enabled) { clearTimeout(timer); timer = setTimeout(() => void publish(), 300); } };
    const observer = new MutationObserver(schedule);
    observer.observe(document.getElementById("root")!, { subtree: true, childList: true, attributes: true, characterData: true });
    window.addEventListener("error", onError); window.addEventListener("unhandledrejection", onRejection);
    window.addEventListener("resize", schedule); document.addEventListener("scroll", schedule, true); document.addEventListener("input", schedule, true);
    const heartbeat = setInterval(() => void publish(), 3000); void publish();
    return () => { live = false; clearTimeout(timer); clearInterval(heartbeat); observer.disconnect(); window.removeEventListener("error", onError); window.removeEventListener("unhandledrejection", onRejection); window.removeEventListener("resize", schedule); document.removeEventListener("scroll", schedule, true); document.removeEventListener("input", schedule, true); };
  }, []);
  return status;
}
