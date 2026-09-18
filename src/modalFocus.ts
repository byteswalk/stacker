import { useEffect, useRef } from "react";

const layers: { element: HTMLElement; priority: number }[] = [];
const selector = "button:not(:disabled), a[href], input:not(:disabled):not([type='hidden']), select:not(:disabled), textarea:not(:disabled), [tabindex]:not([tabindex='-1'])";

function topLayer() {
  return layers.filter(({ element }) => element.isConnected && element.getClientRects().length > 0)
    .reduce<typeof layers[number] | undefined>((top, layer) => !top || layer.priority >= top.priority ? layer : top, undefined);
}

function focusable(element: HTMLElement) {
  return [...element.querySelectorAll<HTMLElement>(selector)].filter((item) =>
    item.tabIndex >= 0 && !item.matches(":disabled") && !item.closest("[inert], [hidden]")
    && item.getClientRects().length > 0 && getComputedStyle(item).visibility !== "hidden");
}

export function useModalFocus(onClose: (() => void) | undefined, priority = 50) {
  const ref = useRef<HTMLDivElement>(null);
  const onCloseRef = useRef(onClose);
  useEffect(() => { onCloseRef.current = onClose; }, [onClose]);
  useEffect(() => {
    const element = ref.current;
    if (!element) return;
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const layer = { element, priority };
    layers.push(layer);
    const focusFirst = () => (focusable(element)[0] ?? element).focus();
    const frame = requestAnimationFrame(() => {
      if (topLayer() === layer) focusFirst();
    });
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || topLayer() !== layer) return;
      if (event.key === "Escape") {
        event.preventDefault();
        onCloseRef.current?.();
      } else if (event.key === "Tab") {
        const items = focusable(element);
        const first = items[0];
        const last = items[items.length - 1];
        const active = document.activeElement;
        if (!first || !element.contains(active) || active === element) {
          event.preventDefault();
          (event.shiftKey ? last ?? element : first ?? element).focus();
        } else if (event.shiftKey && active === first) {
          event.preventDefault();
          last.focus();
        } else if (!event.shiftKey && active === last) {
          event.preventDefault();
          first.focus();
        }
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => {
      cancelAnimationFrame(frame);
      // Passive cleanup can run after React has removed this dialog from the DOM.
      const wasTop = !layers.some((other, index) => other !== layer
        && other.element.isConnected && other.element.getClientRects().length > 0
        && (other.priority > priority || (other.priority === priority && index > layers.indexOf(layer))));
      layers.splice(layers.indexOf(layer), 1);
      document.removeEventListener("keydown", onKeyDown);
      const top = topLayer();
      if (wasTop && previous?.isConnected && (!top || top.element.contains(previous))) previous.focus();
      else if (wasTop && top) (focusable(top.element)[0] ?? top.element).focus();
    };
  }, [priority]);
  return ref;
}
