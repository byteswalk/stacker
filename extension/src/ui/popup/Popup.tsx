import { DeleteOutlined, ExportOutlined, StarFilled, StarOutlined } from "@ant-design/icons";
import { Alert, Button, Divider, Empty, Flex, Input, Select, Space, Tag, Tooltip, Typography } from "antd";
import { useCallback, useEffect, useState } from "react";
import { t } from "../../i18n";
import { addTag, conversationKey, deleteExcerpt, getConversation, listExcerpts, listFolders, openDb, updateLocal, type Conversation, type Db, type Excerpt, type Folder } from "../../lib/db";
import { LAST_TAB_KEY, type SiteTab } from "../../lib/lastTab";
import { conversationIdOfUrl } from "../../sites/registry";
import { Brand, SiteTag } from "../bits";

export function Popup() {
  const [db, setDb] = useState<Db | null>(null);
  const [tab, setTab] = useState<SiteTab | null>(null);
  const [conv, setConv] = useState<Conversation | null>(null);
  const [folders, setFolders] = useState<Folder[]>([]);
  const [excerpts, setExcerpts] = useState<Excerpt[]>([]);
  const [tag, setTag] = useState("");

  useEffect(() => {
    void openDb().then(setDb);
    void chrome.storage.session.get(LAST_TAB_KEY).then((v) => setTab((v[LAST_TAB_KEY] as SiteTab) ?? null));
    const onChange = (changes: Record<string, chrome.storage.StorageChange>) => { if (changes[LAST_TAB_KEY]) setTab(changes[LAST_TAB_KEY].newValue as SiteTab); };
    chrome.storage.session.onChanged.addListener(onChange);
    return () => chrome.storage.session.onChanged.removeListener(onChange);
  }, []);

  const id = tab ? conversationIdOfUrl(tab.site, tab.url) : null;
  const load = useCallback(async () => {
    if (!db || !tab || !id) return;
    setConv((await getConversation(db, conversationKey(tab.site, id))) ?? null);
    setFolders(await listFolders(db));
    setExcerpts(await listExcerpts(db, tab.site, id));
  }, [db, tab, id]);
  useEffect(() => { void load(); }, [load]);

  const openManage = <Button block icon={<ExportOutlined />} onClick={() => void chrome.tabs.create({ url: chrome.runtime.getURL("manage.html") })}>
    {t("在管理页打开")}
  </Button>;

  if (!tab || !id) {
    return <Flex vertical gap={12} style={{ padding: 14, minWidth: 300 }}>
      <Brand />
      <Empty
        image={Empty.PRESENTED_IMAGE_SIMPLE}
        description={t("在 ChatGPT、Claude、Gemini、Grok 或 DeepSeek 打开一条对话后，这里会显示它。")}
      />
      {openManage}
    </Flex>;
  }

  const save = async (patch: Parameters<typeof updateLocal>[2]) => { if (db && conv) { await updateLocal(db, [conv.key], patch); await load(); } };
  return <Flex vertical gap={10} style={{ padding: 14, minWidth: 300 }}>
    <Flex align="center" gap={8}>
      <SiteTag site={tab.site} />
      {conv && <Tooltip title={t("收藏")}>
        <Button
          size="small" type="text"
          icon={conv.favorite ? <StarFilled style={{ color: "#e4b450" }} /> : <StarOutlined />}
          onClick={() => void save({ favorite: !conv.favorite })}
        />
      </Tooltip>}
    </Flex>
    <Typography.Title level={5} style={{ margin: 0 }}>{conv?.title || tab.title}</Typography.Title>

    {!conv && <Alert type="warning" showIcon title={t("这条对话还不在列表里：请在管理页刷新该站点。")} />}
    {conv && db && <>
      <Select
        value={conv.folderId ?? ""}
        onChange={(folderId: string) => void save({ folderId: folderId || null })}
        options={[{ value: "", label: t("未归入文件夹") }, ...folders.map((f) => ({ value: f.id, label: f.name }))]}
      />
      <Space size={[6, 6]} wrap>
        {conv.tags.map((x) => <Tag key={x} variant="filled" style={{ marginInlineEnd: 0 }}>{x}</Tag>)}
        <Input
          size="small" style={{ width: 118 }} placeholder={t("加标签")} value={tag}
          onChange={(e) => setTag(e.target.value)}
          onPressEnter={() => { if (tag.trim()) void addTag(db, [conv.key], tag.trim()).then(load); setTag(""); }}
        />
      </Space>
      <Input.TextArea
        rows={4} placeholder={t("备注")} key={conv.key} defaultValue={conv.note}
        onBlur={(e) => { if (e.target.value !== conv.note) void save({ note: e.target.value }); }}
      />
    </>}

    <Divider style={{ margin: "2px 0" }} />
    <Typography.Text type="secondary">{t("摘录")}（{excerpts.length}）</Typography.Text>
    <Typography.Text type="secondary" style={{ fontSize: 12 }}>
      {t("在网页上选中文字，点出现的「存为摘录」按钮即可添加。")}
    </Typography.Text>
    {excerpts.map((x) => <div key={x.id} className="msg msg-other">
      {x.text}
      <Flex justify="flex-end">
        <Button size="small" type="text" danger icon={<DeleteOutlined />} onClick={() => { if (db) void deleteExcerpt(db, x.id).then(load); }}>
          {t("删除")}
        </Button>
      </Flex>
    </div>)}
    {openManage}
  </Flex>;
}
