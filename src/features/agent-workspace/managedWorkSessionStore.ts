import { invoke } from "../../invoke";
import { setPendingDirectoryTargets } from "../space-analysis/targetStore";
import type { MonitorSnapshot } from "../space-analysis/types";

export type WorkShell = "powershell" | "gitbash" | "cmd";
export type WorkSessionMode = "cli" | "desktop";
export type WorkSessionDesktopAction = "launch" | "attach";

export interface WorkEnvironmentItem {
  id: string;
  label: string;
  kind: string;
  available: boolean;
  version: string | null;
  path: string | null;
  homeVariable: string | null;
  homePath: string | null;
}

export interface WorkEnvironmentContract {
  generatedAt: string;
  fingerprint: string;
  items: WorkEnvironmentItem[];
}

export interface WorkSessionTrackingRoot {
  id: string;
  label: string;
  path: string;
  category: "project" | "agent-data" | "cache";
  safety: "protected" | "review" | "rebuildable";
  defaultEnabled: boolean;
  reason: string;
}

export interface WorkSessionProfile {
  workspace: string;
  agentId: string;
  mode: WorkSessionMode;
  shell: WorkShell;
  desktopAction: WorkSessionDesktopAction;
  desktopPid: number | null;
  enabledItems: string[];
  trackingRootIds: string[];
  trackingRootsConfigured: boolean;
  contractFingerprint: string;
}

export interface WorkSessionDesktopProcess {
  agentId: string;
  agentName: string;
  desktopName: string;
  pid: number;
  parentPid: number;
  processName: string;
}

export interface WorkSessionLaunchResult {
  sessionId: string;
  agentId: string;
  agentName: string;
  cliName: string;
  commandPath: string;
  workspace: string;
  shell: WorkShell | "desktop";
  startedAt: string;
  environmentFingerprint: string;
  environment: WorkEnvironmentItem[];
  mode: WorkSessionMode;
  targetName: string;
  launchKind: "launched" | "attached" | null;
  process: WorkSessionDesktopProcess | null;
}

export interface WorkSessionReport {
  id: string;
  endedAt: string;
  launch: WorkSessionLaunchResult;
  enabledItems: string[];
  trackingRoots: WorkSessionTrackingRoot[];
  monitor: MonitorSnapshot;
  status: "completed" | "failed" | "interrupted";
}

export type WorkSessionPhase =
  | "idle"
  | "baseline"
  | "launching"
  | "tracking"
  | "stopping"
  | "stopped"
  | "error";

export interface ManagedWorkSessionState {
  contract: WorkEnvironmentContract | null;
  contractLoading: boolean;
  trackingRoots: WorkSessionTrackingRoot[];
  trackingRootsLoading: boolean;
  activeTrackingRoots: WorkSessionTrackingRoot[];
  profile: WorkSessionProfile;
  profileSaved: boolean;
  phase: WorkSessionPhase;
  taskId: string | null;
  launch: WorkSessionLaunchResult | null;
  monitor: MonitorSnapshot | null;
  reports: WorkSessionReport[];
  reportsLoading: boolean;
  recovered: boolean;
  error: string | null;
}

interface ActiveWorkSessionDraft {
  launch: WorkSessionLaunchResult;
  profile: WorkSessionProfile;
  trackingRoots: WorkSessionTrackingRoot[];
  monitor: MonitorSnapshot;
}

const PROFILE_KEY = "stacker.managed-work-session.profile.v3";
const LEGACY_PROFILE_KEYS = ["stacker.managed-work-session.profile.v2", "stacker.managed-work-session.profile.v1"];
const ACTIVE_KEY = "stacker.managed-work-session.active.v1";
const EMPTY_PROFILE: WorkSessionProfile = {
  workspace: "",
  agentId: "",
  mode: "cli",
  shell: "powershell",
  desktopAction: "launch",
  desktopPid: null,
  enabledItems: [],
  trackingRootIds: [],
  trackingRootsConfigured: false,
  contractFingerprint: "",
};

function readStorage(key: string): string | null {
  try { return localStorage.getItem(key); } catch { return null; }
}

function writeStorage(key: string, value: string) {
  try { localStorage.setItem(key, value); } catch { /* Diagnostics must not break the work session. */ }
}

function removeStorage(key: string) {
  try { localStorage.removeItem(key); } catch { /* Ignore unavailable browser storage. */ }
}

const hasStoredProfile = () => readStorage(PROFILE_KEY) !== null || LEGACY_PROFILE_KEYS.some((key) => readStorage(key) !== null);

export function normalizeWorkSessionProfile(parsed: Partial<WorkSessionProfile> | null | undefined): WorkSessionProfile {
  if (!parsed) return { ...EMPTY_PROFILE };
  const shell: WorkShell = parsed.shell === "cmd" || parsed.shell === "gitbash" ? parsed.shell : "powershell";
  return {
    workspace: typeof parsed.workspace === "string" ? parsed.workspace : "",
    agentId: typeof parsed.agentId === "string" ? parsed.agentId : "",
    mode: parsed.mode === "desktop" ? "desktop" : "cli",
    shell,
    desktopAction: parsed.desktopAction === "attach" ? "attach" : "launch",
    desktopPid: typeof parsed.desktopPid === "number" ? parsed.desktopPid : null,
    enabledItems: Array.isArray(parsed.enabledItems) ? parsed.enabledItems.filter((item): item is string => typeof item === "string") : [],
    trackingRootIds: Array.isArray(parsed.trackingRootIds) ? parsed.trackingRootIds.filter((item): item is string => typeof item === "string") : [],
    trackingRootsConfigured: parsed.trackingRootsConfigured === true,
    contractFingerprint: typeof parsed.contractFingerprint === "string" ? parsed.contractFingerprint : "",
  };
}

function readProfile(): WorkSessionProfile {
  try {
    const raw = readStorage(PROFILE_KEY) ?? LEGACY_PROFILE_KEYS.map(readStorage).find((value) => value !== null);
    const parsed = JSON.parse(raw || "null") as Partial<WorkSessionProfile> | null;
    return normalizeWorkSessionProfile(parsed);
  } catch {
    return EMPTY_PROFILE;
  }
}

function readActiveDraft(): ActiveWorkSessionDraft | null {
  try {
    const parsed = JSON.parse(readStorage(ACTIVE_KEY) || "null") as ActiveWorkSessionDraft | null;
    if (!parsed?.launch?.sessionId || !parsed.monitor?.taskId) return null;
    return {
      ...parsed,
      profile: normalizeWorkSessionProfile(parsed.profile),
      launch: {
        ...parsed.launch,
        environment: Array.isArray(parsed.launch.environment) ? parsed.launch.environment : [],
        mode: parsed.launch.mode === "desktop" ? "desktop" : "cli",
        targetName: parsed.launch.targetName || parsed.launch.cliName || parsed.launch.agentName,
        launchKind: parsed.launch.launchKind ?? "launched",
        process: parsed.launch.process ?? null,
      },
    };
  } catch {
    return null;
  }
}

const recoveredDraft = readActiveDraft();
let state: ManagedWorkSessionState = {
  contract: null,
  contractLoading: false,
  trackingRoots: [],
  trackingRootsLoading: false,
  activeTrackingRoots: recoveredDraft?.trackingRoots ?? [],
  profile: recoveredDraft?.profile ?? readProfile(),
  profileSaved: true,
  phase: recoveredDraft ? "stopped" : "idle",
  taskId: null,
  launch: recoveredDraft?.launch ?? null,
  monitor: recoveredDraft?.monitor ?? null,
  reports: [],
  reportsLoading: false,
  recovered: Boolean(recoveredDraft),
  error: null,
};
let profileWasStored = hasStoredProfile();
let contractRun: Promise<WorkEnvironmentContract> | null = null;
let trackingRootsRun: Promise<WorkSessionTrackingRoot[]> | null = null;
let trackingRootsRunKey = "";
let trackingRootsCacheKey = "";
let monitorTimer: ReturnType<typeof setTimeout> | null = null;
let generation = 0;
let reportFinalized = false;
let stopRun: Promise<void> | null = null;
let monitorPollFailures = 0;
const listeners = new Set<(next: ManagedWorkSessionState) => void>();

function publish(patch: Partial<ManagedWorkSessionState>) {
  state = { ...state, ...patch };
  listeners.forEach((listener) => listener(state));
}

function sleep(milliseconds: number) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

function saveActiveDraft() {
  if (!state.launch || !state.monitor || state.phase !== "tracking") return;
  writeStorage(ACTIVE_KEY, JSON.stringify({
    launch: state.launch,
    profile: state.profile,
    trackingRoots: state.activeTrackingRoots,
    monitor: state.monitor,
  } satisfies ActiveWorkSessionDraft));
}

function clearActiveDraft() {
  removeStorage(ACTIVE_KEY);
}

export function managedWorkSessionSnapshot() {
  return state;
}

export function subscribeManagedWorkSession(listener: (next: ManagedWorkSessionState) => void) {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

export function updateManagedWorkSessionProfile(patch: Partial<WorkSessionProfile>) {
  publish({ profile: { ...state.profile, ...patch }, profileSaved: false });
}

export function saveManagedWorkSessionProfile() {
  const profile = {
    ...state.profile,
    contractFingerprint: state.contract?.fingerprint ?? state.profile.contractFingerprint,
  };
  writeStorage(PROFILE_KEY, JSON.stringify(profile));
  LEGACY_PROFILE_KEYS.forEach(removeStorage);
  profileWasStored = true;
  publish({ profile, profileSaved: true });
}

export async function loadWorkEnvironmentContract(force = false) {
  if (state.contract && !force) return state.contract;
  if (contractRun) return contractRun;
  publish({ contractLoading: true, error: null });
  contractRun = invoke<WorkEnvironmentContract>("work_environment_contract")
    .then((contract) => {
      const available = new Set(contract.items.filter((item) => item.available).map((item) => item.id));
      const enabledItems = profileWasStored
        ? state.profile.enabledItems.filter((item) => available.has(item))
        : [...available];
      publish({
        contract,
        profile: { ...state.profile, enabledItems },
        profileSaved: state.profile.contractFingerprint === contract.fingerprint && state.profileSaved,
      });
      return contract;
    })
    .catch((error) => {
      publish({ error: String(error) });
      throw error;
    })
    .finally(() => {
      contractRun = null;
      publish({ contractLoading: false });
    });
  return contractRun;
}

export async function loadWorkSessionTrackingRoots(force = false) {
  if (!state.profile.workspace || !state.profile.agentId) {
    publish({ trackingRoots: [], trackingRootsLoading: false });
    return [];
  }
  const request = {
    agentId: state.profile.agentId,
    workspace: state.profile.workspace,
    enabledItems: [...state.profile.enabledItems].sort(),
  };
  const requestKey = JSON.stringify(request);
  if (state.trackingRoots.length > 0 && trackingRootsCacheKey === requestKey && !force) return state.trackingRoots;
  if (trackingRootsRun) {
    if (trackingRootsRunKey === requestKey) return trackingRootsRun;
    await trackingRootsRun.catch(() => undefined);
    return loadWorkSessionTrackingRoots(force);
  }
  publish({ trackingRootsLoading: true, error: null });
  trackingRootsRunKey = requestKey;
  trackingRootsRun = invoke<WorkSessionTrackingRoot[]>("work_session_tracking_roots", {
    request,
  }).then((roots) => {
    const currentKey = JSON.stringify({
      agentId: state.profile.agentId,
      workspace: state.profile.workspace,
      enabledItems: [...state.profile.enabledItems].sort(),
    });
    if (currentKey !== requestKey) return roots;
    const available = new Set(roots.map((root) => root.id));
    const configured = state.profile.trackingRootsConfigured;
    const selected = configured
      ? state.profile.trackingRootIds.filter((id) => available.has(id))
      : roots.filter((root) => root.defaultEnabled).map((root) => root.id);
    if (!selected.includes("project") && available.has("project")) selected.unshift("project");
    publish({
      trackingRoots: roots,
      profile: { ...state.profile, trackingRootIds: selected },
    });
    trackingRootsCacheKey = requestKey;
    return roots;
  }).catch((error) => {
    publish({ error: String(error) });
    throw error;
  }).finally(() => {
    trackingRootsRun = null;
    trackingRootsRunKey = "";
    publish({ trackingRootsLoading: false });
  });
  return trackingRootsRun;
}

export async function loadWorkSessionReports() {
  publish({ reportsLoading: true });
  try {
    const reports = await invoke<WorkSessionReport[]>("work_session_report_list");
    publish({ reports });
    return reports;
  } finally {
    publish({ reportsLoading: false });
  }
}

export async function loadDesktopSessionProcesses(agentId = state.profile.agentId) {
  if (!agentId) return [];
  return invoke<WorkSessionDesktopProcess[]>("work_session_desktop_candidates", {
    request: { agentId },
  });
}

export async function deleteWorkSessionReport(id: string) {
  await invoke("work_session_report_delete", { id });
  publish({ reports: state.reports.filter((report) => report.id !== id) });
}

export async function recoverInterruptedWorkSession() {
  if (!state.recovered || !state.launch || !state.monitor) return null;
  const recoveredTaskId = state.monitor.taskId;
  const report = await invoke<WorkSessionReport>("work_session_report_save", {
    input: {
      launch: state.launch,
      enabledItems: state.profile.enabledItems,
      trackingRoots: state.activeTrackingRoots,
      monitor: { ...state.monitor, state: "stopped" },
      status: "interrupted",
    },
  });
  await invoke("space_monitor_dispose", { taskId: recoveredTaskId }).catch(() => undefined);
  clearActiveDraft();
  publish({ recovered: false, reports: [report, ...state.reports] });
  return report;
}

async function readMonitor(taskId: string) {
  const monitor = await invoke<MonitorSnapshot>("space_monitor_status", { taskId });
  publish({ monitor });
  saveActiveDraft();
  return monitor;
}

function stopMonitorPolling() {
  if (monitorTimer) clearTimeout(monitorTimer);
  monitorTimer = null;
}

async function finalizeReport(status: WorkSessionReport["status"], monitor = state.monitor) {
  if (reportFinalized || !state.launch || !monitor) return null;
  reportFinalized = true;
  try {
    const report = await invoke<WorkSessionReport>("work_session_report_save", {
      input: {
        launch: state.launch,
        enabledItems: state.profile.enabledItems,
        trackingRoots: state.activeTrackingRoots,
        monitor,
        status,
      },
    });
    clearActiveDraft();
    const taskId = state.taskId;
    publish({ taskId: null, reports: [report, ...state.reports.filter((item) => item.id !== report.id)] });
    if (taskId) await invoke("space_monitor_dispose", { taskId }).catch(() => undefined);
    return report;
  } catch (error) {
    reportFinalized = false;
    throw error;
  }
}

function scheduleMonitorPolling(taskId: string) {
  stopMonitorPolling();
  const poll = async () => {
    if (state.taskId !== taskId) return;
    try {
      const monitor = await readMonitor(taskId);
      monitorPollFailures = 0;
      if (monitor.state === "stopped" || monitor.state === "failed") {
        const failed = monitor.state === "failed";
        publish({ phase: failed ? "error" : "stopped", error: monitor.error });
        await finalizeReport(failed ? "failed" : "completed", monitor);
        return;
      }
      if (state.launch?.mode === "desktop" && state.launch.process?.pid) {
        const status = await invoke<{ pid: number; running: boolean }>("work_session_desktop_process_status", {
          pid: state.launch.process.pid,
        });
        if (!status.running) {
          await stopManagedWorkSession();
          return;
        }
      }
      monitorTimer = setTimeout(() => void poll(), 2500);
    } catch (error) {
      if (state.taskId !== taskId) return;
      monitorPollFailures += 1;
      if (monitorPollFailures >= 3) publish({ error: String(error) });
      monitorTimer = setTimeout(() => void poll(), 2500);
    }
  };
  monitorTimer = setTimeout(() => void poll(), 1000);
}

async function waitForBaseline(taskId: string, runGeneration: number) {
  while (generation === runGeneration && state.taskId === taskId) {
    const monitor = await readMonitor(taskId);
    if (monitor.state === "running") return;
    if (monitor.state === "failed") throw new Error(monitor.error || "Unable to establish the tracking baseline.");
    if (monitor.state === "stopped" || monitor.state === "stopping") throw new Error("Work session start was cancelled.");
    await sleep(700);
  }
  throw new Error("Work session start was cancelled.");
}

export async function startManagedWorkSession() {
  if (["baseline", "launching", "tracking", "stopping"].includes(state.phase)) {
    throw new Error("A managed work session is already active.");
  }
  if (state.profile.mode === "cli") await loadWorkEnvironmentContract();
  if (!state.profile.workspace) throw new Error("Select a project folder first.");
  if (!state.profile.agentId) throw new Error("Select an installed work agent first.");
  const roots = await loadWorkSessionTrackingRoots(true);
  const selected = roots.filter((root) => state.profile.trackingRootIds.includes(root.id));
  if (!selected.some((root) => root.id === "project")) throw new Error("Project tracking is required for a managed work session.");
  const profile = state.profile;
  const runGeneration = ++generation;
  reportFinalized = false;
  monitorPollFailures = 0;
  clearActiveDraft();
  publish({ phase: "baseline", launch: null, monitor: null, activeTrackingRoots: selected, error: null });
  let taskId: string | null = null;
  try {
    taskId = await invoke<string>("space_monitor_start", { roots: selected.map((root) => root.path) });
    publish({ taskId });
    await waitForBaseline(taskId, runGeneration);
    publish({ phase: "launching" });
    const launch = profile.mode === "desktop"
      ? await invoke<WorkSessionLaunchResult>("work_session_desktop_launch", {
          request: {
            agentId: profile.agentId,
            workspace: profile.workspace,
            action: profile.desktopAction,
            pid: profile.desktopAction === "attach" ? profile.desktopPid : null,
          },
        })
      : await invoke<WorkSessionLaunchResult>("work_session_launch", {
          request: {
            agentId: profile.agentId,
            workspace: profile.workspace,
            shell: profile.shell,
            enabledItems: profile.enabledItems,
          },
        });
    publish({ phase: "tracking", launch, error: null });
    saveManagedWorkSessionProfile();
    saveActiveDraft();
    scheduleMonitorPolling(taskId);
    return launch;
  } catch (error) {
    if (taskId) {
      await invoke("space_monitor_stop", { taskId }).catch(() => undefined);
      await invoke("space_monitor_dispose", { taskId }).catch(() => undefined);
    }
    if (generation === runGeneration) publish({ phase: "error", taskId: null, launch: null, monitor: null, error: String(error) });
    throw error;
  }
}

export async function stopManagedWorkSession() {
  if (stopRun) return stopRun;
  stopRun = stopManagedWorkSessionInternal().finally(() => {
    stopRun = null;
  });
  return stopRun;
}

async function stopManagedWorkSessionInternal() {
  generation += 1;
  stopMonitorPolling();
  const taskId = state.taskId;
  if (!taskId) {
    publish({ phase: "stopped" });
    return;
  }
  publish({ phase: "stopping", error: null });
  try {
    await invoke("space_monitor_stop", { taskId });
    for (let attempt = 0; attempt < 40; attempt += 1) {
      const monitor = await readMonitor(taskId);
      if (monitor.state === "stopped" || monitor.state === "failed") {
        const failed = monitor.state === "failed";
        publish({ phase: failed ? "error" : "stopped", error: monitor.error });
        await finalizeReport(failed ? "failed" : "completed", monitor);
        return;
      }
      await sleep(250);
    }
    throw new Error("Tracking did not stop within the expected time. The session evidence remains available for recovery.");
  } catch (error) {
    if (state.taskId === taskId) {
      publish({ phase: "tracking", error: String(error) });
      saveActiveDraft();
      scheduleMonitorPolling(taskId);
    }
    throw error;
  }
}

export function cleanupTargetsForTrackingRoots(roots: readonly WorkSessionTrackingRoot[]) {
  const seen = new Set<string>();
  return roots
    .filter((root) => root.category === "cache" && root.safety !== "protected")
    .map((root) => root.path)
    .filter((path) => {
      const comparable = path.replaceAll("/", "\\").toLocaleLowerCase();
      if (seen.has(comparable)) return false;
      seen.add(comparable);
      return true;
    });
}

export function prepareWorkSessionCleanup(roots: readonly WorkSessionTrackingRoot[] = state.activeTrackingRoots) {
  const targets = cleanupTargetsForTrackingRoots(roots);
  return { targets, stored: setPendingDirectoryTargets(targets).ok };
}

export async function openWorkSessionPath(path: string) {
  await invoke("space_open_directory", { path });
}
