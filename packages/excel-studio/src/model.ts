export const ROWS = 1_048_576;
export const COLS = 16_384;
export const MAX_SELECTION = 200_000;
export type Value = null | number | string | boolean;
export interface CellStyle {
  bold: boolean;
  italic: boolean;
  color: string | null;
  fill: string | null;
  align: 'left' | 'center' | 'right' | null;
  numberFormat: string | null;
}
export interface Cell {
  raw: string;
  formula: string | null;
  value: Value;
  display: string;
  style: CellStyle;
  error?: boolean;
  origin?: string;
}
export interface Sheet {
  name: string;
  cells: Record<string, Cell>;
  merges: string[];
  widths: Record<number, number>;
  frozen: boolean;
  sourceIndex?: number;
}
export interface Workbook { sheets: Sheet[]; source?: string }
export interface Coord { row: number; col: number }
export interface Range { start: Coord; end: Coord }
export interface Selection { anchor: Coord; focus: Coord }
export const home = (): Coord => ({row: 1, col: 1});
export const blankStyle = (): CellStyle => ({bold: false, italic: false, color: null, fill: null, align: null, numberFormat: null});
export const blankSheet = (name: string): Sheet => ({name, cells: {}, merges: [], widths: {}, frozen: false});
export const blankWorkbook = (): Workbook => ({sheets: [blankSheet('Sheet1')]});

export function columnLabel(col: number): string {
  let text = '';
  while (col > 0) { col--; text = String.fromCharCode(65 + col % 26) + text; col = Math.floor(col / 26); }
  return text;
}
export function validCoord({row, col}: Coord): boolean {
  return Number.isInteger(row) && Number.isInteger(col) && row >= 1 && row <= ROWS && col >= 1 && col <= COLS;
}
export const toA1 = ({row, col}: Coord): string => columnLabel(col) + row;
export function fromA1(input: string): Coord | null {
  const m = /^\$?([A-Z]{1,3})\$?(\d+)$/i.exec(input.trim());
  if (!m) return null;
  const p = {row: Number(m[2]), col: [...m[1].toUpperCase()].reduce((acc, char) => 26 * acc + char.charCodeAt(0) - 64, 0)};
  return validCoord(p) ? p : null;
}
export const sameCoord = (a: Coord, b: Coord): boolean => a.row === b.row && a.col === b.col;
export const clamp = ({row, col}: Coord): Coord => ({row: Math.max(1, Math.min(ROWS, row)), col: Math.max(1, Math.min(COLS, col))});
export const bounds = (a: Coord, b: Coord): Range => ({start: {row: Math.min(a.row,b.row), col: Math.min(a.col,b.col)}, end: {row: Math.max(a.row,b.row), col: Math.max(a.col,b.col)}});
export const contains = ({start, end}: Range, c: Coord): boolean => start.row <= c.row && c.row <= end.row && start.col <= c.col && c.col <= end.col;
export const overlaps = (a: Range, b: Range): boolean => a.start.row <= b.end.row && b.start.row <= a.end.row && a.start.col <= b.end.col && b.start.col <= a.end.col;
export const rangeSize = ({start, end}: Range): number => (end.row - start.row + 1) * (end.col - start.col + 1);
export const rangeA1 = ({start, end}: Range): string => sameCoord(start,end) ? toA1(start) : toA1(start) + ':' + toA1(end);
export function parseRange(s: string): Range | null {
  const parts = s.split(':');
  const a = fromA1(parts[0]);
  const b = parts.length === 1 ? a : fromA1(parts[1]);
  return parts.length <= 2 && a && b ? bounds(a,b) : null;
}
export const sheetMerges = (sheet: Sheet): Range[] => sheet.merges.flatMap(s => {const r = parseRange(s); return r ? [r] : [];});

export function fromRaw(raw: string, previous?: Cell): Cell {
  const formula = raw.startsWith('=') ? raw.slice(1) : null;
  let value: Value = null;
  if (formula === null && raw !== '') {
    if (raw.startsWith("'")) value = raw.slice(1);
    else if (/^(true|false)$/i.test(raw)) value = raw.toLowerCase() === 'true';
    else if (/^[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:e[+-]?\d+)?$/i.test(raw.trim()) && Number.isFinite(Number(raw))) value = Number(raw);
    else value = raw;
  }
  const display = value === null ? '' : typeof value === 'boolean' ? String(value).toUpperCase() : String(value);
  return {raw, formula, value, display, style: previous?.style ?? blankStyle(), origin: previous?.origin};
}

export function decodeTsv(input: string): string[][] {
  const rows: string[][] = [];
  let row: string[] = [];
  let field = '';
  let quoted = false;
  let afterBreak = false;
  for (let i = 0; i < input.length; i++) {
    const c = input[i];
    afterBreak = false;
    if (quoted) {
      if (c === '"' && input[i + 1] === '"') { field += '"'; i++; }
      else if (c === '"') quoted = false;
      else field += c;
    } else if (c === '"' && field === '' && input.indexOf('"', i + 1) !== -1) quoted = true;
    else if (c === '\t') { row.push(field); field = ''; }
    else if (c === '\n' || c === '\r') {
      if (c === '\r' && input[i + 1] === '\n') i++;
      row.push(field); rows.push(row); row = []; field = ''; afterBreak = true;
    } else field += c;
  }
  if (!afterBreak) { row.push(field); rows.push(row); }
  return rows;
}
export function encodeTsv(rows: string[][]): string {
  return rows.map(row => row.map(value => /[\t\n\r"]/.test(value) ? '"' + value.replaceAll('"','""') + '"' : value).join('\t')).join('\r\n');
}

export type DimensionChange =
  | {action: 'insert_rows' | 'delete_rows' | 'insert_columns' | 'delete_columns'; at: number; count?: number}
  | {action: 'set_width'; column: number; width: number}
  | {action: 'freeze_first_row'; frozen: boolean};
export type SheetChange = {action: 'new'; name: string} | {action: 'rename' | 'copy'; sheet: string; name: string} | {action: 'delete'; sheet: string};
export type Operation =
  | {op: 'format'; sheet: string; range: string; style?: Partial<CellStyle>; merge?: boolean}
  | {op: 'dimensions'; sheet: string; change: DimensionChange}
  | {op: 'sheets'; change: SheetChange};
