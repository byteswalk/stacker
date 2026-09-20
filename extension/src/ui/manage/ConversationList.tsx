import { StarFilled } from "@ant-design/icons";
import { Button, Flex, Space, Table, Tag, Tooltip, Typography } from "antd";
import type { ColumnsType } from "antd/es/table";
import { t } from "../../i18n";
import { bodyIsFresh, type Conversation } from "../../lib/db";
import { SiteTag } from "../bits";

const PAGE = 50;

function Title({ c }: { c: Conversation }) {
  return <div className="title-cell">
    <div className="line">
      {c.favorite && <StarFilled style={{ color: "#e4b450", fontSize: 12 }} />}
      <span className="text">{c.title || t("（无标题）")}</span>
      {c.removedAt !== null && <Tag color="error" variant="filled">{t("已删除")}</Tag>}
      {c.archived && <Tag variant="filled">{t("已归档")}</Tag>}
    </div>
    {c.tags.length > 0 && <div className="line">
      {c.tags.map((tag) => <Tag key={tag} variant="filled" style={{ marginInlineEnd: 0 }}>{tag}</Tag>)}
    </div>}
  </div>;
}

export function ConversationList({ items, aliasOf, selected, onSelect, onOpen, active }: {
  items: Conversation[];
  aliasOf: (key: string) => string;
  selected: string[];
  onSelect: (keys: string[]) => void;
  onOpen: (c: Conversation) => void;
  active: string | null;
}) {
  const columns: ColumnsType<Conversation> = [
    { title: t("对话"), dataIndex: "title", ellipsis: true, render: (_: unknown, c) => <Title c={c} /> },
    { title: t("站点"), dataIndex: "site", width: 104, render: (_: unknown, c) => <SiteTag site={c.site} /> },
    {
      title: t("账号"), dataIndex: "account", width: 132, ellipsis: true,
      render: (_: unknown, c) => <Typography.Text type="secondary" ellipsis>{aliasOf(c.account)}</Typography.Text>,
    },
    {
      title: t("更新时间"), dataIndex: "updatedAt", width: 156, defaultSortOrder: "descend",
      sorter: (a, b) => a.updatedAt - b.updatedAt,
      render: (_: unknown, c) => <Space size={6}>
        <Typography.Text type="secondary">{new Date(c.updatedAt).toLocaleString()}</Typography.Text>
        {bodyIsFresh(c) && <Tooltip title={t("正文已读")}><Tag color="green" variant="filled" style={{ marginInlineEnd: 0 }}>{t("正文")}</Tag></Tooltip>}
      </Space>,
    },
  ];

  const allSelected = items.length > 0 && selected.length >= items.length;
  return <>
    {items.length > 0 && <Flex align="center" gap={8} style={{ margin: "8px 0 2px" }}>
      <Button size="small" type="link" disabled={allSelected} onClick={() => onSelect(items.map((c) => c.key))}>
        {t("选中全部结果")}（{items.length}）
      </Button>
    </Flex>}
    <Table<Conversation>
      size="small"
      rowKey="key"
      columns={columns}
      dataSource={items}
      showSorterTooltip={false}
      rowClassName={(c) => (c.key === active ? "ant-table-row-selected" : "")}
      onRow={(c) => ({ onClick: () => onOpen(c), style: { cursor: "pointer" } })}
      rowSelection={{
        selectedRowKeys: selected,
        onChange: (keys) => onSelect(keys as string[]),
        selections: [Table.SELECTION_ALL, Table.SELECTION_INVERT, Table.SELECTION_NONE],
      }}
      pagination={{
        defaultPageSize: PAGE,
        pageSizeOptions: [20, 50, 100, 200],
        showSizeChanger: true,
        showTotal: (total) => `${t("共")} ${total} ${t("条")}`,
      }}
    />
  </>;
}
