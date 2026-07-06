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
          sources: [
            { id: "official", name: "官方 go.dev", host: "go.dev", urlFor: (v) => `https://go.dev/dl/go${v}.windows-amd64.zip` },
            { id: "aliyun", name: "阿里云镜像", host: "mirrors.aliyun.com", urlFor: (v) => `https://mirrors.aliyun.com/golang/go${v}.windows-amd64.zip` },
          ],
          defaultSource: "aliyun",
          note: "下载解压。国内选阿里云、慢可换官方 go.dev。",
          staticVersions: ["1.23.4", "1.22.10", "1.21.13"],
        }} />

      <div className="grouphd" style={{ marginTop: 18 }}>
        <span className="gt"><i className="ti ti-world-download" /> 包源 / 镜像 <span className="cnt">go env</span></span>
        <span className="hint2">配置 GOPROXY；系统级写入需要 UAC 提权</span>
      </div>
      <SourcesPanel toolIds={["go"]} refresh={srcKey} />

      <div className="callout"><i className="ti ti-info-circle" /><div>GOSUMDB / GOPRIVATE、GOPATH 模块缓存迁移随后端一并接入；私有模块鉴权可在「设置 → 源管理」新建自定义源。</div></div>
    </>
  );
}
