/**
 * 域名 · API 工具 (re-export · 真正实现在 @shared/utils/domains)
 *
 * 兼容老引用 · 新代码请直接 import "@shared"
 */
export {
  PRIMARY_DOMAIN,
  FALLBACK_DOMAINS,
  ALL_DOMAINS,
  API_BASE,
  WS_BASE,
  apiUrl,
  wsUrl,
  allUrls,
  fetchWithFallback,
} from "@shared/utils/domains"
