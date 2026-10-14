import json
S=[]
def ev(js,mx=4000): S.append(["eval",js,mx])
def shot(n): S.append(["shot",n])
def click(sel): S.append(["click",sel])
def ctext(t,exact=True): S.append(["clickText",t,exact])
def key(k): S.append(["key",k])
def wait(ms): S.append(["wait",ms])
CS="const cs=e=>getComputedStyle(e);"
PICKER="Array.from(document.querySelectorAll('div')).find(d=>d.style.width==='360px'&&/100%/.test(d.style.bottom))"
CLICK_LLAMA=("(()=>{"+CS+"const el=Array.from(document.querySelectorAll('div')).find(d=>(d.getAttribute('style')||'').includes('overflow: auto; min-height: 0px'));el.scrollTop=el.scrollHeight;"
 "const rows=Array.from(document.querySelectorAll('div')).filter(d=>(d.getAttribute('style')||'').includes('grid-template-columns'));"
 "const row=rows.find(d=>/llama3\\.3 70B/.test(d.textContent));const tog=row.children[row.children.length-1];const before=cs(tog).backgroundColor;tog.click();"
 "return {scrollTop:el.scrollTop,before,togStyle:(tog.getAttribute('style')||'').slice(0,120)};})()")
ctext("Settings",True); wait(800); ctext("Models",True); wait(600)
ev(CLICK_LLAMA,1200); wait(500)
ev("(()=>{"+CS+"const rows=Array.from(document.querySelectorAll('div')).filter(d=>(d.getAttribute('style')||'').includes('grid-template-columns'));const row=rows.find(d=>/llama3\\.3 70B/.test(d.textContent));const tog=row.children[row.children.length-1];return {track:cs(tog).backgroundColor,knob:cs(tog.children[0]).backgroundColor,left:tog.children[0].style.left,knobColor:cs(tog.children[0]).backgroundColor};})()",1000)
shot("32-models-llama-on")
key("Escape"); wait(600)
ctext("Opus 4.5",True); wait(400)
ev("(()=>{"+CS+"const p="+PICKER+";if(!p)return 'no picker';const list=Array.from(p.children).find(c=>/Claude Opus 4.5/.test(c.textContent));"
 "return {models:Array.from(list.querySelectorAll('div[style*=\"cursor\"]')).map(d=>d.textContent.replace(/\\s+/g,' ').trim()),"
 "rowCount:Array.from(p.querySelectorAll('div')).filter(d=>cs(d).cursor==='pointer'&&/·/.test(d.textContent)).length};})()",2000)
ctext("llama3.3 70B",True); wait(600); shot("33-picker-no-effort")
ev("(()=>{"+CS+"const p="+PICKER+";const note=Array.from(p.children).find(c=>/no effort levels/.test(c.textContent));"
 "const ta=document.querySelector('textarea');const btn=ta.parentElement.children[1].children[2];"
 "return {hasEffortBlock:!!Array.from(p.children).find(c=>/EFFORT ·/.test(c.textContent)),"
 "note:note?{t:note.textContent,fs:cs(note).fontSize,c:cs(note).color,pad:cs(note).padding,bd:cs(note).borderTop,mt:cs(note).marginTop,display:cs(note).display}:null,"
 "trigger:{t:btn.textContent.replace(/\\s+/g,' ').trim(),svgCount:btn.querySelectorAll('svg').length},tileMeta:Array.from(document.querySelectorAll('[data-msgs]'))[0].previousElementSibling.textContent.replace(/\\s+/g,' ').trim()};})()",2500)
json.dump(S,open('/home/archonminipc/ArchonUI/.qa/design/chat-steps14.json','w'))
print(len(S))
