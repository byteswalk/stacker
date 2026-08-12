import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "../../invoke";
import { useI18n } from "../../i18n";
import { useToast } from "../../ui";

type AgentProcess = {
  agent_id: string;
  agent: string;
  pid: number;
  parent_pid: number;
  process_name: string;
};

type AgentEnvironmentSnapshot = {
  variable_count: number;
  fingerprint: string;
};

type AgentActivitySnapshot = {
  scanned_at: string;
  processes: AgentProcess[];
  note: string;
  environment?: AgentEnvironmentSnapshot;
  environmentChanged?: boolean;
};

type AgentActivityEvent = {
  kind: "started" | "stopped";
  at: string;
  process: AgentProcess;
};

const OBSERVATION_INTERVAL_MS = 10_000;

export function AgentActivity() {
  const { locale } = useI18n();
  const toast = useToast();
  const en = locale === "en-US";
  const [snapshot, setSnapshot] = useState<AgentActivitySnapshot | null>(null);
  const [loading, setLoading] = useState(false);
  const [watching, setWatching] = useState(false);
  const [events, setEvents] = useState<AgentActivityEvent[]>([]);
  const requestInFlight = useRef(false);
  const previousProcesses = useRef<Map<string, AgentProcess> | null>(null);
  const previousEnvironment = useRef<string | null>(null);

  const copy = useMemo(() => en ? {
    notice: "Read-only local observation. Stacker only classifies matching processes and never returns raw command lines, tokens, or file contents.",
    title: "Local activity observation",
    subtitle: "Check which supported work agents are running right now.",
    scan: "Refresh once",
    scanning: "Scanning...",
    start: "Start observation",
    stop: "Stop observation",
    observing: "Observing every 10 seconds",
    notStarted: "No observation has started",
    notStartedDesc: "Click Refresh activity to read the current Windows process list.",
    running: "Running agents",
    process: "processes",
    pid: "PID",
    parent: "Parent PID",
    environment: "Environment variables",
    environmentChanged: "Environment structure changed since the previous refresh.",
    environmentStable: "No environment structure change detected.",
    none: "No supported work agent process is running.",
    activityNote: "Only process names and IDs are displayed; command lines and secrets are never returned.",
    timeline: "Activity timeline",
    started: "Started",
    stopped: "Stopped",
    noEvents: "No agent process start or stop has been observed in this session.",
    limitation: "Disk changes can be tracked from a completed scan on the Disk Cleanup page; file changes are not attributed to a process without reliable system evidence.",
    failed: "Activity scan failed: ",
  } : {
    notice: "仅进行本机只读观察。Stacker 只识别匹配的进程，不返回完整命令行、令牌或文件内容。",
    title: "本机活动观察",
    subtitle: "查看当前正在运行的受支持工作智能体。",
    scan: "刷新一次",
    scanning: "正在扫描...",
    start: "开始观察",
    stop: "停止观察",
    observing: "每 10 秒观察一次",
    notStarted: "尚未开始观察",
    notStartedDesc: "点击“刷新一次”读取当前 Windows 进程列表。",
    running: "运行中的智能体",
    process: "个进程",
    pid: "进程 ID",
    parent: "父进程 ID",
    environment: "环境变量",
    environmentChanged: "与上次刷新相比，环境变量结构发生了变化。",
    environmentStable: "未发现环境变量结构变化。",
    none: "当前未发现受支持的工作智能体进程。",
    activityNote: "仅显示进程名称和 ID，不返回命令行或敏感信息。",
    timeline: "活动时间线",
    started: "已启动",
    stopped: "已结束",
    noEvents: "本次观察尚未发现智能体进程启动或结束。",
    limitation: "磁盘变化可在“磁盘清理”完成扫描后启动实时追踪；缺少可靠系统证据时，不会将文件变化强行归因到某个进程。",
    failed: "活动状态扫描失败：",
  }, [en]);

  async function refresh(recordEvents = true) {
    if (requestInFlight.current) return;
    requestInFlight.current = true;
    setLoading(true);
    try {
      const [activity, environment] = await Promise.all([
        invoke<AgentActivitySnapshot>("vibe_agent_activity"),
        invoke<AgentEnvironmentSnapshot>("vibe_agent_environment"),
      ]);
      const nextProcesses = new Map(activity.processes.map((process) => [`${process.agent}:${process.pid}`, process]));
      if (recordEvents && previousProcesses.current) {
        const changes: AgentActivityEvent[] = [];
        for (const [key, process] of nextProcesses) {
          if (!previousProcesses.current.has(key)) changes.push({ kind: "started", at: activity.scanned_at, process });
        }
        for (const [key, process] of previousProcesses.current) {
          if (!nextProcesses.has(key)) changes.push({ kind: "stopped", at: activity.scanned_at, process });
        }
        if (changes.length > 0) setEvents((current) => [...changes.reverse(), ...current].slice(0, 50));
      }
      const environmentChanged = previousEnvironment.current !== null
        && previousEnvironment.current !== environment.fingerprint;
      previousProcesses.current = nextProcesses;
      previousEnvironment.current = environment.fingerprint;
      setSnapshot({
        ...activity,
        environment,
        environmentChanged,
      });
    } catch (error) {
      toast(copy.failed + String(error), "err");
    } finally {
      requestInFlight.current = false;
      setLoading(false);
    }
  }

  useEffect(() => {
    if (!watching) return;
    let disposed = false;
    let timer: number | undefined;
    const observe = async () => {
      await refresh(true);
      if (!disposed) timer = window.setTimeout(() => void observe(), OBSERVATION_INTERVAL_MS);
    };
    void observe();
    return () => {
      disposed = true;
      if (timer) window.clearTimeout(timer);
    };
    // Observation owns its polling lifecycle and stops when the tab unmounts.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [watching]);

  const grouped = useMemo(() => {
    const groups = new Map<string, AgentProcess[]>();
    for (const process of snapshot?.processes ?? []) {
      groups.set(process.agent, [...(groups.get(process.agent) ?? []), process]);
    }
    return [...groups.entries()].sort(([a], [b]) => a.localeCompare(b));
  }, [snapshot]);

  return (
    <div className="agent-prototype-page agent-activity-real">
      <div className="agent-prototype-notice">
        <i className="ti ti-shield-check" />
        <span>{copy.notice}</span>
      </div>

      <div className={`agent-prototype-hero ${loading ? "trace-card" : ""}`}>
        {loading && <span className="border-runner" aria-hidden="true" />}
        <div className="agent-prototype-hero-icon"><i className={`ti ${loading ? "ti-loader spin" : "ti-activity-heartbeat"}`} /></div>
        <div className="agent-prototype-hero-copy">
          <b>{copy.title}</b>
          <span>{copy.subtitle}</span>
        </div>
        <div className="agent-activity-actions">
          <button className="gh sm" disabled={loading || watching} onClick={() => void refresh(true)}>
            <i className={`ti ${loading && !watching ? "ti-loader spin" : "ti-refresh"}`} /> {copy.scan}
          </button>
          <button className={watching ? "gh sm danger" : "pr sm"} onClick={() => setWatching((value) => !value)}>
            <i className={`ti ${watching ? "ti-player-stop" : "ti-player-play"}`} /> {watching ? copy.stop : copy.start}
          </button>
        </div>
      </div>

      {watching && <div className="agent-observation-status"><i className="ti ti-live-photo" /> {copy.observing}</div>}

      {!snapshot ? (
        <div className="agent-panel agent-activity-empty">
          <i className="ti ti-radar-2" />
          <b>{copy.notStarted}</b>
          <span>{copy.notStartedDesc}</span>
        </div>
      ) : (
        <>
          <div className="agent-activity-summary">
            <span><b>{grouped.length}</b> {copy.running}</span>
            <span><b>{snapshot.processes.length}</b> {copy.process}</span>
            <span><b>{snapshot.environment?.variable_count ?? 0}</b> {copy.environment}</span>
            <small>{new Date(snapshot.scanned_at).toLocaleString()}</small>
          </div>
          {grouped.length === 0 ? (
            <div className="agent-panel agent-activity-empty"><i className="ti ti-circle-check" /><span>{copy.none}</span></div>
          ) : (
            <div className="agent-activity-process-list">
              {grouped.map(([agent, processes]) => (
                <section className="agent-panel agent-process-group" key={agent}>
                  <div className="agent-panel-head">
                    <div><b>{agent}</b><span>{processes.length} {copy.process}</span></div>
                    <span className="bd g">{copy.running}</span>
                  </div>
                  {processes.map((process) => (
                    <div className="agent-process-row" key={`${process.agent}-${process.pid}`}>
                      <i className="ti ti-player-play" />
                      <b>{process.process_name}</b>
                      <span>{copy.pid} {process.pid}</span>
                      <span>{copy.parent} {process.parent_pid}</span>
                    </div>
                  ))}
                </section>
              ))}
            </div>
          )}
          <div className="agent-panel agent-activity-timeline">
            <div className="agent-panel-head"><div><b>{copy.timeline}</b><span>{events.length}</span></div></div>
            {events.length > 0 ? events.map((event) => (
              <div className={`agent-timeline-row ${event.kind}`} key={`${event.kind}:${event.process.agent}:${event.process.pid}:${event.at}`}>
                <i className={`ti ${event.kind === "started" ? "ti-player-play" : "ti-player-stop"}`} />
                <b>{event.process.agent}</b>
                <span>{event.process.process_name} · {copy.pid} {event.process.pid}</span>
                <strong>{event.kind === "started" ? copy.started : copy.stopped}</strong>
                <time>{new Date(event.at).toLocaleTimeString()}</time>
              </div>
            )) : <div className="agent-activity-empty compact"><i className="ti ti-clock" /><span>{copy.noEvents}</span></div>}
          </div>
          <div className="agent-scope-note">
            <i className="ti ti-info-circle" />
            <span>{en ? copy.activityNote : snapshot.note} {copy.limitation} {snapshot.environmentChanged ? copy.environmentChanged : copy.environmentStable}</span>
          </div>
        </>
      )}
    </div>
  );
}
