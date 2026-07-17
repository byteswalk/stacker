import { useState } from "react";
import { SourcesPanel } from "../SourcesPanel";
import { VersionManager } from "../VersionManager";

export default function Go() {
  const [srcKey, setSrcKey] = useState(0);
  return (
    <>
      <VersionManager kind="go" icon="ti-brand-golang" cmd="go" envvar="GOROOT" onChanged={() => setSrcKey((k) => k + 1)}
        download={{
          title: "下载 Go",
          subdir: "go",
          folderName: (v) => `go${v}`,
          sourceToolId: "go-runtime",
          sources: [
            { id: "official", name: "官方 go.dev", host: "go.dev", url: "https://go.dev/dl" },
            { id: "aliyun", name: "阿里云镜像", host: "mirrors.aliyun.com", url: "https://mirrors.aliyun.com/golang" },
          ],
          urlFor: (source, v) => `${source.url.replace(/\/$/, "")}/go${v}.windows-amd64.zip`,
          defaultSource: "official",
          note: "版本列表按当前下载源实际提供的 Windows 64 位发行包生成。",
          versionsCmd: "go_versions",
        }} />

      <div className="grouphd" style={{ marginTop: 18 }}>
        <span className="gt"><i className="ti ti-world-download" /> 模块代理 <span className="cnt">GOPROXY</span></span>
        <span className="hint2">配置 GOPROXY；系统级写入需要 UAC 提权</span>
      </div>
      <SourcesPanel toolIds={["go"]} refresh={srcKey} />

      <div className="callout"><i className="ti ti-info-circle" /><div>本页管理 Go 版本与 <span className="code">GOPROXY</span>。私有模块使用的 <span className="code">GOPRIVATE</span>、<span className="code">GONOSUMDB</span> 等变量仍由项目或组织策略维护。</div></div>
    </>
  );
}
