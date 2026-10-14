import json
S=[]
def ev(js,mx=4000): S.append(["eval",js,mx])
def ctext(t,exact=True): S.append(["clickText",t,exact])
def wait(ms): S.append(["wait",ms])
CS = "const cs=e=>getComputedStyle(e);"
JS = ("(()=>{" + CS + "const rows=Array.from(document.querySelectorAll('div')).filter(d=>(d.getAttribute('style')||'').includes('grid-template-columns'));"
 "const row=rows.find(d=>/llama3\\.3 70B/.test(d.textContent));"
 "const tog=row.children[row.children.length-1];const r=tog.getBoundingClientRect();"
 "return {rowText:row.textContent.replace(/\\s+/g,' ').trim(),toggle:(tog.getAttribute('style')||'').slice(0,90),x:Math.round(r.x+r.width/2),y:Math.round(r.y+r.height/2),rect:[Math.round(r.x),Math.round(r.y),Math.round(r.width),Math.round(r.height)]};})()")
ctext("Settings",True); wait(800); ctext("Models",True); wait(600)
ev(JS,2000)
json.dump(S,open('/home/archonminipc/ArchonUI/.qa/design/chat-steps11.json','w'))
print(len(S))
