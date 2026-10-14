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

# chart bars (fixed selector)
ev("(()=>{"+CS+"const btn=Array.from(document.querySelectorAll('button')).find(b=>b.textContent==='Copy CSV');const frame=btn.parentElement.parentElement;const body=frame.children[1];const plot=body.children[0];"
 "const bars=Array.from(plot.children).filter(d=>d.style.position==='relative');"
 "return bars.map(b=>({v:b.children[0].textContent,vc:cs(b.children[0]).color,vcFs:cs(b.children[0]).fontSize,h:b.children[1].style.height,bg:b.children[1].style.background,barW:Math.round(b.children[1].getBoundingClientRect().width),maxW:cs(b.children[1]).maxWidth,radius:cs(b.children[1]).borderRadius,rootH:Math.round(b.getBoundingClientRect().height/0.833333),gap:cs(b).gap}));})()",3000)
# Copy CSV -> toast
click('button:has-text("Copy CSV")'); wait(400); shot("12-toast-copy")
ev("(()=>{const t=Array.from(document.querySelectorAll('div')).find(d=>d.textContent==='Copied CSV');if(!t)return 'no toast';const cs=getComputedStyle(t);const r=t.getBoundingClientRect();return {t:t.textContent,left:cs.left,bottom:cs.bottom,transform:cs.transform,pad:cs.padding,radius:cs.borderRadius,bg:cs.backgroundColor,bd:cs.border,fs:cs.fontSize,shadow:cs.boxShadow,pos:cs.position,z:cs.zIndex};})()",1500)
wait(1600)
# lightbox
ctext("Open",True); wait(400); shot("13-lightbox")
ev("(()=>{"+CS+"const lb=Array.from(document.querySelectorAll('div')).find(d=>cs(d).zIndex==='30');const inner=lb.children[0];const cap=lb.children[1];"
 "return {overlay:{bg:cs(lb).backgroundColor,display:cs(lb).display,gap:cs(lb).gap,cursor:cs(lb).cursor,align:cs(lb).alignItems,justify:cs(lb).justifyContent,pos:cs(lb).position},"
 "img:{w:inner.style.width,h:inner.style.height,radius:cs(inner).borderRadius,bg:cs(inner).backgroundImage.slice(0,90),fit:cs(inner).backgroundSize},"
 "cap:{t:cap.textContent.trim(),fs:cs(cap).fontSize,c:cs(cap).color,muted:cs(cap.children[0]).color}};})()",2000)
key("Escape"); wait(300); shot("14-lightbox-closed")
# attach popover + card
click('button[title="Attach files"]'); wait(350); shot("09-attach-popover")
ev("(()=>{"+CS+"const p=document.querySelector('textarea').parentElement;const pop=Array.from(p.querySelectorAll('div')).find(d=>d.style.width==='300px');"
 "return {pop:{w:pop.style.width,left:pop.style.left,bottom:pop.style.bottom,pad:cs(pop).padding,radius:cs(pop).borderRadius,bg:cs(pop).backgroundColor,bd:cs(pop).border,shadow:cs(pop).boxShadow,gap:cs(pop).gap,display:cs(pop).display},"
 "head:{t:pop.children[0].textContent,fs:cs(pop.children[0]).fontSize,ls:cs(pop.children[0]).letterSpacing,c:cs(pop.children[0]).color,pad:cs(pop.children[0]).padding},"
 "opts:Array.from(pop.children).slice(1).map(d=>{const sp=Array.from(d.children);const badge=sp.find(x=>x.tagName==='SPAN');const txt=sp.filter(x=>x.tagName==='SPAN'&&x!==badge);"
 "return {pad:cs(d).padding,radius:cs(d).borderRadius,gap:cs(d).gap,cursor:cs(d).cursor,badge:badge?{t:badge.textContent,fs:cs(badge).fontSize,fw:cs(badge).fontWeight,bg:cs(badge).backgroundColor,c:cs(badge).color,w:cs(badge).width,h:cs(badge).height,radius:cs(badge).borderRadius}:null,"
 "name:(txt[0]||{}).textContent,fs:cs(txt[0]||d).fontSize,kind:(txt[1]||{}).textContent,kindC:txt[1]?cs(txt[1]).color:null,kindFs:txt[1]?cs(txt[1]).fontSize:null};})};})()",4000)
ctext("loop.ts", True); wait(500); shot("10-composer-attachment")
ev("(()=>{"+CS+"const p=document.querySelector('textarea').parentElement;const holder=p.children[0];const card=holder.children[0];const badges=Array.from(card.querySelectorAll('span'));const b=badges[0];const rb=card.querySelector('button');"
 "return {holder:{pad:cs(holder).padding,gap:cs(holder).gap,wrap:cs(holder).flexWrap,display:cs(holder).display},"
 "card:{pos:cs(card).position,pad:cs(card).padding,radius:cs(card).borderRadius,bg:cs(card).backgroundColor,bd:cs(card).border,gap:cs(card).gap,maxW:cs(card).maxWidth,align:cs(card).alignItems,display:cs(card).display},"
 "badge:{t:b.textContent,fs:cs(b).fontSize,fw:cs(b).fontWeight,ls:cs(b).letterSpacing,bg:cs(b).backgroundColor,c:cs(b).color,w:cs(b).width,h:cs(b).height,radius:cs(b).borderRadius,display:cs(b).display,place:cs(b).placeItems},"
 "name:{t:badges[1].textContent,fs:cs(badges[1]).fontSize,c:cs(badges[1]).color},meta:{t:badges[2].textContent,fs:cs(badges[2]).fontSize,c:cs(badges[2]).color},"
 "remove:{t:rb.textContent,title:rb.title,pos:cs(rb).position,top:rb.style.top,right:rb.style.right,w:rb.style.width,h:rb.style.height,radius:cs(rb).borderRadius,bg:cs(rb).backgroundColor,bd:cs(rb).border,fs:cs(rb).fontSize,lh:cs(rb).lineHeight,c:cs(rb).color},"
 "attachBtnBg:cs(p.children[1].children[0]).backgroundColor};})()",3500)
# send button with draft text
click("textarea"); typ("add a metric for the cache"); wait(300); shot("11-composer-draft")
ev("(()=>{"+CS+"const p=document.querySelector('textarea').parentElement;const bar=p.children[1];const s=bar.children[bar.children.length-1];return {draft:p.children[1].children[0].value,bg:cs(s).backgroundColor,color:cs(s).color,title:s.title};})()",1200)
# Shift+Enter newline + Shift+Tab mode cycle
key("Shift+Enter"); typ("second line"); wait(200)
ev("(()=>{const t=document.querySelector('textarea');return {value:t.value,ph:t.placeholder};})()",800)
key("Shift+Tab"); wait(300); shot("15-mode-plan-placeholder")
ev("(()=>{const t=document.querySelector('textarea');const p=t.parentElement;const btn=p.children[1].children[2];return {mode:p.placeholder,ph:t.placeholder,trigger:btn.textContent.replace(/\\s+/g,' ').trim(),modeIcon:Array.from(btn.querySelectorAll('svg'))[0].querySelector('path').getAttribute('d')};})()",1200)
key("Shift+Tab"); wait(300); shot("16-mode-ask-placeholder")
ev("(()=>{const t=document.querySelector('textarea');const p=t.parentElement;const btn=p.children[1].children[2];return {ph:t.placeholder,trigger:btn.textContent.replace(/\\s+/g,' ').trim(),modeIcon:Array.from(btn.querySelectorAll('svg'))[0].querySelector('path').getAttribute('d')};})()",1200)
key("Shift+Tab"); wait(300)
ev("(()=>{const t=document.querySelector('textarea');return {ph:t.placeholder};})()",800)
# send with Enter -> running state
key("Enter"); wait(250); shot("17-running-thinking")
ev("(()=>{"+CS+"const msgs=document.querySelectorAll('[data-msgs]')[0];const last=msgs.children[msgs.children.length-1];const p=document.querySelector('textarea').parentElement;const bar=p.children[1];const s=bar.children[bar.children.length-1];"
 "return {lastMsgText:last.textContent.replace(/\\s+/g,' ').trim().slice(0,120),working:Array.from(last.querySelectorAll('span')).filter(x=>/Working/.test(x.textContent)).map(x=>x.textContent),"
 "stop:{title:s.title,bg:cs(s).backgroundColor,color:cs(s).color,square:Array.from(s.querySelectorAll('span')).map(x=>x.style.width+'x'+x.style.height+' r'+x.style.borderRadius+' '+cs(x).backgroundColor)},"
 "tileMeta:msgs.previousElementSibling.textContent.replace(/\\s+/g,' ').trim(),draft:p.children[1].children[0].value,attsHolder:p.children[0]?p.children[0].children.length:0};})()",2000)
wait(1300); shot("18-running-toolcalls")
ev("(()=>{"+CS+"const msgs=document.querySelectorAll('[data-msgs]')[0];const last=msgs.children[msgs.children.length-1];return {txt:last.textContent.replace(/\\s+/g,' ').trim().slice(0,200)};})()",1000)
# stop
click('button[title="Stop"]'); wait(400); shot("19-stopped")
ev("(()=>{"+CS+"const msgs=document.querySelectorAll('[data-msgs]')[0];const last=msgs.children[msgs.children.length-1];const p=document.querySelector('textarea').parentElement;const bar=p.children[1];const s=bar.children[bar.children.length-1];"
 "return {txt:last.textContent.replace(/\\s+/g,' ').trim().slice(0,200),sendTitle:s.title,bg:cs(s).backgroundColor,tileMeta:msgs.previousElementSibling.textContent.replace(/\\s+/g,' ').trim()};})()",2000)
json.dump(S,open('/home/archonminipc/ArchonUI/.qa/design/chat-steps7.json','w'))
print(len(S))
