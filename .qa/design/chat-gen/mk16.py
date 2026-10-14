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
click('button[title="Attach files"]'); wait(400); ctext("loop.ts",True); wait(300)
click('button[title="Attach files"]'); wait(400); ctext("browser screenshot.png",True); wait(400); shot("38-composer-two-attachments")
ev("(()=>{"+CS+"const h=document.querySelector('textarea').parentElement.children[0];"
 "return {holder:{gap:cs(h).gap,wrap:cs(h).flexWrap,pad:cs(h).padding,n:h.children.length},"
 "cards:Array.from(h.children).map(c=>({bd:cs(c).border,bg:cs(c).backgroundColor,pad:cs(c).padding,radius:cs(c).borderRadius,gap:cs(c).gap,maxW:cs(c).maxWidth,"
 "kids:Array.from(c.children).map(k=>({tag:k.tagName,style:(k.getAttribute('style')||'').slice(0,70),t:k.textContent.replace(/\\s+/g,' ').trim().slice(0,30)}))}))};})()",3000)
click("textarea"); typ("Ship it"); wait(200); key("Enter"); wait(1200); shot("39-sent-with-attachments")
ev("(()=>{"+CS+"const msgs=document.querySelectorAll('[data-msgs]')[0];const list=Array.from(msgs.children);"
 "const user=list.filter(d=>cs(d).alignSelf==='flex-end');const u=user[user.length-1];"
 "const row=Array.from(u.children).find(c=>cs(c).justifyContent==='flex-end');"
 "return {present:!!u,n:user.length,"
 "row:row?{gap:cs(row).gap,justify:cs(row).justifyContent,wrap:cs(row).flexWrap,n:row.children.length}:null,"
 "cards:row?Array.from(row.children).map(c=>({bd:cs(c).border,bg:cs(c).backgroundColor,pad:cs(c).padding,radius:cs(c).borderRadius,gap:cs(c).gap,maxW:cs(c).maxWidth,"
 "kids:Array.from(c.children).map(k=>({tag:k.tagName,style:(k.getAttribute('style')||'').slice(0,60),t:k.textContent.replace(/\\s+/g,' ').trim().slice(0,30)}))})):[]};})()",3500)
wait(2500)
json.dump(S,open('/home/archonminipc/ArchonUI/.qa/design/chat-steps16.json','w'))
print(len(S))
