import { t } from "../../i18n";
import type { Filter } from "../../lib/search";

const toDay = (ms: number | null) => (ms === null ? "" : new Date(ms).toISOString().slice(0, 10));
const fromDay = (text: string, endOfDay: boolean) => (text ? Date.parse(`${text}T${endOfDay ? "23:59:59" : "00:00:00"}`) : null);

export function Filters({ value, onChange, tags }: { value: Filter; onChange: (f: Filter) => void; tags: string[] }) {
  const set = (patch: Partial<Filter>) => onChange({ ...value, ...patch });
  return <div className="filters">
    <input placeholder={t("搜索标题或备注")} value={value.text} onChange={(e) => set({ text: e.target.value })} />
    <label><input type="checkbox" checked={value.inBody} onChange={(e) => set({ inBody: e.target.checked })} /> {t("搜索正文（仅已读取的对话）")}</label>
    <select value={value.tag} onChange={(e) => set({ tag: e.target.value })}>
      <option value="">{t("全部标签")}</option>
      {tags.map((tag) => <option key={tag} value={tag}>{tag}</option>)}
    </select>
    <select value={value.body} onChange={(e) => set({ body: e.target.value as Filter["body"] })}>
      <option value="any">{t("正文：全部")}</option>
      <option value="read">{t("正文已读")}</option>
      <option value="unread">{t("正文未读")}</option>
    </select>
    <input type="date" value={toDay(value.from)} onChange={(e) => set({ from: fromDay(e.target.value, false) })} aria-label={t("起始日期")} />
    <input type="date" value={toDay(value.to)} onChange={(e) => set({ to: fromDay(e.target.value, true) })} aria-label={t("结束日期")} />
    <label><input type="checkbox" checked={value.favorite} onChange={(e) => set({ favorite: e.target.checked })} /> {t("仅收藏")}</label>
    <label><input type="checkbox" checked={value.showRemoved} onChange={(e) => set({ showRemoved: e.target.checked })} /> {t("显示已删除")}</label>
  </div>;
}
