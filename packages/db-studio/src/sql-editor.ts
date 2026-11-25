const words=['SELECT','FROM','WHERE','JOIN','ON','ORDER BY','GROUP BY','HAVING','LIMIT','OFFSET','WITH','AS','AND','OR','INSERT INTO','UPDATE','SET','DELETE FROM','EXPLAIN'];
const SQL_TOKENS= /\/\*[\s\S]*?\*\/|--[^\n]*|'(?:[^']|'')*'|"(?:[^"]|"")*"|[A-Za-z_][\w$]*|[\d]+(?:\.[\d]+)?|[^\w]|./gu;
export function mark(sql:string,node:HTMLElement) {
  const output=document.createDocumentFragment();
  for(const token of sql.match(SQL_TOKENS)??[]) {
    const cls=/^(?:\/\*|--)/.test(token)?'comment':/^['"]/.test(token)?'literal':/^\d/.test(token)?'number':words.some(w=>w.split(' ').includes(token.toUpperCase()))?'keyword':'';
    if(!cls){output.append(token);continue;}
    const part=document.createElement('span');part.className=cls;part.textContent=token;output.append(part);
  }
  output.append('\n');node.replaceChildren(output);
}
export function formatSql(sql:string){
  // Never reflow across quoting rules we do not understand.
  if(/['"$\\]|\/\*|--/.test(sql)||sql.includes(String.fromCharCode(96)))return sql;
  const clauses=/\b(SELECT|FROM|WHERE|LEFT JOIN|RIGHT JOIN|INNER JOIN|JOIN|ORDER BY|GROUP BY|HAVING|LIMIT|OFFSET|VALUES|SET)\b/gi;
  return sql.trim().replace(clauses,(_m:string,w:string,i:number)=>(i?'\n':'')+w.toUpperCase()).replace(/[ \t]+\n/g,'\n');
}
export function suggestions(prefix:string,names:string[]){return [...new Set([...names,...words])].filter(n=>n.toLowerCase().startsWith(prefix.toLowerCase())&&n!==prefix).slice(0,12);}
export function savedHistory():string[]{try {const a:unknown=JSON.parse(localStorage.getItem('db-studio-history')||'[]');return Array.isArray(a)?a.filter((v):v is string=>typeof v==='string').slice(0,30):[];}catch{return [];}}
export function remember(sql:string){if(sql.length>16_000)return;try{localStorage.setItem('db-studio-history',JSON.stringify([sql,...savedHistory().filter(s=>s!==sql)].slice(0,30)));}catch{/* storage unavailable */}}
