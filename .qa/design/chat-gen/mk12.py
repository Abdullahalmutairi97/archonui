import json
S=[]
def ev(js,mx=4000): S.append(["eval",js,mx])
def shot(n): S.append(["shot",n])
def click(sel): S.append(["click",sel])
def ctext(t,exact=True): S.append(["clickText",t,exact])
def key(k): S.append(["key",k])
def typ(t): S.append(["type",t])
def wait(ms): S.append(["wait",ms])
def xy(x,y): S.append(["clickXY",x,y])
CS="const cs=e=>getComputedStyle(e);"
PICKER="Array.from(document.querySelectorAll('div')).find(d=>d.style.width==='360px'&&/100%/.test(d.style.bottom))"
ctext("Settings",True); wait(800); ctext("Models",True); wait(600)
xy(1292,954); wait(400)
ev("(()=>{"+CS+"const rows=Array.from(document.querySelectorAll('div')).filter(d=>(d.getAttribute('style')||'').includes('grid-template-columns'));const row=rows.find(d=>/llama3\\.3 70B/.test(d.textContent));const tog=row.children[row.children.length-1];return {track:cs(tog).backgroundColor,knob:cs(tog.children[0]).backgroundColor,x:tog.children[0].style.left,op:cs(row).opacity};})()",1000)
key("Escape"); wait(500)
click('button[title="Attach files"]'); wait(400); ctext("browser screenshot.png",True); wait(500); shot("30-composer-image-attachment")
ev("(()=>{"+CS+"const p=document.querySelector('textarea').parentElement;const holder=p.children[0];const card=holder.children[0];"
 "const sp=Array.from(card.children);"
 "return {card:{bd:cs(card).border,bg:cs(card).backgroundColor,pad:cs(card).padding,radius:cs(card).borderRadius,gap:cs(card).gap,maxW:cs(card).maxWidth},"
 "thumb:sp[0]?(sp[0].getAttribute('style')||'').slice(0,120):null,thumbCss:sp[0]?{w:cs(sp[0]).width,h:cs(sp[0]).height,radius:cs(sp[0]).borderRadius,bg:cs(sp[0]).backgroundImage.slice(0,80)}:null,"
 "texts:Array.from(card.querySelectorAll('span')).filter(s=>!s.classList.contains('sc-interp')).map(s=>({t:s.textContent,fs:cs(s).fontSize,c:cs(s).color,ov:cs(s).textOverflow}))};})()",2500)
ev("(()=>{document.querySelector('textarea').parentElement.querySelector('button[title=\"Remove\"]').click();return 'removed';})()",100)
wait(300)
click('button[title="Attach files"]'); wait(400); ctext("session.ts",True); wait(400)
# open picker and switch to llama
click('button[title=""]'); wait(300)
ev("(()=>{"+CS+"const p="+PICKER+";if(!p)return 'no picker';const rows=Array.from(p.children);const last=rows[rows.length-1];"
 "const ta=document.querySelector('textarea');const btn=ta.parentElement.children[1].children[2];"
 "return {trigger:btn.textContent.replace(/\\s+/g,' ').trim(),svgs:Array.from(btn.querySelectorAll('svg')).length,"
 "lastBlock:last.textContent.slice(0,60),moonsInTrigger:Array.from(btn.querySelectorAll('svg')).map(s=>s.getAttribute('viewBox'))};})()",2000)
ctext("llama3.3 70B",True); wait(500); shot("31-picker-no-effort")
ev("(()=>{"+CS+"const p="+PICKER+";const note=Array.from(p.children).find(c=>/no effort levels/.test(c.textContent));"
 "const ta=document.querySelector('textarea');const btn=ta.parentElement.children[1].children[2];"
 "return {hasEffortBlock:!!Array.from(p.children).find(c=>/EFFORT ·/.test(c.textContent)),note:note?{t:note.textContent,fs:cs(note).fontSize,c:cs(note).color,pad:cs(note).padding,bd:cs(note).borderTop,mt:cs(note).marginTop,display:cs(note).display}:null,"
 "trigger:{t:btn.textContent.replace(/\\s+/g,' ').trim(),svgCount:btn.querySelectorAll('svg').length},tileMeta:Array.from(document.querySelectorAll('[data-msgs]'))[0].previousElementSibling.textContent.replace(/\\s+/g,' ').trim()};})()",2500)
json.dump(S,open('/home/archonminipc/ArchonUI/.qa/design/chat-steps12.json','w'))
print(len(S))
