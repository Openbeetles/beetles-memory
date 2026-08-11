export type AccessibleDialogOptions = {
  onClose: () => void;
};

type InertEntry = { count: number; wasInert: boolean };
const inertEntries = new WeakMap<HTMLElement, InertEntry>();

const FOCUSABLE_SELECTOR = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  '[tabindex]:not([tabindex="-1"])',
].join(",");

function focusableElements(node: HTMLElement): HTMLElement[] {
  return Array.from(node.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)).filter(
    (element) => element.getAttribute("aria-hidden") !== "true",
  );
}

function isolateDialog(node: HTMLElement): HTMLElement[] {
  const isolated: HTMLElement[] = [];
  let current: HTMLElement | null = node;
  while (current?.parentElement) {
    const parent: HTMLElement = current.parentElement;
    for (const sibling of Array.from(parent.children)) {
      if (!(sibling instanceof HTMLElement) || sibling === current) continue;
      const entry = inertEntries.get(sibling);
      if (entry) {
        entry.count += 1;
      } else {
        inertEntries.set(sibling, { count: 1, wasInert: sibling.inert });
        sibling.inert = true;
      }
      isolated.push(sibling);
    }
    current = parent;
    if (parent === document.body) break;
  }
  return isolated;
}

function restoreIsolation(elements: HTMLElement[]) {
  for (const element of elements) {
    const entry = inertEntries.get(element);
    if (!entry) continue;
    entry.count -= 1;
    if (entry.count === 0) {
      element.inert = entry.wasInert;
      inertEntries.delete(element);
    }
  }
}

export function accessibleDialog(node: HTMLElement, options: AccessibleDialogOptions) {
  let currentOptions = options;
  const previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  const isolatedElements = isolateDialog(node);

  function focusInitial() {
    const target = node.querySelector<HTMLElement>("[data-dialog-autofocus]") ?? focusableElements(node)[0] ?? node;
    target.focus({ preventScroll: true });
  }

  function handleKeydown(event: KeyboardEvent) {
    const dialogs = document.querySelectorAll<HTMLElement>('[role="dialog"][aria-modal="true"]');
    if (dialogs[dialogs.length - 1] !== node) return;
    if (event.key === "Escape") {
      event.preventDefault();
      currentOptions.onClose();
      return;
    }
    if (event.key !== "Tab") return;

    const focusable = focusableElements(node);
    if (focusable.length === 0) {
      event.preventDefault();
      node.focus({ preventScroll: true });
      return;
    }

    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    const active = document.activeElement;
    if (event.shiftKey && (active === first || !node.contains(active))) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && (active === last || !node.contains(active))) {
      event.preventDefault();
      first.focus();
    }
  }

  document.addEventListener("keydown", handleKeydown);
  queueMicrotask(focusInitial);

  return {
    update(nextOptions: AccessibleDialogOptions) {
      currentOptions = nextOptions;
    },
    destroy() {
      document.removeEventListener("keydown", handleKeydown);
      restoreIsolation(isolatedElements);
      queueMicrotask(() => {
        if (!document.querySelector('[role="dialog"][aria-modal="true"]') && previousFocus?.isConnected) {
          previousFocus.focus({ preventScroll: true });
        }
      });
    },
  };
}
