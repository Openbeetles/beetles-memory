import { getCurrentWindow } from "@tauri-apps/api/window";

const interactiveSelector = [
  "a",
  "button",
  "input",
  "select",
  "textarea",
  "[contenteditable='true']",
  "[role='button']",
  "[data-tauri-no-drag]",
].join(",");

function isTauriRuntime(): boolean {
  return typeof window !== "undefined"
    && ("__TAURI_INTERNALS__" in window || "__TAURI__" in window);
}

export function startWindowDrag(event: PointerEvent) {
  if (event.button !== 0 || !isTauriRuntime()) return;

  const target = event.target;
  if (target instanceof Element && target.closest(interactiveSelector)) return;

  event.preventDefault();
  void getCurrentWindow().startDragging().catch((error) => {
    console.error("Failed to start Tauri window drag", error);
  });
}

export function windowDragRegion(node: HTMLElement) {
  node.addEventListener("pointerdown", startWindowDrag);

  return {
    destroy() {
      node.removeEventListener("pointerdown", startWindowDrag);
    },
  };
}
