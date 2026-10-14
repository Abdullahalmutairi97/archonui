import json
S = []
def ev(js, mx=4000): S.append(["eval", js, mx])
def shot(n): S.append(["shot", n])
def click(sel): S.append(["click", sel])
def ctext(t, exact=True): S.append(["clickText", t, exact])
def key(k): S.append(["key", k])
def typ(t): S.append(["type", t])
def wait(ms): S.append(["wait", ms])

CS = "const cs=e=>getComputedStyle(e);"

# 1 user bubble + agent block
ev("(()=>{"+CS+"const t=document.querySelectorAll('[data-msgs]')[0];const find=(p,f)=>Array.from(p.querySelectorAll('*')).filter(f);"
   "const bubble=find(t,d=>cs(d).borderRadius==='10px 10px 3px 10px')[0];const bb=cs(bubble);"
   "const wrap=bubble.parentElement,wcs=cs(wrap),wr=wrap.getBoundingClientRect();"
   "const a=find(t,d=>cs(d).maxWidth==='94%')[0],acs=cs(a);"
   "return {msgs:{pad:cs(t).padding,gap:cs(t).gap,fs:cs(t).fontSize,ff:cs(t).fontFamily,overflow:cs(t).overflow},"
   "userWrap:{maxWidth:wcs.maxWidth,alignSelf:wcs.alignSelf,gap:wcs.gap,dir:wcs.flexDirection,w:Math.round(wr.width)},"
   "bubble:{bg:bb.backgroundColor,color:bb.color,pad:bb.padding,radius:bb.borderRadius,lh:bb.lineHeight,fs:bb.fontSize,ws:bb.whiteSpace,textWrap:bb.textWrap,w:Math.round(bubble.getBoundingClientRect().width)},"
   "agent:{maxWidth:acs.maxWidth,gap:acs.gap,dir:acs.flexDirection,fs:acs.fontSize,lh:acs.lineHeight,w:Math.round(a.getBoundingClientRect().width)}};})()", 2500)

# 2 agent header line
ev("(()=>{"+CS+"const t=document.querySelectorAll('[data-msgs]')[0];"
   "const hdr=Array.from(t.querySelectorAll('div')).find(d=>cs(d).fontSize==='11px'&&/^ARCHONUI/.test(d.textContent.trim()));"
   "const logo=hdr.querySelector('div');"
   "return {hdr:{gap:cs(hdr).gap,fs:cs(hdr).fs||cs(hdr).fontSize,color:cs(hdr).color,gap2:cs(hdr).gap},"
   "logo:{w:logo.style.width,h:logo.style.height,ir:cs(logo).imageRendering,src:cs(logo).backgroundImage.slice(0,60)},"
   "spans:Array.from(hdr.querySelectorAll('span')).map(s=>({t:s.textContent.trim(),ls:cs(s).letterSpacing,c:cs(s).color,fs:cs(s).fontSize}))};})()", 2000)

# 3 tool group rows
ev("(()=>{"+CS+"const t=document.querySelectorAll('[data-msgs]')[0];"
   "const grp=Array.from(t.querySelectorAll('div')).find(d=>cs(d).borderRadius==='8px'&&cs(d).overflow==='hidden'&&/read/.test(d.textContent));"
   "return {group:{bd:cs(grp).border,radius:cs(grp).borderRadius,gap:cs(grp).gap,dir:cs(grp).flexDirection},"
   "rows:Array.from(grp.children).map(r=>({h:Math.round(r.getBoundingClientRect().height),pad:cs(r).padding,fs:cs(r).fontSize,bg:cs(r).backgroundColor,gap:cs(r).gap,"
   "spans:Array.from(r.querySelectorAll('span')).map(s=>({t:s.textContent.trim(),c:cs(s).color,w:cs(s).width,fs:cs(s).fontSize,ta:cs(s).textAlign,flex:cs(s).flex}))}))};})()", 3000)

# 4 code block
ev("(()=>{"+CS+"const t=document.querySelectorAll('[data-msgs]')[0];"
   "const el=Array.from(t.querySelectorAll('div')).find(d=>cs(d).borderRadius==='8px'&&/Copy/.test(d.textContent)&&!/Open/.test(d.textContent)&&d.children.length===2);"
   "const head=el.children[0],body=el.children[1],ln=body.children[0];"
   "return {block:{bd:cs(el).border,radius:cs(el).borderRadius,bg:cs(el).backgroundColor},"
   "head:{pad:cs(head).padding,fs:cs(head).fontSize,color:cs(head).color,bd:cs(head).borderBottom,display:cs(head).display,gap:cs(head).gap,"
   "lang:{t:head.children[0].textContent,fs:cs(head.children[0]).fontSize,c:cs(head.children[0]).color,ls:cs(head.children[0]).letterSpacing,tt:cs(head.children[0]).textTransform,ff:cs(head.children[0]).fontFamily},"
   "file:head.children[1].textContent,fileColor:cs(head.children[1]).color,"
   "copy:{t:head.querySelector('button').textContent,pad:cs(head.querySelector('button')).padding,radius:cs(head.querySelector('button')).borderRadius,fs:cs(head.querySelector('button')).fs||cs(head.querySelector('button')).fontSize,c:cs(head.querySelector('button')).color,cur:cs(head.querySelector('button')).cursor}},"
   "body:{pad:cs(body).padding,ff:cs(body).fontFamily,fs:cs(body).fontSize,lh:cs(body).lineHeight,overflow:cs(body).overflow,lines:body.children.length},"
   "line:{display:cs(ln).display,gap:cs(ln).gap,ws:cs(ln).whiteSpace,padRight:cs(ln).paddingRight,"
   "num:{w:cs(ln.children[0]).width,c:cs(ln.children[0]).color,ta:cs(ln.children[0]).textAlign,flex:cs(ln.children[0]).flex}},"
   "line2segs:Array.from(body.children[1].children[1].children).map(s=>({x:s.textContent,c:cs(s).color})),"
   "line1segs:Array.from(ln.children[1].children).map(s=>({x:s.textContent,c:cs(s).color}))};})()", 5000)

# 5 attachment card in sent message
ev("(()=>{"+CS+"const els=Array.from(document.querySelectorAll('div')).filter(d=>cs(d).borderRadius==='8px'&&cs(d).maxWidth==='240px');"
   "return els.map(el=>({bd:cs(el).border,bg:cs(el).backgroundColor,pad:cs(el).padding,radius:cs(el).borderRadius,gap:cs(el).gap,maxW:cs(el).maxWidth,"
   "spans:Array.from(el.querySelectorAll('span')).map(s=>({t:s.textContent.trim(),fs:cs(s).fontSize,c:cs(s).color,bg:cs(s).backgroundColor,w:cs(s).width,h:cs(s).height,radius:cs(s).borderRadius,fw:cs(s).fontWeight,ls:cs(s).letterSpacing,display:cs(s).display,place:cs(s).placeItems}))}));})()", 2500)

# 6 composer wrapper + toolbar
ev("(()=>{"+CS+"const ta=document.querySelector('textarea');const p=ta.parentElement;const bar=p.children[p.children.length-3];"
   "return {wrapper:{bd:cs(p).border,radius:cs(p).borderRadius,bg:cs(p).backgroundColor,pos:cs(p).position,w:Math.round(p.getBoundingClientRect().width)},"
   "outer:{pad:cs(p.parentElement).padding},"
   "ta:{pad:cs(ta).padding,fs:cs(ta).fontSize,lh:cs(ta).lineHeight,color:cs(ta).color,ff:cs(ta).fontFamily,display:cs(ta).display,rows:ta.rows,ph:ta.placeholder,resize:cs(ta).resize},"
   "bar:{pad:cs(bar).padding,gap:cs(bar).gap,display:cs(bar).display,wrap:cs(bar).flexWrap,"
   "kids:Array.from(bar.children).map(c=>({tag:c.tagName,t:c.textContent.trim().slice(0,24),w:c.style.width,h:c.style.height,bg:cs(c).backgroundColor,radius:cs(c).borderRadius,color:cs(c).color,fs:cs(c).fontSize,pad:cs(c).padding,display:cs(c).display,gap:cs(c).gap,title:c.title||''}))}};})()", 3000)

# 7 composer buttons detail (svgs, styles)
ev("(()=>{"+CS+"const ta=document.querySelector('textarea');const p=ta.parentElement;"
   "return Array.from(p.querySelectorAll('button')).map(b=>({title:b.title,t:b.textContent.trim().slice(0,24),w:b.style.width,h:b.style.height,bgs:b.style.background,"
   "cssbg:cs(b).backgroundColor,color:cs(b).color,radius:cs(b).borderRadius,fs:cs(b).fontSize,gap:cs(b).gap,display:cs(b).display,"
   "svg:b.querySelectorAll('svg').length?Array.from(b.querySelectorAll('svg')).map(s=>s.getAttribute('width')+'x'+s.getAttribute('height')+' vb='+s.getAttribute('viewBox')+' '+Array.from(s.querySelectorAll('path,rect,circle')).map(x=>x.getAttribute('d')||x.getAttribute('x')).join(' | ')):[]}));})()", 4000)

# 8 working indicator
ev("(()=>{"+CS+"return Array.from(document.querySelectorAll('[data-msgs]')).map((t,i)=>Array.from(t.querySelectorAll('span')).filter(s=>/^Working/.test(s.textContent.trim())).map(s=>({tile:i,t:s.textContent.trim(),fs:cs(s).fontSize,c:cs(s).color,italic:cs(s).fontStyle,style:s.getAttribute('style')}))).flat();})()", 1500)

# 9 chart block
ev("(()=>{"+CS+"const chart=Array.from(document.querySelectorAll('div')).find(d=>/Copy CSV/.test(d.textContent));"
   "const head=chart.children[0],body=chart.children[1],plot=body.children[0];"
   "const bars=Array.from(plot.children).filter(d=>d.style.flex==='1');"
   "const grid=Array.from(plot.children).filter(d=>/dashed/.test(cs(d).borderTopStyle));"
   "const labels=body.children[1];"
   "return {frame:{bd:cs(chart).border,radius:cs(chart).borderRadius,bg:cs(chart).backgroundColor},"
   "head:{pad:cs(head).padding,fs:cs(head).fontSize,color:cs(head).color,bd:cs(head).borderBottom,display:cs(head).display,gap:cs(head).gap,"
   "title:{t:head.children[1].textContent,fs:cs(head.children[1]).fontSize,c:cs(head.children[1]).color},sum:head.children[2].textContent,"
   "copy:{t:head.querySelector('button').textContent,pad:cs(head.querySelector('button')).padding,radius:cs(head.querySelector('button')).borderRadius,fs:cs(head.querySelector('button')).fontSize}},"
   "body:{pad:cs(body).padding},plot:{h:cs(plot).height,display:cs(plot).display,gap:cs(plot).gap,align:cs(plot).alignItems,bd:cs(plot).borderBottom},"
   "grid:grid.map(g=>({bottom:g.style.bottom,border:cs(g).borderTop})),"
   "bars:bars.map(b=>({label:b.children[0].textContent,v:b.children[0].textContent,vc:cs(b.children[0]).color,h:b.children[1].style.height,bg:b.children[1].style.background,w:Math.round(b.children[1].getBoundingClientRect().width),maxW:cs(b.children[1]).maxWidth,radius:cs(b.children[1]).borderRadius,rootH:Math.round(b.getBoundingClientRect().height)})),"
   "xLabels:{pad:cs(labels).padding,gap:cs(labels).gap,items:Array.from(labels.children).map(s=>({t:s.textContent,fs:cs(s).fontSize,c:cs(s).color}))}};})()", 4000)

# 10 image card
ev("(()=>{"+CS+"const img=Array.from(document.querySelectorAll('div')).find(d=>/Attach/.test(d.textContent)&&d.style.maxWidth==='560px');"
   "const thumb=img.children[0],cap=img.children[1];"
   "return {frame:{maxW:cs(img).maxWidth,bd:cs(img).border,radius:cs(img).borderRadius,bg:cs(img).backgroundColor},"
   "thumb:{aspect:cs(thumb).aspectRatio,cursor:cs(thumb).cursor,bg:cs(thumb).backgroundImage.slice(0,90),w:Math.round(thumb.getBoundingClientRect().width),h:Math.round(thumb.getBoundingClientRect().height)},"
   "cap:{pad:cs(cap).padding,gap:cs(cap).gap,bd:cs(cap).borderTop,display:cs(cap).display,"
   "texts:Array.from(cap.querySelectorAll('span')).map(s=>({t:s.textContent.trim(),fs:cs(s).fontSize,c:cs(s).color,ov:cs(s).textOverflow})),"
   "btns:Array.from(cap.querySelectorAll('button')).map(b=>({t:b.textContent,pad:cs(b).padding,radius:cs(b).borderRadius,fs:cs(b).fontSize,c:cs(b).color}))}};})()", 3000)

json.dump(S, open('/home/archonminipc/ArchonUI/.qa/design/chat-steps3.json','w'))
print("steps", len(S))
