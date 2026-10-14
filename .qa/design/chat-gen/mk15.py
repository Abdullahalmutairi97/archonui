import json
S=[]
def ev(js,mx=4000): S.append(["eval",js,mx])
def shot(n): S.append(["shot",n])
def click(sel): S.append(["click",sel])
def ctext(t,exact=True): S.append(["clickText",t,exact])
def key(k): S.append(["key",k])
def typ(t): S.append(["type",t])
def wait(ms): S.append(["wait",ms])
CS="const cs=e=>getComputedStyle(e);"
def SEND(): return "const bar=document.querySelector('textarea').parentElement.children[1];const s=bar.children[bar.children.length-1];"
# copy code toast
ctext("Copy",True); wait(500); shot("34-toast-copy-code")
ev("(()=>{"+CS+"const t=Array.from(document.querySelectorAll('div')).find(d=>/^Copied /.test(d.textContent)&&d.children.length===0);return t?{t:t.textContent,fs:cs(t).fs||cs(t).fontSize,pad:cs(t).padding,radius:cs(t).borderRadius,bg:cs(t).backgroundColor,bd:cs(t).border,shadow:cs(t).boxShadow,left:cs(t).left,bottom:cs(t).bottom,pos:cs(t).position}:{t:null,all:Array.from(document.querySelectorAll('div')).map(d=>d.textContent.slice(0,20)).filter(x=>/Copied/.test(x)).slice(0,5)};})()",1200)
wait(1500)
# two attachments then send
click('button[title="Attach files"]'); wait(350); ctext("session.ts",True); wait(300)
click('button[title="Attach files"]'); wait(350); ctext("cache.ts",True); wait(400)
ev("(()=>{const h=document.querySelector('textarea').parentElement.children[0];return {n:h.children.length,kids:Array.from(h.children).map(c=>c.textContent.replace(/\\s+/g,' ').trim().slice(0,30)),gap:getComputedStyle(h).gap,wrap:getComputedStyle(h).flexWrap,pad:getComputedStyle(h).padding};})()",1200)
click("textarea"); typ("Ship it"); key("Enter"); wait(900); shot("35-sent-with-attachments")
ev("(()=>{"+CS+"const msgs=document.querySelectorAll('[data-msgs]')[0];const last=msgs.children[msgs.children.length-2];const wrap=last.children[0];"
 "return {msgWrap:{alignSelf:cs(wrap).alignSelf,alignItems:cs(wrap).alignItems,gap:cs(wrap).gap,maxW:cs(wrap).maxWidth},"
 "attRow:{gap:cs(wrap.children[0]).gap,justify:cs(wrap.children[0]).justifyContent,wrap:cs(wrap.children[0]).flexWrap,n:wrap.children[0].children.length},"
 "cards:Array.from(wrap.children[0].children).map(c=>({bd:cs(c).border,bg:cs(c).backgroundColor,pad:cs(c).padding,radius:cs(c).borderRadius,gap:cs(c).gap,maxW:cs(c).maxWidth,"
 "badge:(()=>{const b=Array.from(c.querySelectorAll('span')).find(s=>s.style.width==='30px');return b?{t:b.textContent,fs:cs(b).fontSize,bg:cs(b).backgroundColor,c:cs(b).color,w:b.style.width,h:b.style.height,radius:cs(b).borderRadius}:null;})(),"
 "name:(Array.from(c.querySelectorAll('span')).filter(s=>!s.classList.contains('sc-interp'))[1]||{}).textContent,meta:(Array.from(c.querySelectorAll('span')).filter(s=>!s.classList.contains('sc-interp'))[2]||{}).textContent})),"
 "hasRemove:!!wrap.querySelector('button')};})()",3500)
wait(2200)
# stop test
click("textarea"); typ("Stop me"); key("Enter"); wait(800); shot("36-running-mid")
click("button[title='Stop']"); wait(500); shot("37-stopped")
ev("(()=>{"+CS+SEND()+"const msgs=document.querySelectorAll('[data-msgs]')[0];const last=msgs.children[msgs.children.length-1];"
 "return {txt:last.textContent.replace(/\\s+/g,' ').trim().slice(0,160),sendTitle:s.title,bg:cs(s).backgroundColor,tileMeta:msgs.previousElementSibling.textContent.replace(/\\s+/g,' ').trim()};})()",2500)
json.dump(S,open('/home/archonminipc/ArchonUI/.qa/design/chat-steps15.json','w'))
print(len(S))
