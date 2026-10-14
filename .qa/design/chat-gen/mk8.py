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
ev("(()=>{return {tiles:Array.from(document.querySelectorAll('[data-msgs]')).map((m,i)=>({i,id:m.getAttribute('data-msgs'),textareas:m.parentElement.parentElement.querySelectorAll('textarea').length}))};})()",1500)
click('button[title="Attach files"]'); wait(400)
ctext("loop.ts", True); wait(600); shot("10-composer-attachment")
ev("(()=>{"+CS+"const tas=Array.from(document.querySelectorAll('textarea'));"
 "return {n:tas.length,tas:tas.map(ta=>{const p=ta.parentElement;return {ph:ta.placeholder.slice(0,20),kids:Array.from(p.children).map(c=>c.tagName+'|'+(c.textContent||'').trim().slice(0,40))};})};})()",3000)
ev("(()=>{"+CS+"const ta=document.querySelector('textarea');const p=ta.parentElement;const holder=p.children[0];if(!holder||holder.tagName==='TEXTAREA')return 'no atts holder';const card=holder.children[0];const badges=Array.from(card.querySelectorAll('span'));const rb=card.querySelector('button');"
 "const o={holder:{pad:cs(holder).padding,gap:cs(holder).gap,wrap:cs(holder).flexWrap,display:cs(holder).display},"
 "card:{pos:cs(card).position,pad:cs(card).padding,radius:cs(card).borderRadius,bg:cs(card).backgroundColor,bd:cs(card).border,gap:cs(card).gap,maxW:cs(card).maxWidth,align:cs(card).alignItems,display:cs(card).display},"
 "badge:{t:badges[0].textContent,fs:cs(badges[0]).fontSize,fw:cs(badges[0]).fontWeight,ls:cs(badges[0]).letterSpacing,bg:cs(badges[0]).backgroundColor,c:cs(badges[0]).color,w:cs(badges[0]).width,h:cs(badges[0]).height,radius:cs(badges[0]).borderRadius,display:cs(badges[0]).display,place:cs(badges[0]).placeItems},"
 "name:{t:badges[1].textContent,fs:cs(badges[1]).fontSize,c:cs(badges[1]).color},meta:{t:badges[2].textContent,fs:cs(badges[2]).fontSize,c:cs(badges[2]).color},"
 "remove:{t:rb.textContent,title:rb.title,pos:cs(rb).position,top:rb.style.top,right:rb.style.right,w:rb.style.width,h:rb.style.height,radius:cs(rb).borderRadius,bg:cs(rb).backgroundColor,bd:cs(rb).border,fs:cs(rb).fontSize,lh:cs(rb).lineHeight,c:cs(rb).color}};return o;})()",3500)
ev("(()=>{document.querySelector('textarea').parentElement.querySelector('button[title=\"Remove\"]').click();return 'removed';})()",200)
wait(400); shot("11-attachment-removed")
json.dump(S,open('/home/archonminipc/ArchonUI/.qa/design/chat-steps8.json','w'))
print(len(S))
