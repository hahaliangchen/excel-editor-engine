import {icons,type IconName} from './icons';
export function element<K extends keyof HTMLElementTagNameMap>(tag: K, className = '', text?: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}
export function icon(name: IconName) {
  const element = document.createElement('span');element.className='icon';element.setAttribute('aria-hidden','true');element.innerHTML=icons[name];return element;
}
export function button(label: string, glyph?: IconName, text?: string, className='tool-button') {
  const btn=element('button',className);
  btn.type='button';btn.title=label;btn.setAttribute('aria-label',label);
  if(glyph)btn.append(icon(glyph));if(text)btn.append(element('span','button-label',text));return btn;
}
export function segment(...children: HTMLElement[]) {
  const group=element('div','toolbar-group');group.append(...children);return group;
}
export function menu(label:string,glyph:IconName,actions: Array<{label:string;icon?:IconName;onClick:()=>void}>,opensUp=false) {
  const details=element('details',opensUp?'menu menu-up':'menu');
  const summary=element('summary','tool-button');summary.title=label;summary.setAttribute('aria-label',label);summary.append(icon(glyph));
  if(!opensUp)summary.append(icon('chevron-down'));
  const list=element('div','menu-list');
  for(const action of actions) {const item=button(action.label,action.icon,action.label,'menu-item');item.addEventListener('click',()=>{details.open=false;action.onClick();});list.append(item);}
  details.append(summary,list);
  details.addEventListener('toggle',()=>{if(details.open) document.querySelectorAll<HTMLDetailsElement>('details.menu[open]').forEach(other=>{if(other!==details)other.open=false;});});
  details.addEventListener('keydown',e=>{if(e.key==='Escape' && details.open){e.stopPropagation();details.open=false;summary.focus();}});
  return {root:details,summary,list};
}
