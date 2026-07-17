import { useState } from "react";
import { SourcesPanel } from "../SourcesPanel";
import { VersionManager } from "../VersionManager";

const mavenTrack = (v: string) => v.startsWith("4.") ? "maven-4" : v.startsWith("2.") ? "maven-2" : "maven-3";
const mavenZip = (base: string, v: string) => `${base}/${mavenTrack(v)}/${v}/binaries/apache-maven-${v}-bin.zip`;

export default function Maven() {
  const [srcKey, setSrcKey] = useState(0);
  return (
    <>
      <VersionManager kind="maven" icon="ti-feather" cmd="mvn" envvar="MAVEN_HOME" onChanged={() => setSrcKey((k) => k + 1)}
        download={{
          title: "下载 Maven",
          subdir: "maven",
          folderName: (v) => `apache-maven-${v}`,
          defaultSource: "official",
          sourceToolId: "maven-runtime",
          sources: [
            { id: "official", name: "官方 Apache", host: "archive.apache.org", url: "https://archive.apache.org/dist/maven" },
            { id: "tuna", name: "清华大学", host: "mirrors.tuna.tsinghua.edu.cn", url: "https://mirrors.tuna.tsinghua.edu.cn/apache/maven" },
            { id: "ustc", name: "中科大", host: "mirrors.ustc.edu.cn", url: "https://mirrors.ustc.edu.cn/apache/maven" },
            { id: "aliyun", name: "阿里云", host: "mirrors.aliyun.com", url: "https://mirrors.aliyun.com/apache/maven" },
            { id: "huawei", name: "华为云", host: "repo.huaweicloud.com", url: "https://repo.huaweicloud.com/apache/maven" },
            { id: "tencent", name: "腾讯云", host: "mirrors.cloud.tencent.com", url: "https://mirrors.cloud.tencent.com/apache/maven" },
          ],
          urlFor: (source, v) => mavenZip(source.url.replace(/\/$/, ""), v),
          note: "版本列表按当前下载源实际存在的发行包生成。",
          versionsCmd: "maven_versions",
          staticVersions: ["3.9.9", "3.8.8", "3.6.3"],
        }} />

      <div className="grouphd" style={{ marginTop: 18 }}>
        <span className="gt"><i className="ti ti-world-download" /> 仓库镜像 <span className="cnt">settings.xml &lt;mirrors&gt;</span></span>
        <span className="hint2">配置当前用户 settings.xml；也可单独处理指定的 settings.xml</span>
      </div>
      <SourcesPanel toolIds={["maven"]} refresh={srcKey} />

      <div className="callout"><i className="ti ti-info-circle" /><div>仓库镜像只影响依赖解析，不会移动本地仓库。需要接入私有仓库时，可在「设置 → 源管理」添加自定义源。</div></div>
    </>
  );
}
