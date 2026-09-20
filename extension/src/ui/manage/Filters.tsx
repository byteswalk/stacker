import { SearchOutlined } from "@ant-design/icons";
import { Checkbox, DatePicker, Input, Select, Space } from "antd";
import dayjs, { type Dayjs } from "dayjs";
import { t } from "../../i18n";
import type { Filter } from "../../lib/search";

const day = (ms: number | null) => (ms === null ? null : dayjs(ms));

export function Filters({ value, onChange, tags }: { value: Filter; onChange: (f: Filter) => void; tags: string[] }) {
  const set = (patch: Partial<Filter>) => onChange({ ...value, ...patch });
  return <Space size={[10, 8]} wrap style={{ width: "100%" }}>
    <Input
      allowClear
      prefix={<SearchOutlined className="dim" />}
      placeholder={t("搜索标题或备注")}
      value={value.text}
      onChange={(e) => set({ text: e.target.value })}
      style={{ width: 232 }}
    />
    <Checkbox checked={value.inBody} onChange={(e) => set({ inBody: e.target.checked })}>{t("搜索正文（仅已读取的对话）")}</Checkbox>
    <Select
      value={value.tag}
      onChange={(tag: string) => set({ tag })}
      style={{ width: 136 }}
      options={[{ value: "", label: t("全部标签") }, ...tags.map((tag) => ({ value: tag, label: tag }))]}
    />
    <Select
      value={value.body}
      onChange={(body: Filter["body"]) => set({ body })}
      style={{ width: 118 }}
      options={[
        { value: "any", label: t("正文：全部") },
        { value: "read", label: t("正文已读") },
        { value: "unread", label: t("正文未读") },
      ]}
    />
    <DatePicker.RangePicker
      value={[day(value.from), day(value.to)]}
      onChange={(range) => {
        const [from, to] = (range ?? [null, null]) as (Dayjs | null)[];
        set({ from: from ? from.startOf("day").valueOf() : null, to: to ? to.endOf("day").valueOf() : null });
      }}
      allowEmpty={[true, true]}
      placeholder={[t("起始日期"), t("结束日期")]}
      style={{ width: 232 }}
    />
    <Checkbox checked={value.favorite} onChange={(e) => set({ favorite: e.target.checked })}>{t("仅收藏")}</Checkbox>
    <Checkbox checked={value.showRemoved} onChange={(e) => set({ showRemoved: e.target.checked })}>{t("显示已删除")}</Checkbox>
  </Space>;
}
