export const PRIMARY_DOMAIN = 'https://www.wujisuanli.com';
export const FALLBACK_DOMAINS = ['https://pidbai.com'];
export const ALL_DOMAINS = [PRIMARY_DOMAIN, ...FALLBACK_DOMAINS];
export const API_BASE = `${PRIMARY_DOMAIN}/api/v8`;
export const WS_BASE = 'wss://www.wujisuanli.com/api/v8/ws';

export const API = {
  bundles: {
    list: () => '/bundles',
  },
  workers: {
    list: () => '/workers',
    workerHistory: (workerId: string) => `/workers/${encodeURIComponent(workerId)}/history`,
  },
  workloads: {
    list: (params?: { limit?: number; offset?: number; status?: string }) => {
      const qs = new URLSearchParams();
      if (params?.limit !== undefined) qs.set('limit', String(params.limit));
      if (params?.offset !== undefined) qs.set('offset', String(params.offset));
      if (params?.status) qs.set('status', params.status);
      return `/workloads${qs.toString() ? `?${qs.toString()}` : ''}`;
    },
  },
  files: '/files',
  skills: '/skills',
  opSlots: '/op-slots',
  scripts: {
    list: () => '/scripts',
  },
} as const;

export function apiUrl(path: string, base = API_BASE): string {
  if (!path) return base;
  if (/^https?:\/\//i.test(path)) return path;
  return `${base.replace(/\/$/, '')}/${path.replace(/^\//, '')}`;
}

export function wsUrl(path: string, base = WS_BASE): string {
  if (!path) return base;
  if (/^wss?:\/\//i.test(path)) return path;
  return `${base.replace(/\/$/, '')}/${path.replace(/^\//, '')}`;
}

export function allUrls(path: string): string[] {
  return ALL_DOMAINS.map((domain) => apiUrl(path, `${domain}/api/v8`));
}

export async function fetchWithFallback(input: string, init?: RequestInit): Promise<Response> {
  let lastErr: unknown;
  for (const url of allUrls(input)) {
    try {
      const resp = await fetch(url, init);
      if (resp.ok || resp.status < 500) return resp;
      lastErr = new Error(`HTTP ${resp.status}`);
    } catch (err) {
      lastErr = err;
    }
  }
  throw lastErr instanceof Error ? lastErr : new Error('fetch failed');
}
