import { ExportOutlined, ReloadOutlined, StarFilled, StarOutlined } from "@ant-design/icons";
import { Button, Empty, Flex, Input, Select, Space, Tabs, Tag, Tooltip, Typography } from "antd";
import { useEffect, useState } from "react";
import { t } from "../../i18n";
import { addTag, getBody, listExcerpts, updateLocal, type Conversation, type Db, type Excerpt, type Folder, type StoredBody } from "../../lib/db";
import { DISTILL_KIND_LABEL, distillResults, type DistillItem } from "../../lib/distill";
import { conversationUrl } from "../../sites/registry";
import { SiteTag } from "../bits";

function Message({ role, text }: { role: string; text: string }) {
  const label = role === "user" ? t("用户") : role === "assistant" ? t("助手") : role;
  return <div className={`msg ${role === "user" ? "msg-user" : "msg-other"}`}>
    <span className="msg-role">{label}</span>{text}
  </div>;
}

export function Detail({ db, conv, folders, onRead, onChanged, reading }: {
  db: Db; conv: Conversation; folders: Folder[]; reading: boolean;
  onRead: (c: Conversation) => void; onChanged: () => void;
}) {
  const [body, setBody] = useState<StoredBody | undefined>();
  const [excerpts, setExcerpts] = useState<Excerpt[]>([]);
  const [distilled, setDistilled] = useState<DistillItem[]>([]);
  const [note, setNote] = useState(conv.note);
  const [tag, setTag] = useState("");
  useEffect(() => {
    setNote(conv.note);
    void getBody(db, conv.key).then(setBody);
    void listExcerpts(db, conv.site, conv.id).then(setExcerpts);
    void distillResults(conv.site, conv.id).then(setDistilled, () => setDistilled([]));
  }, [db, conv]);
  const url = conversationUrl(conv.site, conv.id);
  const save = async (patch: Parameters<typeof updateLocal>[2]) => { await updateLocal(db, [conv.key], patch); onChanged(); };

  const bodyTab = body?.messages.length
    ? <div>{body.messages.map((m, i) => <Message key={i} role={m.role} text={m.text} />)}</div>
    : <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={t("还没有读取正文")} />;

  const tabs = [
    { key: "body", label: t("正文"), children: bodyTab },
    {
      key: "excerpts", label: `${t("摘录")} ${excerpts.length || ""}`.trim(),
      children: excerpts.length
        ? <div>{excerpts.map((x) => <div key={x.id} className="msg msg-other">{x.text}
          {x.note && <div className="dim" style={{ marginTop: 4 }}>{x.note}</div>}</div>)}</div>
        : <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={t("在网页上选中文字，点出现的「存为摘录」按钮即可添加。")} />,
    },
  ];
  if (distilled.length > 0) {
    tabs.push({
      key: "distilled", label: `${t("提炼结果")} ${distilled.length}`,
      children: <div>
        {distilled.map((d) => <div key={d.id} className="msg msg-other">
          <span className="msg-role">
            <Tag variant="filled" color="purple">{t(DISTILL_KIND_LABEL[d.kind] ?? d.kind)}</Tag>
            {d.state === "adopted" && <Tag variant="filled" color="green">{t("已采用")}</Tag>}
          </span>
          <b>{d.title}</b>{"\n\n"}{d.body}
        </div>)}
        <Typography.Text type="secondary">{t("提炼在 Stacker 里进行，这里只能查看。")}</Typography.Text>
      </div>,
    });
  }

  return <Flex vertical gap={10} style={{ height: "100%" }}>
    <div>
      <Flex align="center" gap={8} style={{ marginBottom: 6 }}>
        <SiteTag site={conv.site} />
        <Tooltip title={t("收藏")}>
          <Button
            size="small" type="text"
            icon={conv.favorite ? <StarFilled style={{ color: "#e4b450" }} /> : <StarOutlined />}
            onClick={() => void save({ favorite: !conv.favorite })}
          />
        </Tooltip>
        <Tooltip title={t("打开原对话")}>
          <Button size="small" type="text" icon={<ExportOutlined />} href={url} target="_blank" rel="noreferrer" />
        </Tooltip>
      </Flex>
      <Typography.Title level={5} style={{ margin: 0 }}>{conv.title || t("（无标题）")}</Typography.Title>
    </div>

    <Select
      value={conv.folderId ?? ""}
      onChange={(id: string) => void save({ folderId: id || null })}
      style={{ width: "100%" }}
      options={[{ value: "", label: t("未归入文件夹") }, ...folders.map((f) => ({ value: f.id, label: f.name }))]}
    />

    <Space size={[6, 6]} wrap>
      {conv.tags.map((x) => <Tag key={x} closable variant="filled" onClose={() => void save({ tags: conv.tags.filter((y) => y !== x) })}>{x}</Tag>)}
      <Input
        size="small" style={{ width: 118 }} placeholder={t("加标签")} value={tag}
        onChange={(e) => setTag(e.target.value)}
        onPressEnter={() => { if (tag.trim()) void addTag(db, [conv.key], tag.trim()).then(onChanged); setTag(""); }}
      />
    </Space>

    <Input.TextArea
      rows={3} placeholder={t("备注")} value={note}
      onChange={(e) => setNote(e.target.value)}
      onBlur={() => { if (note !== conv.note) void save({ note }); }}
    />

    <Flex align="center" gap={8}>
      <Button
        size="small" icon={<ReloadOutlined />} loading={reading} disabled={reading || conv.removedAt !== null}
        onClick={() => onRead(conv)}
      >{t(body ? "重新读取正文" : "读取正文")}</Button>
      {body && <Typography.Text type="secondary">{t("读取于")} {new Date(conv.bodyFetchedAt ?? 0).toLocaleString()}</Typography.Text>}
    </Flex>

    <Tabs items={tabs} size="small" style={{ flex: 1, minHeight: 0 }} />
  </Flex>;
}
