import json
S=[]
def ev(js,mx=4000): S.append(["eval",js,mx])
def shot(n): S.append(["shot",n])
def click(sel): S.append(["click",sel])
def ctext(t,exact=True): S.append(["clickText",t,exact])
def wait(ms): S.append(["wait",ms])
CS="const cs=e=>getComputedStyle(e);"
ZRESET="(()=>{const w=Array.from(document.querySelectorAll('div')).find(d=>d.style.width==='1920px'&&d.style.position==='fixed');w.setAttribute('style',window.__orig);return 'reset';})()"
def zoom(sel,S_):
    return ("(()=>{"+CS+"const w=Array.from(document.querySelectorAll('div')).find(d=>d.style.width==='1920px'&&d.style.position==='fixed');if(!window.__orig)window.__orig=w.getAttribute('style');"
     "const t="+sel+";if(!t)return 'no target';const wr=w.getBoundingClientRect(),tr=t.getBoundingClientRect();const cur=wr.width/1920;"
     "const cx=((tr.x+tr.width/2)-wr.x)/cur, cy=((tr.y+tr.height/2)-wr.y)/cur;"
     "w.style.transformOrigin='0 0';w.style.left='0px';w.style.top='0px';"
     "w.style.transform='translate('+(-(cx*"+str(S_)+"-800))+'px,'+(-(cy*"+str(S_)+"-500))+'px) scale("+str(S_)+")';return 'zoomed';})()")
shot("01-default")
click('button[title="Attach files"]'); wait(400); shot("09-attach-popover")
ev(zoom("Array.from(document.querySelectorAll('div')).find(d=>d.style.width==='300px')",1.7)); wait(200); shot("09b-attach-popover-zoom"); ev(ZRESET); wait(200)
ctext("loop.ts",True); wait(400)
ev(zoom("document.querySelector('textarea').parentElement",1.6)); wait(200); shot("11b-composer-attachment-zoom"); ev(ZRESET)
json.dump(S,open('/home/archonminipc/ArchonUI/.qa/design/chat-steps17.json','w'))
print(len(S))
