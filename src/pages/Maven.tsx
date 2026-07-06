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
          defaultSource: "tuna",
          sources: [
            { id: "apache", name: "官方 Apache", host: "archive.apache.org", urlFor: (v) => mavenZip("https://archive.apache.org/dist/maven", v) },
            { id: "tuna", name: "清华大学", host: "mirrors.tuna.tsinghua.edu.cn", urlFor: (v) => mavenZip("https://mirrors.tuna.tsinghua.edu.cn/apache/maven", v) },
            { id: "ustc", name: "中科大", host: "mirrors.ustc.edu.cn", urlFor: (v) => mavenZip("https://mirrors.ustc.edu.cn/apache/maven", v) },
            { id: "aliyun", name: "阿里云", host: "mirrors.aliyun.com", urlFor: (v) => mavenZip("https://mirrors.aliyun.com/apache/maven", v) },
            { id: "huawei", name: "华为云", host: "repo.huaweicloud.com", urlFor: (v) => mavenZip("https://repo.huaweicloud.com/apache/maven", v) },
            { id: "tencent", name: "腾讯云", host: "mirrors.cloud.tencent.com", urlFor: (v) => mavenZip("https://mirrors.cloud.tencent.com/apache/maven", v) },
          ],
          note: "版本列表按当前下载源实际存在的发行包生成。",
          versionsCmd: "maven_versions",
          staticVersions: ["3.9.9", "3.8.8", "3.6.3"],
        }} />

      <div className="grouphd" style={{ marginTop: 18 }}>
        <span className="gt"><i className="ti ti-world-download" /> 仓库镜像 <span className="cnt">settings.xml &lt;mirrors&gt;</span></span>
        <span className="hint2">可配置仓库镜像与代理；支持当前用户 settings.xml 和自选文件</span>
      </div>
      <SourcesPanel toolIds={["maven"]} refresh={srcKey} />

      <div className="callout"><i className="ti ti-info-circle" /><div>本地仓库 <span className="code">localRepository</span> 迁移随后端一并接入；私服鉴权可在「设置 → 源管理」新建自定义源。</div></div>
    </>
  );
}
