export type Theme = 'light' | 'dark';
const params = new URLSearchParams(location.search);
export const preview = params.get('mode') === 'preview';

export function sameOriginFile(input: string): URL {
  const url = new URL(input, location.origin);
  if (url.origin !== location.origin || !url.pathname.startsWith('/agent/') || url.username || url.password) {
    throw new Error('仅允许打开同源 /agent/ 文件');
  }
  return url;
}

export function connectStudio(options: {
  theme: (theme: Theme) => void;
  load: (url: URL, name: string, signal: AbortSignal, reload: boolean) => Promise<{ pages?: number; sheets?: number } | void>;
  kind: string;
  error: (error: unknown) => void;
}): boolean {
  let active: AbortController | undefined;
  const send = (message: unknown) => { if (parent !== window) parent.postMessage(message, location.origin); };
  const theme = (value: unknown) => { if (value === 'light' || value === 'dark') options.theme(value); };
  const load = async (file: string, name: string, reload: boolean) => {
    active?.abort();
    const controller = new AbortController();
    active = controller;
    try {
      const url = sameOriginFile(file);
      const meta = await options.load(url, name || url.pathname.split('/').pop() || 'document', controller.signal, reload);
      if (!controller.signal.aborted && preview) send({ type: 'quickagent:preview-ready', kind: options.kind, meta });
    } catch (error) {
      if (!controller.signal.aborted) {
        options.error(error);
        if (preview) send({ type: 'quickagent:preview-error', message: error instanceof Error ? error.message : String(error) });
      }
    }
  };
  window.addEventListener('message', event => {
    if (parent === window || event.source !== parent || event.origin !== location.origin) return;
    const message = event.data;
    if (message?.type === 'quickagent:theme') theme(message.theme);
    if (message?.type === (preview ? 'quickagent:reload-file' : 'quickagent:open-file') && typeof message.file === 'string') {
      void load(message.file, typeof message.name === 'string' ? message.name : '', preview);
    }
  });
  theme(params.get('theme'));
  const file = params.get('file');
  if (file) void load(file, params.get('name') ?? '', false);
  return !!file || preview;
}

export async function fetchStudioFile(url: URL, name: string, signal: AbortSignal): Promise<File> {
  const response = await fetch(url, { signal, cache: 'no-store', redirect: 'error' });
  if (!response.ok) throw new Error('无法读取文件（HTTP ' + response.status + '）');
  const blob = await response.blob();
  return new File([blob], name, { type: blob.type });
}
