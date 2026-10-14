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

def zoom(sel_js, S_=1.6):
    return ("(()=>{"+CS+"const w=Array.from(document.querySelectorAll('div')).find(d=>d.style.width==='1920px'&&d.style.position==='fixed');"
      "if(!w)return 'no wrap';if(!window.__orig)window.__orig=w.getAttribute('style');"
      "const t="+sel_js+";if(!t)return 'no target';"
      "const wr=w.getBoundingClientRect(),tr=t.getBoundingClientRect();const cur=wr.width/1920;"
      "const cx=((tr.x+tr.width/2)-wr.x)/cur, cy=((tr.y+tr.height/2)-wr.y)/cur;"
      "window.__z={cx,cy};w.style.transformOrigin='0 0';w.style.left='0px';w.style.top='0px';"
      "w.style.transform='translate('+(-(cx*"+str(S_)+"-800))+'px,'+(-(cy*"+str(S_)+"-500))+'px) scale("+str(S_)+")';"
      "return {cx,cy};})()")
ZOOM_PICKER = zoom("Array.from(document.querySelectorAll('div')).find(d=>d.style.width==='360px'&&/100%/.test(d.style.bottom))", 1.5)
ZOOM_CODE   = zoom("Array.from(document.querySelectorAll('[data-msgs]')[0].querySelectorAll('div')).find(d=>cs(d).borderRadius==='8px'&&/Copy/.test(d.textContent)&&d.children.length===2)", 1.7)
ZOOM_TOOLS  = zoom("Array.from(document.querySelectorAll('[data-msgs]')[0].querySelectorAll('div')).find(d=>cs(d).borderRadius==='8px'&&cs(d).overflow==='hidden'&&/read/.test(d.textContent))", 2.0)
ZOOM_IMG    = zoom("Array.from(document.querySelectorAll('div')).find(d=>d.style.maxWidth==='560px')", 1.7)
ZOOM_ATT    = zoom("document.querySelector('textarea').parentElement", 1.7)
ZOOM_CHIP   = zoom("Array.from(document.querySelectorAll('[data-msgs]')[0].querySelectorAll('div')).find(d=>cs(d).maxWidth==='240px')", 2.4)
ZRESET = "(()=>{const w=Array.from(document.querySelectorAll('div')).find(d=>d.style.width==='1920px'&&d.style.position==='fixed');w.setAttribute('style',window.__orig);return 'reset';})()"

# --- open picker
ctext("Opus 4.5", True); wait(350); shot("02-picker")
ev(zoom("Array.from(document.querySelectorAll('div')).find(d=>d.style.width==='360px'&&/100%/.test(d.style.bottom))",1.45)); wait(200); shot("03-picker-zoom"); ev(ZRESET)
# --- picker deep data
ev("(()=>{"+CS+"const p=Array.from(document.querySelectorAll('div')).find(d=>d.style.width==='360px'&&/100%/.test(d.style.bottom));const r=p.getBoundingClientRect();"
 "const rectTop=(e,label)=>{const b=e.getBoundingClientRect();return {label,w:Math.round(b.width/0.833333),h:Math.round(b.height/0.833333),x:Math.round(b.x)};};"
 "const labels=Array.from(p.children).map(c=>({tag:c.tagName,t:c.textContent.trim().slice(0,26),fs:cs(c).fontSize,ls:cs(c).letterSpacing,c:cs(c).color,pad:cs(c).padding,bd:cs(c).borderTop}));"
 "return {pop:{w:p.style.width,insetR:p.style.right,bottom:p.style.bottom,pad:cs(p).padding,radius:cs(p).borderRadius,bg:cs(p).backgroundColor,bd:cs(p).border,shadow:cs(p).boxShadow,gap:cs(p).gap,z:cs(p).zIndex,display:cs(p).display},"
 "topLevel:labels,"
 "modeGrid:(()=>{const g=p.children[2];const b=cs(g);return {cols:b.gridTemplateColumns,gap:b.gap,pad:b.padding};})(),"
 "modelList:(()=>{const l=p.querySelector('div[style*=\"max-height\"]');return {maxH:cs(l).maxHeight,overflow:cs(l).overflowY,gap:cs(l).gap,display:cs(l).display};})()};})()",3000)
# mode buttons detail
ev("(()=>{"+CS+"const p=Array.from(document.querySelectorAll('div')).find(d=>d.style.width==='360px'&&/100%/.test(d.style.bottom));"
 "const btns=Array.from(p.querySelectorAll('button')).filter(b=>/Auto|Plan|Ask/.test(b.textContent));"
 "return btns.map(b=>({t:b.textContent.replace(/\\s+/g,' ').trim(),title:b.title,pad:cs(b).padding,radius:cs(b).radius||cs(b).borderRadius,bd:cs(b).borderTop+' / '+cs(b).borderColor,bg:cs(b).backgroundColor,dir:cs(b).flexDirection,gap:cs(b).gap,"
 "titleSpan:{fs:cs(b.children[0]).fontSize,c:cs(b.children[0]).color,gap:cs(b.children[0]).gap,icon:b.children[0].querySelector('path').getAttribute('d'),iconW:b.children[0].querySelector('svg').getAttribute('width')},"
 "sub:{t:b.children[1].textContent,fs:cs(b.children[1]).fontSize,c:cs(b.children[1]).color}}));})()",2500)
# effort block detail
ev("(()=>{"+CS+"const p=Array.from(document.querySelectorAll('div')).find(d=>d.style.width==='360px'&&/100%/.test(d.style.bottom));"
 "const eff=Array.from(p.children).find(c=>/EFFORT/.test(c.textContent));"
 "const hdr=eff.children[0],track=eff.children[1],labels=eff.children[2],desc=eff.children[3];"
 "const ab=Array.from(track.children).filter(d=>cs(d).position==='absolute');"
 "const moons=Array.from(track.querySelectorAll('button'));"
 "return {hdr:{pad:cs(eff).padding,gap:cs(eff).gap,label:{t:hdr.children[0].textContent,fs:cs(hdr.children[0]).fontSize,ls:cs(hdr.children[0]).letterSpacing,c:cs(hdr.children[0]).color},"
 "name:{t:hdr.children[1].textContent,fs:cs(hdr.children[1]).fontSize,ff:cs(hdr.children[1]).fontFamily,c:cs(hdr.children[1]).color}},"
 "track:{grid:cs(track).gridTemplateColumns,h:cs(track).height,pos:cs(track).position,"
 "rail:ab[0]?{left:ab[0].style.left,right:ab[0].style.right,h:ab[0].style.height,bg:cs(ab[0]).backgroundColor,radius:cs(ab[0]).borderRadius}:null,"
 "fill:ab[1]?{left:ab[1].style.left,w:ab[1].style.width,bg:ab[1].style.background,shadow:ab[1].style.boxShadow,tr:ab[1].style.transition}:null},"
 "moons:moons.map(b=>({title:b.title,w:b.style.width,h:b.style.height,radius:cs(b).borderRadius,bg:cs(b).backgroundColor,bd:cs(b).borderColor,bdW:cs(b).borderWidth,c:cs(b).color,filter:cs(b).filter,"
 "svg:b.querySelector('svg').getAttribute('width')+'x'+b.querySelector('svg').getAttribute('height')+' vb='+b.querySelector('svg').getAttribute('viewBox'),"
 "circle:Array.from(b.querySelectorAll('circle')).map(c=>'r'+c.getAttribute('r')+' stroke '+c.getAttribute('stroke-width')+' opacity '+c.getAttribute('opacity')),"
 "moonPath:b.querySelector('path').getAttribute('d')})),"
 "labels:{grid:cs(labels).gridTemplateColumns,fs:cs(labels).fontSize,items:Array.from(labels.children).map(s=>({t:s.textContent,c:cs(s).color,fs:cs(s).fontSize}))},"
 "desc:{t:desc.textContent,fs:cs(desc).fontSize,c:cs(desc).color,align:cs(desc).textAlign,tw:cs(desc).textWrap}};})()",5000)
# model rows detail
ev("(()=>{"+CS+"const p=Array.from(document.querySelectorAll('div')).find(d=>d.style.width==='360px'&&/100%/.test(d.style.bottom));"
 "const list=p.querySelector('div[style*=\"max-height\"]');"
 "return Array.from(list.children).map(d=>({pad:cs(d).padding,radius:cs(d).borderRadius,bg:cs(d).backgroundColor,gap:cs(d).gap,align:cs(d).alignItems,"
 "name:{t:d.children[0].children[0].textContent,fs:cs(d.children[0].children[0]).fontSize,c:cs(d.children[0].children[0]).color},"
 "sub:{t:d.children[0].children[1].textContent,fs:cs(d.children[0].children[1]).fontSize,c:cs(d.children[0].children[1]).color},"
 "check:{t:d.children[1].textContent,c:cs(d.children[1]).color}}));})()",3000)
# --- select Plan mode
ctext("Steps first", True); wait(300); shot("04-picker-plan")
ev("(()=>{"+CS+"const p=Array.from(document.querySelectorAll('div')).find(d=>d.style.width==='360px'&&/100%/.test(d.style.bottom));const btns=Array.from(p.querySelectorAll('button')).filter(b=>/Auto|Plan|Ask/.test(b.textContent));return btns.map(b=>({t:b.textContent.replace(/\\s+/g,' ').trim().slice(0,12),bg:cs(b).backgroundColor,bd:cs(b).borderColor,color:cs(b).children[0].color,subColor:cs(b).children[1].color}));})()",2000)
# select Max effort moon
click('button[title="Max"]'); wait(400); shot("05-picker-effort-max")
ev("(()=>{"+CS+"const p=Array.from(document.querySelectorAll('div')).find(d=>d.style.width==='360px'&&/100%/.test(d.style.bottom));const eff=Array.from(p.children).find(c=>/EFFORT/.test(c.textContent));const ab=Array.from(eff.children[1].children).filter(d=>cs(d).position==='absolute');"
 "return {label:eff.children[0].children[0].textContent,name:eff.children[0].children[1].textContent,fillW:ab[1].style.width,desc:eff.children[3].textContent,moons:Array.from(eff.children[1].querySelectorAll('button')).map(b=>({t:b.title,c:cs(b).color,bd:cs(b).borderColor,filter:cs(b).filter}))};})()",2500)
# --- switch to GPT-5 (no effort)
ctext("GPT-5", True); wait(350); shot("06-picker-no-effort")
ev("(()=>{"+CS+"const p=Array.from(document.querySelectorAll('div')).find(d=>d.style.width==='360px'&&/100%/.test(d.style.bottom));"
 "const note=Array.from(p.children).find(c=>/no effort levels/.test(c.textContent));"
 "const ta=document.querySelector('textarea');const btn=ta.parentElement.children[1].children[2];"
 "return {note:note?{t:note.textContent,fs:cs(note).fontSize,c:cs(note).color,pad:cs(note).padding,bd:cs(note).borderTop,margin:cs(note).marginTop}:null,"
 "trigger:{t:btn.textContent.replace(/\\s+/g,' ').trim(),svgs:Array.from(btn.querySelectorAll('svg')).map(s=>s.getAttribute('width')+' '+Array.from(s.querySelectorAll('path')).map(x=>x.getAttribute('d')).join(''))},"
 "tileMeta:Array.from(document.querySelectorAll('[data-msgs]'))[0].previousElementSibling.textContent.replace(/\\s+/g,' ').trim()};})()",2500)
# back to Opus
ctext("Opus 4.5", True); wait(300)
# close picker by clicking trigger
ev("document.querySelector('textarea').parentElement.children[1].children[2].click()"); wait(300); shot("07-picker-closed")
# --- attach popover
click('button[title="Attach files"]'); wait(350); shot("08-attach-popover")
ev("(()=>{"+CS+"const p=document.querySelector('textarea').parentElement;const pop=Array.from(p.querySelectorAll('div')).find(d=>d.style.width==='300px');"
 "const r=pop.getBoundingClientRect();"
 "return {pop:{w:pop.style.width,left:pop.style.left,bottom:pop.style.bottom,pad:cs(pop).padding,radius:cs(pop).borderRadius,bg:cs(pop).backgroundColor,bd:cs(pop).border,shadow:cs(pop).boxShadow,gap:cs(pop).gap},"
 "head:{t:pop.children[0].textContent,fs:cs(pop.children[0]).fontSize,ls:cs(pop.children[0]).letterSpacing,c:cs(pop.children[0]).color,pad:cs(pop.children[0]).padding},"
 "opts:Array.from(pop.children).slice(1).map(d=>({pad:cs(d).padding,radius:cs(d).borderRadius,gap:cs(d).gap,cursor:cs(d).cursor,"
 "badge:(()=>{const b=d.querySelector('span');return b?{t:b.textContent,fs:cs(b).fontSize,fw:cs(b).fontWeight,bg:cs(b).backgroundColor,c:cs(b).color,w:cs(b).width,h:cs(b).height,radius:cs(b).borderRadius}:null;})(),"
 "name:d.children[d.children.length-2].textContent,kind:d.children[d.children.length-1].textContent}))};})()",4000)
ev(zoom("Array.from(document.querySelectorAll('div')).find(d=>d.style.width==='300px')",1.7)); wait(200); shot("09-attach-popover-zoom"); ev(ZRESET)
# --- add an attachment
ctext("loop.ts", True); wait(400); shot("10-composer-attachment")
ev("(()=>{"+CS+"const p=document.querySelector('textarea').parentElement;const holder=p.children[0];const card=holder.children[0];"
 "return {holder:{pad:cs(holder).padding,gap:cs(holder).gap,wrap:cs(holder).flexWrap,display:cs(holder).display},"
 "card:{pos:cs(card).position,pad:cs(card).padding,radius:cs(card).borderRadius,bg:cs(card).backgroundColor,bd:cs(card).border,gap:cs(card).gap,maxW:cs(card).maxWidth,align:cs(card).alignItems},"
 "badge:(()=>{const b=card.querySelector('span');return {t:b.textContent,fs:cs(b).fontSize,fw:cs(b).fontWeight,ls:cs(b).letterSpacing,bg:cs(b).backgroundColor,c:cs(b).color,w:cs(b).width,h:cs(b).height,radius:cs(b).borderRadius,display:cs(b).display};})(),"
 "texts:Array.from(card.querySelectorAll('span,div')).slice(0,6).map(x=>({t:x.textContent.trim().slice(0,30),fs:cs(x).fontSize,c:cs(x).color})),"
 "remove:(()=>{const b=card.querySelector('button');return {t:b.textContent,title:b.title,pos:cs(b).position,top:b.style.top,right:b.style.right,w:b.style.width,h:b.style.height,radius:cs(b).borderRadius,bg:cs(b).backgroundColor,bd:cs(b).border,fs:cs(b).fontSize,lh:cs(b).lineHeight};})(),"
 "attachBtnBg:cs(p.children[1].children[0]).backgroundColor};})()",3000)
ev(zoom("document.querySelector('textarea').parentElement",1.6)); wait(200); shot("11-composer-attachment-zoom"); ev(ZRESET)

json.dump(S,open('/home/archonminipc/ArchonUI/.qa/design/chat-steps5.json','w'))
print(len(S))
