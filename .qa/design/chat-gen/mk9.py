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

# code block Copy -> toast
ctext("Copy",True); wait(500); shot("20-toast-copy-code")
ev("(()=>{const t=Array.from(document.querySelectorAll('div')).find(d=>/^Copied /.test(d.textContent)&&d.children.length===0);if(!t)return 'none';return {t:t.textContent,fs:cs(t).fontSize};})()",800)
wait(1500)
# composer draft
click("textarea"); typ("Add a metric for the cache hit rate"); wait(300); shot("21-composer-draft")
ev("(()=>{"+CS+SEND()+"return {draft:document.querySelector('textarea').value,send:{title:s.title,bg:cs(s).backgroundColor,color:cs(s).color,icon:Array.from(s.querySelectorAll('svg path')).map(x=>x.getAttribute('d'))}};})()",1200)
# multiline
key("Shift+Enter"); typ("and a test"); wait(250); shot("22-composer-multiline")
ev("(()=>{const t=document.querySelector('textarea');const r=t.getBoundingClientRect();return {value:t.value,h:Math.round(r.height/0.833333),scrollH:t.scrollHeight};})()",1000)
# mode cycling
key("Shift+Tab"); wait(350); shot("23-mode-plan")
ev("(()=>{"+CS+"const t=document.querySelector('textarea');const btn=t.parentElement.children[1].children[2];return {ph:t.placeholder,trigger:btn.textContent.replace(/\\s+/g,' ').trim(),icon:Array.from(btn.querySelectorAll('svg'))[0].querySelector('path').getAttribute('d'),iconColor:cs(btn.children[0]).color};})()",1500)
key("Shift+Tab"); wait(350); shot("24-mode-ask")
ev("(()=>{"+CS+"const t=document.querySelector('textarea');const btn=t.parentElement.children[1].children[2];return {ph:t.placeholder,trigger:btn.textContent.replace(/\\s+/g,' ').trim(),icon:Array.from(btn.querySelectorAll('svg'))[0].querySelector('path').getAttribute('d'),iconColor:cs(btn.children[0]).color};})()",1500)
key("Shift+Tab"); wait(350)
ev("(()=>{const t=document.querySelector('textarea');return {ph:t.placeholder,valueKept:t.value.length};})()",800)
# send -> running
key("Enter"); wait(260); shot("25-running-thinking")
ev("(()=>{"+CS+SEND()+"const msgs=document.querySelectorAll('[data-msgs]')[0];const last=msgs.children[msgs.children.length-1];"
 "return {draftAfterSend:document.querySelector('textarea').value,working:Array.from(last.querySelectorAll('span')).filter(x=>/Working/.test(x.textContent)).map(x=>({t:x.textContent,fs:cs(x).fontSize,it:cs(x).fontStyle,c:cs(x).color})),"
 "stop:{title:s.title,bg:cs(s).backgroundColor,color:cs(s).color,icon:Array.from(s.querySelectorAll('span')).map(x=>x.style.width+'x'+x.style.height+' r'+x.style.borderRadius+' '+cs(x).backgroundColor)},tileMeta:msgs.previousElementSibling.textContent.replace(/\\s+/g,' ').trim()};})()",2500)
wait(1000); shot("26-running-toolcalls")
ev("(()=>{"+CS+"const msgs=document.querySelectorAll('[data-msgs]')[0];const last=msgs.children[msgs.children.length-1];return {txt:last.textContent.replace(/\\s+/g,' ').trim().slice(0,240),rows:Array.from(last.querySelectorAll('div')).filter(d=>cs(d).fontSize==='12px').length};})()",1200)
click("button[title='Stop']"); wait(400); shot("27-stopped")
ev("(()=>{"+CS+SEND()+"const msgs=document.querySelectorAll('[data-msgs]')[0];const last=msgs.children[msgs.children.length-1];"
 "return {txt:last.textContent.replace(/\\s+/g,' ').trim().slice(0,240),sendTitle:s.title,sendBg:cs(s).backgroundColor,wmIcon:Array.from(s.querySelectorAll('svg path')).map(x=>x.getAttribute('d')),tileMeta:msgs.previousElementSibling.textContent.replace(/\\s+/g,' ').trim()};})()",2500)
# settings -> models -> enable llama -> composer picker no-effort
ctext("Settings",True); wait(700); shot("28-settings-models")
ctext("Models",True); wait(500); shot("29-settings-models-tab")
ev("(()=>{const rows=Array.from(document.querySelectorAll('div')).filter(d=>(d.getAttribute('style')||'').includes('grid-template-columns:48px minmax'));return rows.map(r=>r.textContent.replace(/\\s+/g,' ').trim().slice(0,50));})()",1500)
json.dump(S,open('/home/archonminipc/ArchonUI/.qa/design/chat-steps9.json','w'))
print(len(S))
