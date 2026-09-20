'use strict';
const $ = id => document.getElementById(id);
const kinds = {Q:'Questions', R:'Research items', H:'Hypotheses', T:'Theories'};
let snapshot = {records:[], tags:[]}, filter = 'all', tag = '', token = '', requestKey = '';
function element(name, className, text) { const e=document.createElement(name); if(className)e.className=className; if(text!==undefined)e.textContent=text; return e; }
async function api(path, data) { const options=data ? {method:'POST',headers:{'Content-Type':'application/json','X-Research-Token':token},body:JSON.stringify(data)} : {}; const response=await fetch(path,options); const result=await response.json(); if(!response.ok)throw Error(result.error || 'Request failed'); return result; }
async function refresh(){ try{snapshot=await api('/api/corpus');$('notice').hidden=true;render();}catch(e){$('notice').textContent=e.message;$('notice').hidden=false;} }
function render(){
 $('stats').replaceChildren(...Object.entries(kinds).map(([kind,label])=>{const card=element('div','stat');card.append(element('strong','',String(snapshot.records.filter(r=>r.kind===kind).length)),element('span','',label));return card;}));
 $('tags').replaceChildren(...['',...snapshot.tags].map(t=>{const b=element('button','tag'+(tag===t?' selected':''),t||'All tags');b.onclick=()=>{tag=t;render();};return b;}));
 const query=$('search').value.toLowerCase();
 const records=snapshot.records.filter(r=>(filter==='all'||r.kind===filter)&&(!tag||r.metadata.tags.includes(tag))&&[r.id,r.metadata.title,...r.metadata.tags].join(' ').toLowerCase().includes(query));
 $('records').replaceChildren(...records.map(r=>{const row=element('button','record');const main=element('div','record-main');main.append(element('h3','',r.metadata.title),element('p','',r.metadata.tags.join(' · ')||'No tags yet'));row.append(element('span','kind',r.id),main,element('span','status',r.metadata.status));row.onclick=()=>showDetail(r);return row;}));
 if(!records.length)$('records').append(element('div','empty',snapshot.records.length?'No records match this view.':'Start with a question. Your research will take shape here.'));
 $('revision').textContent=`Canonical Markdown · corpus ${snapshot.revision?.slice(0,10)||'unknown'} · no execution implied`;
}
function showDetail(r){$('detail-id').textContent=`${r.id} / ${kinds[r.kind]}`;$('detail-title').textContent=r.metadata.title;$('detail-meta').textContent=`${r.metadata.status} · ${r.metadata.tags.join(' · ')}`;$('detail-body').textContent=r.body;$('detail-lineage').textContent=r.metadata.derived_from.length?`Derived from ${r.metadata.derived_from.join(', ')}`:'No predecessors';$('detail').showModal();}
document.querySelectorAll('[data-filter]').forEach(b=>b.onclick=()=>{filter=b.dataset.filter;document.querySelectorAll('[data-filter]').forEach(n=>n.classList.toggle('active',n===b));document.querySelector('h1').textContent=filter==='all'?'Overview':kinds[filter];render();});
$('search').oninput=render;
$('new-question').onclick=()=>{requestKey=crypto.randomUUID();$('capture-form').reset();$('capture-error').textContent='';$('capture').showModal();};
$('close-capture').onclick=()=>$('capture').close();$('close-detail').onclick=()=>$('detail').close();
$('capture-form').onsubmit=async e=>{e.preventDefault();const form=new FormData(e.target);const button=e.target.querySelector('[type=submit]');button.disabled=true;try{await api('/api/questions',{request_key:requestKey,title:form.get('title'),body:form.get('body'),tags:form.get('tags').split(',').map(t=>t.trim()).filter(Boolean)});$('capture').close();await refresh();}catch(error){$('capture-error').textContent=error.message;}finally{button.disabled=false;}};
(async()=>{try{token=(await api('/api/session')).token;await refresh();}catch(e){$('notice').hidden=false;$('notice').textContent=e.message;}})();
