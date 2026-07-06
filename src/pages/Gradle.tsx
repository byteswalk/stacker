import { useState } from "react";
import { SourcesPanel } from "../SourcesPanel";
import { VersionManager } from "../VersionManager";

export default function Gradle() {
  const [srcKey, setSrcKey] = useState(0);
  return (
    <>
      <VersionManager kind="gradle" icon="ti-box" cmd="gradle" envvar="GRADLE_HOME" onChanged={() => setSrcKey((k) => k + 1)}
        download={{
          title: "下载 Gradle",
          subdir: "gradle",
          folderName: (v) => `gradle-${v}`,
          defaultSource: "tencent",
          sources: [
            { id: "official", name: "官方 Gradle", host: "services.gradle.org", urlFor: (v) => `https://services.gradle.org/distributions/gradle-${v}-bin.zip` },
            { id: "tencent", name: "腾讯云", host: "mirrors.cloud.tencent.com", urlFor: (v) => `https://mirrors.cloud.tencent.com/gradle/gradle-${v}-bin.zip` },
            { id: "huawei", name: "华为云", host: "repo.huaweicloud.com", urlFor: (v) => `https://repo.huaweicloud.com/gradle/gradle-${v}-bin.zip` },
          ],
          note: "版本列表按当前下载源实际存在的发行包生成。",
          versionsCmd: "gradle_versions",
          staticVersions: ["8.12", "8.10", "7.6.4"],
        }} />

      <div className="grouphd" style={{ marginTop: 18 }}>
        <span className="gt"><i className="ti ti-world-download" /> 仓库镜像 <span className="cnt">init.gradle</span></span>
        <span className="hint2">可配置仓库镜像与代理；支持当前用户配置和自选文件</span>
      </div>
      <SourcesPanel toolIds={["gradle"]} refresh={srcKey} />

      <div className="callout"><i className="ti ti-info-circle" /><div>全局镜像可能被项目 <span className="code">build.gradle</span> 的 repositories 覆盖；私服鉴权可在「设置 → 源管理」新建自定义源。</div></div>
    </>
  );
}
