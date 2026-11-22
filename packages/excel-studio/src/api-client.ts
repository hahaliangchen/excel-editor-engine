import type {Operation, Workbook} from './model';

export function apiBase(page: URL): URL {
  const quickAgent = /^(.*\/agent\/skill-packages\/[^/]+)\/pages?\//.exec(page.pathname);
  if (quickAgent) return new URL(quickAgent[1]+'/local-api/api/',page);
  return new URL('http://127.0.0.1:8083/api/');
}
export class ApiClient {
  constructor(private readonly base: URL = apiBase(new URL(location.href))) {}
  private async request(path: string, init?: RequestInit): Promise<Response> {
    let response: Response;
    try {response = await fetch(new URL(path,this.base),{...init,signal:AbortSignal.timeout(60_000)});}
    catch (error) {throw new Error('无法连接本地 Excel 服务，请检查服务是否已启动。',{cause:error});}
    if (!response.ok) {
      const data = await response.text();
      let message = data;
      try {message = JSON.parse(data).error ?? data;} catch { /* JSON extraction is optional. */ }
      throw new Error(message.slice(0,240) || '本地服务请求失败：' + response.status);
    }
    return response;
  }
  async workbook(path: string, init?: RequestInit): Promise<Workbook> { return (await this.request('xlsx/'+path,init)).json(); }
  sample() { return this.workbook('sample'); }
  create() { return this.workbook('create'); }
  parse(file: File) { return this.workbook('parse',{method:'POST',headers:{'Content-Type':'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet'},body:file}); }
  recalc(workbook: Workbook) { return this.workbook('recalc',this.json(workbook)); }
  operate(workbook: Workbook, operation: Operation) { return this.workbook('operate',this.json({workbook,operation})); }
  async export(workbook: Workbook) { return (await this.request('xlsx/export',this.json(workbook))).blob(); }
  private json(body: unknown): RequestInit { return {method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)}; }
}
