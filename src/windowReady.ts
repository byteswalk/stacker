import { invoke } from "./invoke";

let announced = false;

/** The first screen is drawn: the main window, hidden until now, can appear. */
export function markWindowReady(): void {
  if (announced) return;
  announced = true;
  invoke("main_window_ready").catch(() => undefined);
}
