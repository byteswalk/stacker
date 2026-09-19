import { t } from "../../i18n";
import type { BrokenSites } from "../../lib/brokenSites";
import type { SiteId } from "../../shared/types";
import { SITES } from "../../sites/registry";

/** The site's name in pickers; unverified sites say so. */
export function siteName(site: SiteId): string {
  return SITES[site].verified ? SITES[site].label : `${SITES[site].label}（${t("未实测")}）`;
}

/** Why the site's conversations cannot be deleted right now, or null when they can; shown per site in the delete dialog. */
export function deleteBlockReason(site: SiteId, broken: BrokenSites): string | null {
  if (!SITES[site].verified) return t("未实测：该站点的接口还没有在真实账号上核对过，暂不支持删除");
  if (broken[site]) return t("接口已变化，请先刷新该站点");
  return null;
}
