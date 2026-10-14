import json,sys
CLICK = sys.argv[1] if len(sys.argv)>1 else None
S=[]
def ev(js,mx=4000): S.append(["eval",js,mx])
def shot(n): S.append(["shot",n])
def click(sel): S.append(["click",sel])
def ctext(t,exact=True): S.append(["clickText",t,exact])
def key(k): S.append(["key",k])
def wait(ms): S.append(["wait",ms])
def xy(x,y): S.append(["clickXY",x,y])
CS="const cs=e=>getComputedStyle(e);"
PICKER="Array.from(document.querySelectorAll('div')).find(d=>d.style.width==='360px'&&/100%/.test(d.style.bottom))"
SCROLL=("(()=>{const el=Array.from(document.querySelectorAll('div')).find(d=>(d.getAttribute('style')||'').includes('overflow: auto; min-height: 0px'));"
 "el.scrollTop=el.scrollHeight;return {scrollTop:el.scrollTop,scrollH:el.scrollHeight,clientH:el.clientHeight};})()")
RECT=("(()=>{"+CS+"const rows=Array.from(document.querySelectorAll('div')).filter(d=>(d.getAttribute('style')||'').includes('grid-template-columns'));"
 "const row=rows.find(d=>/llama3\\.3 70B/.test(d.textContent));const tog=row.children[row.children.length-1];const r=tog.getBoundingClientRect();"
 "const cx=Math.round(r.x+r.width/2),cy=Math.round(r.y+r.height/2);"
 "return {x:cx,y:cy,hit:(document.elementFromPoint(cx,cy)||{}).outerHTML?document.elementFromPoint(cx,cy).outerHTML.slice(0,80):null,track:cs(tog).backgroundColor};})()")
ctext("Settings",True); wait(800); ctext("Models",True); wait(600)
ev(SCROLL,800); wait(300)
if CLICK:
    pass
ev(RECT,1500)
if CLICK:
    xy(int(CLICK.split(',')[0]),int(CLICK.split(',')[1])); wait(500)
    ev("(()=>{"+CS+"const rows=Array.from(document.querySelectorAll('div')).filter(d=>(d.getAttribute('style')||'').includes('grid-template-columns'));const row=rows.find(d=>/llama3\\.3 70B/.test(d.textContent));const tog=row.children[row.children.length-1];return {track:cs(tog).backgroundColor,knobLeft:tog.children[0].style.left,on:cs(tog).backgroundColor!=='rgb(106, 90, 72)'};})()",800)
    shot("32-models-llama-on")
json.dump(S,open('/home/archonminipc/ArchonUI/.qa/design/chat-steps13.json','w'))
print(len(S))
