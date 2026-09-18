import { useCallback, useEffect, useState } from "react";
import { invoke } from "../invoke";
import { VersionManager } from "../VersionManager";
import { StorageLocations } from "../StorageLocations";
import { SourcesPanel } from "../SourcesPanel";
import { ConfirmModal, useBusy, useToast, operationWasCancelled } from "../ui";
import { useNotifications } from "../notifications";

type ComposerStatus = {
  installed: boolean;
  managed: boolean;
  version: string;
  path: string;
  home: string;
};

const PHP_SOURCES = [
  {
    id: "official",
    name: "官方 PHP for Windows",
    url: "https://windows.php.net/downloads/releases",
    host: "windows.php.net",
  },
];

export default function PHP() {
  const toast = useToast();
  const runBusy = useBusy();
  const notices = useNotifications();
  const [composer, setComposer] = useState<ComposerStatus | null>(null);
  const [composerError, setComposerError] = useState("");
  const [removeComposer, setRemoveComposer] = useState(false);
  const [sourceRefresh, setSourceRefresh] = useState(0);

  const loadComposer = useCallback(async () => {
    try {
      setComposer(await invoke<ComposerStatus>("composer_status"));
      setComposerError("");
    } catch (error) {
      setComposerError(String(error));
    }
  }, []);

  useEffect(() => { void loadComposer(); }, [loadComposer]);

  async function installComposer() {
    try {
      await runBusy({
        title: composer?.managed ? "更新 Composer" : "安装 Composer",
        message: "正在下载并验证 Composer。完成后会刷新命令状态和包仓库配置。",
        progressEvent: "install-progress",
        cancel: {
          label: "取消",
          onCancel: () => { invoke("op_cancel").catch(() => undefined); },
        },
      }, async () => {
        await invoke("composer_install");
        await loadComposer();
      });
      setSourceRefresh((value) => value + 1);
      toast(composer?.managed ? "Composer 已更新" : "Composer 已安装，新终端生效", "ok");
      void notices.checkNow("composer-install").catch(() => undefined);
    } catch (error) {
      const cancelled = operationWasCancelled(error);
      toast(cancelled ? "已取消 Composer 操作" : `Composer 操作失败：${error}`, cancelled ? "info" : "err");
    }
  }

  async function clearComposer() {
    try {
      await runBusy({
        title: "卸载 Composer",
        message: "正在删除由 Stacker 安装的 Composer 命令入口，并清理对应的用户环境变量。",
      }, async () => {
        await invoke("composer_clear");
        await loadComposer();
      });
      setRemoveComposer(false);
      setSourceRefresh((value) => value + 1);
      toast("Composer 已卸载", "ok");
    } catch (error) {
      toast(`卸载 Composer 失败：${error}`, "err");
    }
  }

  return (
    <>
      <VersionManager
        kind="php"
        icon="ti-brand-php"
        cmd="php"
        envvar=""
        download={{
          title: "安装 PHP",
          subdir: "php",
          folderName: (version) => `php-${version}-nts-x64`,
          sources: PHP_SOURCES,
          sourceToolId: "php-runtime",
          urlFor: () => "",
          versionsCmd: "php_versions",
          resolveUrlCmd: "php_download_url",
          stripTop: false,
          defaultSource: "official",
          note: "版本列表按当前下载源实际提供的 Windows x64 Non Thread Safe ZIP 生成。",
        }}
        onChanged={() => setSourceRefresh((value) => value + 1)}
      />

      <StorageLocations ecosystem="php" />

      <div className="grouphd" style={{ marginTop: 18 }}>
        <span className="gt"><i className="ti ti-package" /> Composer <span className="cnt">PHP 包管理器</span></span>
      </div>
      <div className="srcrow">
        <span className="av"><i className="ti ti-package" /></span>
        <div className="mt">
          <div className="t">
            Composer
            {composer?.installed && <span className="bd g">已安装</span>}
            {composer?.installed && !composer.managed && <span className="bd n">外部安装</span>}
          </div>
          <div className="s dim" title={composer?.path || composerError || "未检测到 Composer"}>
            {composerError
              ? `读取失败：${composerError}`
              : composer?.installed
                ? `${composer.version} · ${composer.path}`
                : "未检测到 Composer；安装前需要先配置可用的 PHP 默认版本。"}
          </div>
        </div>
        {!composer?.installed && <button className="pr sm" onClick={() => void installComposer()}><i className="ti ti-download" /> 安装</button>}
        {composer?.managed && <button className="pr sm" onClick={() => void installComposer()}><i className="ti ti-refresh" /> 更新</button>}
        {composer?.managed && <button className="gh xs danger" title="卸载由 Stacker 安装的 Composer" onClick={() => setRemoveComposer(true)}><i className="ti ti-trash" /></button>}
      </div>

      <SourcesPanel toolIds={["composer"]} refresh={sourceRefresh} />

      {removeComposer && <ConfirmModal
        title="卸载 Composer"
        icon="ti-trash"
        danger
        message="将删除由 Stacker 安装的 Composer 命令入口和 PHAR 文件，并移除对应的用户 PATH。Composer 缓存和项目文件不会删除。"
        confirmLabel="确认卸载"
        onClose={() => setRemoveComposer(false)}
        onConfirm={() => void clearComposer()}
      />}
    </>
  );
}
