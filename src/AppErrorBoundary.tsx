import { Component, type ErrorInfo, type ReactNode } from "react";
import { invoke, reportFrontendError, safeLogText } from "./invoke";
import { useI18n } from "./i18n";

type BoundaryCopy = {
  title: string;
  description: string;
  diagnosticLabel: string;
  openLogs: string;
  reload: string;
  openLogsFailed: string;
};

type BoundaryProps = {
  children: ReactNode;
  copy: BoundaryCopy;
};

type BoundaryState = {
  crashed: boolean;
  diagnosticId: string;
  openingLogs: boolean;
  openLogsFailed: boolean;
};

function createDiagnosticId(): string {
  const timestamp = new Date().toISOString().replace(/[-:.TZ]/g, "").slice(0, 14);
  return `UI-${timestamp}`;
}

class Boundary extends Component<BoundaryProps, BoundaryState> {
  state: BoundaryState = {
    crashed: false,
    diagnosticId: "",
    openingLogs: false,
    openLogsFailed: false,
  };

  static getDerivedStateFromError(): Partial<BoundaryState> {
    return { crashed: true };
  }

  componentDidCatch(error: unknown, info: ErrorInfo): void {
    const diagnosticId = createDiagnosticId();
    reportFrontendError(
      `[${diagnosticId}] Unhandled interface error: ${safeLogText(error)}; component_stack=${safeLogText(info.componentStack)}`,
    );
    this.setState({ diagnosticId });
  }

  private openLogs = async () => {
    this.setState({ openingLogs: true, openLogsFailed: false });
    try {
      await invoke("settings_open_logs_dir");
    } catch {
      this.setState({ openLogsFailed: true });
    } finally {
      this.setState({ openingLogs: false });
    }
  };

  render() {
    if (!this.state.crashed) return this.props.children;
    const { copy } = this.props;
    return (
      <div className="a app-crash-shell" role="alert">
        <section className="app-crash-card">
          <div className="app-crash-icon" aria-hidden="true"><i className="ti ti-alert-triangle" /></div>
          <div className="app-crash-copy">
            <h1>{copy.title}</h1>
            <p>{copy.description}</p>
            {this.state.diagnosticId && (
              <code>{copy.diagnosticLabel}{this.state.diagnosticId}</code>
            )}
            {this.state.openLogsFailed && <p className="app-crash-error">{copy.openLogsFailed}</p>}
          </div>
          <div className="app-crash-actions">
            <button className="gh" disabled={this.state.openingLogs} onClick={this.openLogs}>
              <i className={"ti " + (this.state.openingLogs ? "ti-loader-2 spin" : "ti-folder-open")} />
              {copy.openLogs}
            </button>
            <button className="pr" onClick={() => window.location.reload()}>
              <i className="ti ti-refresh" /> {copy.reload}
            </button>
          </div>
        </section>
      </div>
    );
  }
}

export default function AppErrorBoundary({ children }: { children: ReactNode }) {
  const { tr } = useI18n();
  return (
    <Boundary copy={{
      title: tr("界面暂时无法显示"),
      description: tr("Stacker 遇到界面错误。你的配置和后台任务不会因此被删除。"),
      diagnosticLabel: tr("诊断编号："),
      openLogs: tr("打开日志目录"),
      reload: tr("重新加载界面"),
      openLogsFailed: tr("无法打开日志目录。请重新加载界面后，从“设置”中打开日志目录。"),
    }}>
      {children}
    </Boundary>
  );
}
