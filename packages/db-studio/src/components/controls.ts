import {icons} from '../../../excel-studio/src/components/icons';
export const el=<K extends keyof HTMLElementTagNameMap>(tag:K,cls='',text?:string)=>{const e=document.createElement(tag);e.className=cls;if(text!==undefined)e.textContent=text;return e;};
export function btn(label:string,icon?:keyof typeof icons,visible=true,cls=''){
  const b=el('button','db-button '+cls);b.type='button';b.title=label;b.setAttribute('aria-label',label);
  if(icon){const i=el('span','icon');i.innerHTML=icons[icon];i.setAttribute('aria-hidden','true');b.append(i);}
  if(visible)b.append(label);
  return b;
}
export function heading(label:string){return el('span','db-section-label',label);}
export function input(label:string,type='text'){const i=el('input');i.type=type;i.setAttribute('aria-label',label);return i;}
export function select(label:string,options:[string,string][]){const s=el('select');s.setAttribute('aria-label',label);for(const [v,text] of options){const o=el('option','',text);o.value=v;s.append(o);}return s;}
export function field(label:string,control:HTMLElement){const l=el('label','db-field');l.append(el('span','',label),control);return l;}
export function modal(title:string){
  const dialog=el('dialog','db-dialog');const header=el('header','db-dialog-head');const h=el('h2','',title);const close=btn('关闭','x',false);close.onclick=()=>dialog.close();header.append(h,close);
  const body=el('div','db-dialog-body');const foot=el('footer','db-dialog-foot');dialog.append(header,body,foot);document.body.append(dialog);
  dialog.setAttribute('aria-label',title);dialog.addEventListener('close',()=>dialog.remove(),{once:true});
  return {dialog,body,foot,open:()=>dialog.showModal(),close:()=>dialog.close()};
}
export function download(blob:Blob,name:string){const link=el('a');const url=URL.createObjectURL(blob);link.href=url;link.download=name;link.click();setTimeout(()=>URL.revokeObjectURL(url),10_000);}
export async function base64(file:File){
  if(file.size>32*1024*1024)throw new Error('Excel 文件不能超过 32 MiB');
  return await new Promise<string>((resolve,reject)=>{const r=new FileReader();r.onerror=()=>reject(r.error);r.onload=()=>resolve(String(r.result).split(',',2)[1]);r.readAsDataURL(file);});
}
