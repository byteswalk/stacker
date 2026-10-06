import { useI18n } from "../../i18n";
import { Modal } from "../../ui";
import { AiButton } from "../ai/AiAsk";

export type LogRow = { id: number; at: number; endpoint: string; model: string; status: number; elapsedMs: number; detail: string };

type Detail = {
  client?: string; userAgent?: string; stream?: boolean; effort?: string; turns?: number; attachments?: number;
  inputChars?: number; outputChars?: number; error?: string;
};

export const INTERNAL = "stacker://internal";

/** What a status means for someone who called the service. */
export function statusMeaning(status: number): string {
  if (status < 300) return "成功";
  const known: Record<number, string> = {
    400: "请求有误：格式、模型名或推理强度不被接受",
    401: "密钥不对或没带密钥",
    403: "被拒绝：浏览器网页发起的请求，或这个智能体在接口服务里被关掉了",
    404: "没有这个接口",
    429: "排队的请求太多，或智能体账号的额度、套餐不够",
    499: "调用被取消",
    500: "智能体运行失败",
    502: "智能体没有给出可用的回复",
    504: "智能体超时没有回复",
  };
  return known[status] ?? (status >= 500 ? "智能体或服务出错" : "请求没有成功");
}

export function parseDetail(text: string): Detail {
  try { return text ? JSON.parse(text) as Detail : {}; } catch { return {}; }
}

/** One request of the log, with every fact kept about it; never its content. */
export function LogDetail({ row, onClose, onDiagnose }: { row: LogRow; onClose: () => void; onDiagnose: () => void }) {
  const { tr: t } = useI18n();
  const detail = parseDetail(row.detail);
  const internal = row.endpoint === INTERNAL;
  const facts: [string, string | undefined][] = [
    ["时间", new Date(row.at * 1000).toLocaleString()],
    ["接口", internal ? t("Stacker 内部（自己的 AI 功能）") : row.endpoint],
    ["模型", row.model || "—"],
    ["状态", `${row.status} · ${t(statusMeaning(row.status))}`],
    ["耗时", `${(row.elapsedMs / 1000).toFixed(1)}s`],
    ["调用方", detail.client],
    ["客户端", detail.userAgent],
    ["流式", detail.stream === undefined ? undefined : t(detail.stream ? "是" : "否")],
    ["推理强度", detail.effort],
    ["消息轮数", detail.turns?.toString()],
    ["附件", detail.attachments ? String(detail.attachments) : undefined],
    ["输入长度", detail.inputChars === undefined ? undefined : `${detail.inputChars} ${t("字")}`],
    ["输出长度", detail.outputChars === undefined ? undefined : `${detail.outputChars} ${t("字")}`],
  ];
  return <Modal wide title={t("请求详情")} icon="ti-list-details" onClose={onClose}
    footer={<>
      {row.status >= 400 && <AiButton label="AI 诊断" onClick={onDiagnose} />}
      <button className="pr sm" onClick={onClose}>{t("关闭")}</button>
    </>}>
    <div className="gw-detail">
      {facts.filter(([, value]) => value).map(([name, value]) => <div key={name}>
        <span>{t(name)}</span><code translate="no">{value}</code>
      </div>)}
      {detail.error && <div className="bad"><span>{t("错误")}</span><code translate="no">{detail.error}</code></div>}
    </div>
    <p className="s dim" style={{ margin: 0 }}>{t(row.detail
      ? "日志只记调用方、方式、长度和错误，不记任何消息内容。"
      : "这条是早先的记录，当时只记了接口、模型、状态和耗时。")}</p>
  </Modal>;
}
