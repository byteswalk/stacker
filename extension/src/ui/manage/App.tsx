import {
  DeleteOutlined, DownOutlined, EditOutlined, ExportOutlined, FolderOpenOutlined, PlusOutlined,
  ReloadOutlined, SettingOutlined, StarOutlined, TagOutlined,
} from "@ant-design/icons";
import {
  App as AntApp, Button, Divider, Drawer, Dropdown, Empty, Flex, Layout, Menu, Modal, Popover, Segmented,
  Select, Space, Spin, Tag, Tooltip, Typography,
} from "antd";
import { useCallback, useEffect, useMemo, useState } from "react";
import { t } from "../../i18n";
import {
  accountDisplayName, addTag, bodyIsFresh, createFolder, deleteFolder, getBody, listAccounts, listConversations,
  listFolders, openDb, putBody, renameAccount, renameFolder, updateLocal, type Account, type Conversation, type Db,
  type Folder,
} from "../../lib/db";
import { bridgeStatus, callStacker, type BridgeStatus } from "../../lib/bridgeMessages";
import { brokenSitesIn, createBrokenSites, type BrokenSites } from "../../lib/brokenSites";
import { runDeleteJob } from "../../lib/deleteJob";
import { exportFileName, toMarkdown, type ExportMode } from "../../lib/markdown";
import { createPacer, withPacing } from "../../lib/pacer";
import { refreshIndex } from "../../lib/refresh";
import { restoreFromStacker } from "../../lib/restore";
import { pickSaver } from "../../lib/save";
import { allTags, applyFilter, bodyText, EMPTY_FILTER, type Filter } from "../../lib/search";
import { createSiteApi } from "../../lib/siteClient";
import { SiteError, type SiteId } from "../../shared/types";
import { conversationUrl, SITES } from "../../sites/registry";
import { askText, Brand } from "../bits";
import { errorText } from "../errors";
import { usePrefs } from "../Shell";
import { ConversationList } from "./ConversationList";
import { DeleteDialog } from "./DeleteDialog";
import { Detail } from "./Detail";
import { Filters } from "./Filters";
import { deleteBlockReason, siteName } from "./siteStatus";
import { SyncStatus } from "./SyncStatus";

const api = createSiteApi();
const brokenStore = createBrokenSites();
const urlOf = (c: Conversation) => conversationUrl(c.site, c.id);
const SITE_IDS = Object.keys(SITES) as SiteId[];

export function App() {
  const { message, modal } = AntApp.useApp();
  const { prefs, update: setPrefs } = usePrefs();
  const [db, setDb] = useState<Db | null>(null);
  const [convs, setConvs] = useState<Conversation[]>([]);
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [folders, setFolders] = useState<Folder[]>([]);
  const [bodies, setBodies] = useState(new Map<string, string>());
  const [filter, setFilter] = useState<Filter>(EMPTY_FILTER);
  const [selected, setSelected] = useState<string[]>([]);
  const [active, setActive] = useState<Conversation | null>(null);
  const [busy, setBusy] = useState("");
  const [deleting, setDeleting] = useState<Conversation[] | null>(null);
  const [current, setCurrent] = useState(new Set<string>());
  const [siteErrors, setSiteErrors] = useState<Partial<Record<SiteId, string>>>({});
  const [broken, setBroken] = useState<BrokenSites>({});
  const [bridge, setBridge] = useState<BridgeStatus | null>(null);
  // Below this the details would squeeze the list, so they move into a drawer over it.
  const [wide, setWide] = useState(() => window.innerWidth >= 1180);

  const reload = useCallback(async (d: Db) => {
    const [c, a, f] = await Promise.all([listConversations(d), listAccounts(d), listFolders(d)]);
    setConvs(c); setAccounts(a); setFolders(f);
    setActive((old) => (old ? c.find((x) => x.key === old.key) ?? null : null));
  }, []);
  useEffect(() => { void openDb().then(async (d) => { setDb(d); await reload(d); }); }, [reload]);
  useEffect(() => { void brokenStore.all().then(setBroken); }, []);
  useEffect(() => {
    const measure = () => setWide(window.innerWidth >= 1180);
    window.addEventListener("resize", measure);
    return () => window.removeEventListener("resize", measure);
  }, []);

  // Poll the connection; connecting is cheap when Stacker is registered and skipped for 30 s after a failure.
  useEffect(() => {
    let alive = true;
    const poll = () => void bridgeStatus(true).then((s) => { if (alive) setBridge(s); }, () => { if (alive) setBridge(null); });
    poll();
    const timer = setInterval(poll, 5000);
    return () => { alive = false; clearInterval(timer); };
  }, []);
  const connectedNow = async () => (await bridgeStatus(true).catch(() => null))?.connected ?? false;

  useEffect(() => {
    if (!db || !filter.inBody) return;
    void (async () => {
      const map = new Map<string, string>();
      for (const c of convs) if (c.bodyFetchedAt !== null) { const b = await getBody(db, c.key); if (b) map.set(c.key, bodyText(b)); }
      setBodies(map);
    })();
  }, [db, convs, filter.inBody]);

  const shown = useMemo(() => applyFilter(convs, filter, bodies), [convs, filter, bodies]);
  const aliasOf = useCallback((key: string) => {
    const a = accounts.find((x) => x.key === key);
    return a ? accountDisplayName(a) : key;
  }, [accounts]);
  const chosen = convs.filter((c) => selected.includes(c.key));
  const tags = allTags(convs);

  async function guarded(label: string, task: () => Promise<void>) {
    setBusy(label);
    try { await task(); } catch (e) { message.error(errorText(e)); } finally { setBusy(""); if (db) await reload(db); }
  }

  const refreshOne = async (site: SiteId, note: (text: string) => void) => {
    try {
      const r = await refreshIndex(api, db!, site, createPacer(), (n) => note(`${t("正在刷新列表")} ${n}`));
      setBroken(await brokenStore.clear(site));
      setCurrent((old) => new Set([...old, r.account.key]));
      return `${SITES[site].label}：${t("共")} ${r.total}，${t("新增")} ${r.added}，${t("已删除")} ${r.removed}`;
    } catch (e) {
      if (e instanceof SiteError && e.code === "E_BROKEN") setBroken(await brokenStore.mark([site], Date.now()));
      throw e;
    }
  };

  const refresh = (site: SiteId) => guarded(t("正在刷新列表"), async () => {
    message.success(await refreshOne(site, setBusy));
  });

  /** Every site in turn; a site that is not open or not signed in is reported, not fatal. */
  const refreshAll = () => guarded(t("正在刷新列表"), async () => {
    const lines: string[] = [];
    let failed = 0;
    for (const site of SITE_IDS) {
      setBusy(`${t("正在刷新列表")} · ${SITES[site].label}`);
      try {
        lines.push(await refreshOne(site, (text) => setBusy(`${text} · ${SITES[site].label}`)));
      } catch (e) {
        failed++;
        lines.push(`${SITES[site].label}：${errorText(e)}`);
      }
    }
    const show = failed ? message.warning : message.success;
    show(<div>{lines.map((line) => <div key={line}>{line}</div>)}</div>, 6);
  });

  const readBody = (c: Conversation) => guarded(t("正在读取正文"), async () => {
    await putBody(db!, c.key, await api.read(c.site, c.id), Date.now());
  });

  const exportChosen = (mode: ExportMode) => guarded(t("正在导出"), async () => {
    const pacer = createPacer();
    const { save, where } = await pickSaver(connectedNow);
    let n = 0;
    for (const c of chosen) {
      setBusy(`${t("正在导出")} ${++n} / ${chosen.length}`);
      let body = bodyIsFresh(c) ? await getBody(db!, c.key) : undefined;
      if (!body) {
        const fresh = await withPacing(pacer, () => api.read(c.site, c.id));
        await putBody(db!, c.key, fresh, Date.now());
        body = { ...fresh, key: c.key };
      }
      await save(exportFileName(c, "md"), toMarkdown(c, aliasOf(c.account), body, mode, urlOf(c)), "text/markdown");
      if (mode === "full") await save(exportFileName(c, "json"), JSON.stringify(body, null, 2), "application/json");
    }
    message.success(where === "stacker"
      ? `${t("已导出")} ${chosen.length} ${t("条到 Stacker 的导出目录")}`
      : `${t("已导出")} ${chosen.length} ${t("条到下载目录的「Stacker 网页对话」文件夹")}`);
  });

  const restore = () => {
    if (!db) return;
    modal.confirm({
      title: t("从 Stacker 恢复"),
      content: t("从 Stacker 恢复账号备注名、文件夹、标签、收藏、备注和摘录？这台浏览器里较新的修改会保留。"),
      okText: t("确定"),
      cancelText: t("取消"),
      onOk: () => guarded(t("正在从 Stacker 恢复"), async () => {
        const n = await restoreFromStacker(db, (request) => callStacker("pullBackup", request));
        message.success(`${t("已恢复")}：${t("账号")} ${n.accounts}，${t("文件夹")} ${n.folders}，${t("对话")} ${n.conversations}，${t("摘录")} ${n.excerpts}`);
      }),
    });
  };

  async function openDelete() {
    const sites = [...new Set(chosen.map((c) => c.site))];
    const signedIn = new Set<string>();
    const errors: Partial<Record<SiteId, string>> = {};
    const stillBroken = await brokenStore.all();
    setBroken(stillBroken);
    for (const site of sites) {
      const blocked = deleteBlockReason(site, stillBroken);
      if (blocked) { errors[site] = blocked; continue; }
      try { const a = await api.account(site); signedIn.add(`${site}:${a.remoteId}`); } catch (e) { errors[site] = errorText(e); }
    }
    setCurrent(signedIn);
    setSiteErrors(errors);
    setDeleting(chosen);
  }

  if (!db) return <Flex align="center" justify="center" style={{ height: "100%" }}><Spin tip={t("正在打开…")} size="large" /></Flex>;

  const siteAccounts = accounts.filter((a) => !filter.site || a.site === filter.site);
  const unverified = SITE_IDS.filter((s) => !SITES[s].verified);
  const brokenNow = SITE_IDS.filter((s) => broken[s]);
  const folderKey = filter.folder || "all";

  const newFolder = async () => {
    const name = await askText(modal, { title: t("新建文件夹"), placeholder: t("文件夹名称") });
    if (name) await createFolder(db, name, Date.now()).then(() => reload(db));
  };

  const folderItems = [
    { key: "all", icon: <FolderOpenOutlined />, label: t("全部") },
    { key: "none", icon: <FolderOpenOutlined />, label: t("未归入") },
    ...folders.map((f) => ({
      key: f.id,
      icon: <FolderOpenOutlined />,
      label: <span className="folder-row">
        <span className="grow">{f.name}</span>
        <span className="tools">
          <Button
            size="small" type="text" aria-label={t("重命名")} icon={<EditOutlined />}
            onClick={async (e) => {
              e.stopPropagation();
              const name = await askText(modal, { title: t("重命名"), placeholder: t("文件夹名称"), value: f.name });
              if (name) await renameFolder(db, f.id, name).then(() => reload(db));
            }}
          />
          <Button
            size="small" type="text" danger aria-label={t("删除文件夹")} icon={<DeleteOutlined />}
            onClick={(e) => {
              e.stopPropagation();
              modal.confirm({
                title: t("删除文件夹"),
                content: t("删除文件夹？其中的对话不会被删除。"),
                okText: t("删除"), okButtonProps: { danger: true }, cancelText: t("取消"),
                onOk: () => deleteFolder(db, f.id).then(() => reload(db)),
              });
            }}
          />
        </span>
      </span>,
    })),
  ];

  const settings = <Flex vertical gap={12} style={{ width: 214 }}>
    <div>
      <Typography.Text type="secondary">{t("外观")}</Typography.Text>
      <Segmented
        block size="small" style={{ marginTop: 4 }} value={prefs.mode}
        onChange={(mode) => setPrefs({ mode: mode as typeof prefs.mode })}
        options={[{ value: "auto", label: t("跟随系统") }, { value: "dark", label: t("深色") }, { value: "light", label: t("浅色") }]}
      />
    </div>
    <div>
      <Typography.Text type="secondary">{t("语言")}</Typography.Text>
      <Segmented
        block size="small" style={{ marginTop: 4 }} value={prefs.lang}
        onChange={(lang) => setPrefs({ lang: lang as typeof prefs.lang })}
        options={[{ value: "auto", label: t("跟随浏览器") }, { value: "zh", label: "中文" }, { value: "en", label: "English" }]}
      />
    </div>
  </Flex>;

  const toolbar = <Flex align="center" gap={8} wrap className="bulkbar">
    <Typography.Text strong>{t("已选")} {selected.length} {t("条")}</Typography.Text>
    <Dropdown menu={{
      items: [{ key: "none", label: t("未归入") }, ...folders.map((f) => ({ key: f.id, label: f.name }))],
      onClick: ({ key }) => void updateLocal(db, selected, { folderId: key === "none" ? null : key }).then(() => reload(db)),
    }}>
      <Button size="small" icon={<FolderOpenOutlined />}>{t("移到文件夹…")} <DownOutlined /></Button>
    </Dropdown>
    <Button size="small" icon={<TagOutlined />} onClick={async () => {
      const tag = await askText(modal, { title: t("加标签"), placeholder: t("标签") });
      if (tag) await addTag(db, selected, tag).then(() => reload(db));
    }}>{t("加标签")}</Button>
    <Button size="small" icon={<StarOutlined />} onClick={() => void updateLocal(db, selected, { favorite: true }).then(() => reload(db))}>{t("收藏")}</Button>
    <Dropdown menu={{
      items: [{ key: "slim", label: t("导出精简版") }, { key: "full", label: t("导出完整版") }],
      onClick: ({ key }) => void exportChosen(key as ExportMode),
    }}>
      <Button size="small" icon={<ExportOutlined />} disabled={!!busy}>{t("导出")} <DownOutlined /></Button>
    </Dropdown>
    <Button size="small" danger icon={<DeleteOutlined />} disabled={!!busy} onClick={() => void openDelete()}>{t("删除…")}</Button>
    <Button size="small" type="text" onClick={() => setSelected([])}>{t("取消选择")}</Button>
  </Flex>;

  const detail = active
    ? <Detail db={db} conv={active} folders={folders} reading={!!busy} onRead={(c) => void readBody(c)} onChanged={() => void reload(db)} />
    : null;

  return <Layout style={{ height: "100%" }}>
    <Layout.Header>
      <Flex align="center" gap={10} style={{ height: "100%" }}>
        <Brand />
        <Divider type="vertical" style={{ height: 20 }} />
        <Select
          value={filter.site} style={{ width: 132 }}
          onChange={(site: SiteId | "") => setFilter({ ...filter, site, account: "" })}
          options={[{ value: "", label: t("全部站点") }, ...SITE_IDS.map((s) => ({ value: s, label: siteName(s) }))]}
        />
        <Select
          value={filter.account} style={{ width: 142 }}
          onChange={(account: string) => setFilter({ ...filter, account })}
          options={[{ value: "", label: t("全部账号") }, ...siteAccounts.map((a) => ({ value: a.key, label: accountDisplayName(a) }))]}
        />
        {filter.account && <Tooltip title={t("改备注名")}>
          <Button type="text" icon={<EditOutlined />} onClick={async () => {
            const alias = await askText(modal, { title: t("账号备注名"), value: aliasOf(filter.account) });
            if (alias !== null) await renameAccount(db, filter.account, alias).then(() => reload(db));
          }} />
        </Tooltip>}
        <Dropdown menu={{
          items: [
            { key: "*", label: t("全部站点") },
            { type: "divider" as const },
            ...SITE_IDS.map((s) => ({ key: s, label: SITES[s].label })),
          ],
          onClick: ({ key }) => void (key === "*" ? refreshAll() : refresh(key as SiteId)),
        }}>
          <Button type="primary" icon={<ReloadOutlined />} loading={!!busy}>{t("刷新")} <DownOutlined /></Button>
        </Dropdown>
        <Dropdown menu={{
          items: SITE_IDS.map((s) => ({ key: s, label: <a href={SITES[s].origin} target="_blank" rel="noreferrer">{SITES[s].label}</a> })),
        }}>
          <Button type="text" icon={<ExportOutlined />}>{t("打开网站")}</Button>
        </Dropdown>

        <div style={{ flex: 1 }} />

        {brokenNow.length > 0 && <Tooltip title={t("接口已变化，请先刷新该站点")}>
          <Tag color="error">{brokenNow.map((s) => SITES[s].label).join("、")}：{t("接口已变化")}</Tag>
        </Tooltip>}
        {unverified.length > 0 && <Tooltip title={t("这些站点的接口还没有在真实账号上核对过：可以刷新、读取和导出，暂不支持删除。")}>
          <Tag color="warning">{unverified.map((s) => SITES[s].label).join("、")}：{t("未实测")}</Tag>
        </Tooltip>}
        <SyncStatus
          status={bridge} busy={!!busy}
          onReconnect={() => void bridgeStatus(true, true).then(setBridge, () => setBridge(null))}
          onRestore={restore}
        />
        <Popover content={settings} trigger="click" placement="bottomRight" title={t("设置")}>
          <Button type="text" icon={<SettingOutlined />} aria-label={t("设置")} />
        </Popover>
      </Flex>
    </Layout.Header>

    <Layout>
      <Layout.Sider width={218} theme="light">
        <div className="pane pane-pad">
          <Typography.Text type="secondary">{t("文件夹")}</Typography.Text>
          <Menu
            mode="inline" style={{ background: "transparent", borderInlineEnd: "none", marginTop: 4 }}
            selectedKeys={[folderKey]} items={folderItems}
            onClick={({ key }) => setFilter({ ...filter, folder: key === "all" ? "" : key })}
          />
          <Button block size="small" type="dashed" icon={<PlusOutlined />} style={{ marginTop: 8 }} onClick={() => void newFolder()}>
            {t("新建文件夹")}
          </Button>
          <Divider style={{ margin: "14px 0 10px" }} />
          <Typography.Text type="secondary">{t("标签")}</Typography.Text>
          <Space size={[6, 6]} wrap style={{ marginTop: 6 }}>
            <Tag.CheckableTag checked={!filter.tag} onChange={() => setFilter({ ...filter, tag: "" })}>{t("全部标签")}</Tag.CheckableTag>
            {tags.map((tag) => <Tag.CheckableTag
              key={tag} checked={filter.tag === tag}
              onChange={(on) => setFilter({ ...filter, tag: on ? tag : "" })}
            >{tag}</Tag.CheckableTag>)}
          </Space>
        </div>
      </Layout.Sider>

      <Layout.Content className="pane">
        <div className="pane-pad">
          <Filters value={filter} onChange={setFilter} tags={tags} />
          {selected.length > 0 && toolbar}
          {shown.length === 0
            ? <Empty
              style={{ marginTop: 64 }}
              description={t("没有对话。先打开并登录 ChatGPT、Claude、Gemini、Grok 或 DeepSeek，再点「刷新」。")}
            />
            : <ConversationList
              items={shown} aliasOf={aliasOf} selected={selected} active={active?.key ?? null}
              onOpen={setActive} onSelect={setSelected}
            />}
        </div>
      </Layout.Content>

      {wide && <Layout.Sider width={416} theme="light">
        <div className="pane pane-pad">
          {active ? detail : <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} style={{ marginTop: 64 }} description={t("选择一条对话查看详情")} />}
        </div>
      </Layout.Sider>}
    </Layout>

    <Drawer
      open={!wide && !!active} width={Math.min(440, window.innerWidth - 40)} onClose={() => setActive(null)}
      title={t("对话详情")} styles={{ body: { paddingTop: 12 } }}
    >{detail}</Drawer>

    <Modal open={!!busy} closable={false} maskClosable={false} keyboard={false} footer={null} width={286} centered>
      <Flex vertical align="center" gap={14} style={{ padding: "10px 0" }}>
        <Spin size="large" />
        <Typography.Text>{busy}…</Typography.Text>
        <Typography.Text type="secondary">{t("请不要关闭此页")}</Typography.Text>
      </Flex>
    </Modal>

    {deleting && <DeleteDialog
      items={deleting} currentAccounts={current} aliasOf={aliasOf} siteErrors={siteErrors}
      onRun={async (runItems, mode, signal, onProgress) => {
        const { save } = await pickSaver(connectedNow);
        const results = await runDeleteJob(runItems, mode, { api, db, pacer: createPacer(), now: Date.now, save, aliasOf }, signal, onProgress);
        const newlyBroken = brokenSitesIn(runItems, results);
        if (newlyBroken.length) setBroken(await brokenStore.mark(newlyBroken, Date.now()));
        return results;
      }}
      onClose={(changed) => { setDeleting(null); if (changed) { setSelected([]); void reload(db); } }}
    />}
  </Layout>;
}
