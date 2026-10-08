import { useI18n } from "../../../i18n";
import { invoke } from "../../../invoke";
import { AiAskModal, askAi } from "../../ai/AiAsk";
import type { DirectoryNode, Paged } from "../types";
import { formatSpaceBytes } from "./SpaceOverview";

/** How many of the biggest subfolders the AI is shown. */
const CHILDREN = 30;

/**
 * What a folder is, where its space goes and what can go: the folder and its biggest subfolders
 * (names and sizes from the scan) are handed to the AI for one overall reading.
 */
export function DirectoryAi({ taskId, node, onClose }: { taskId: string; node: DirectoryNode; onClose: () => void }) {
  const { tr } = useI18n();
  async function run() {
    const page = node.childCount > 0
      ? await invoke<Paged<DirectoryNode>>("space_scan_children", { taskId, parentId: node.nodeId, offset: 0, limit: 200 })
      : { items: [] as DirectoryNode[], total: 0, offset: 0, limit: 0 };
    const children = [...page.items].sort((a, b) => b.allocatedBytes - a.allocatedBytes).slice(0, CHILDREN)
      .map((child) => ({ name: child.name, size: formatSpaceBytes(child.allocatedBytes), subfolders: child.childCount }));
    return askAi("disk_directory", { path: node.path, size: formatSpaceBytes(node.allocatedBytes), children });
  }
  return <AiAskModal title={tr("AI 解读这个目录")} sub={node.path} waiting="AI 正在看这个目录…" saveAs={`space-dir:${node.path}:${node.allocatedBytes}`}
    note={tr("只把目录路径、子目录名称和大小发给 AI，不读取文件内容。回答仅供参考，删前以清理确认为准。")}
    run={run} onClose={onClose} />;
}
