import json
S=[]
def ev(js,mx=4000): S.append(["eval",js,mx])
def shot(n): S.append(["shot",n])
def click(sel): S.append(["click",sel])
def ctext(t,exact=True): S.append(["clickText",t,exact])
def key(k): S.append(["key",k])
def wait(ms): S.append(["wait",ms])
CS="const cs=e=>getComputedStyle(e);"
ctext("Settings",True); wait(800); ctext("Models",True); wait(600)
ev("(()=>{const all=Array.from(document.querySelectorAll('div'));const hits=all.filter(d=>/llama3\\.3 70B/.test(d.textContent));"
 "return {n:hits.length,sizes:hits.map(d=>({cls:d.className,style:(d.getAttribute('style')||'').slice(0,120),kids:d.children.length,w:Math.round(d.getBoundingClientRect().width),h:Math.round(d.getBoundingClientRect().height)}))};})()",3000)
ev("(()=>{"+CS+"const all=Array.from(document.querySelectorAll('div'));const row=all.filter(d=>/llama3\\.3 70B/.test(d.textContent)).pop();"
 "if(!row)return 'no row';const tog=Array.from(row.querySelectorAll('span')).find(s=>(s.getAttribute('style')||'').includes('width:34px'));"
 "const r=(tog||row).getBoundingClientRect();return {rowStyle:(row.getAttribute('style')||'').slice(0,140),tog:!!tog,x:Math.round(r.x+r.width/2),y:Math.round(r.y+r.height/2),rect:[Math.round(r.x),Math.round(r.y),Math.round(r.width),Math.round(r.height)]};})()",2000)
json.dump(S,open('/home/archonminipc/ArchonUI/.qa/design/chat-steps10.json','w'))
print(len(S))
