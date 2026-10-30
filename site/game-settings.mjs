// The settings surface is painted into the game canvas. The clipped DOM is only its
// accessible counterpart; no separate visible web settings panel or engine input is used.
const ASCII = {
  A:['01110','10001','10001','11111','10001','10001','10001'],B:['11110','10001','10001','11110','10001','10001','11110'],
  C:['01111','10000','10000','10000','10000','10000','01111'],D:['11110','10001','10001','10001','10001','10001','11110'],
  E:['11111','10000','10000','11110','10000','10000','11111'],F:['11111','10000','10000','11110','10000','10000','10000'],
  G:['01111','10000','10000','10111','10001','10001','01111'],H:['10001','10001','10001','11111','10001','10001','10001'],
  I:['111','010','010','010','010','010','111'],J:['00111','00010','00010','00010','10010','10010','01100'],
  K:['10001','10010','10100','11000','10100','10010','10001'],L:['10000','10000','10000','10000','10000','10000','11111'],
  M:['10001','11011','10101','10101','10001','10001','10001'],N:['10001','11001','10101','10011','10001','10001','10001'],
  O:['01110','10001','10001','10001','10001','10001','10001','01110'],P:['11110','10001','10001','11110','10000','10000','10000'],
  Q:['01110','10001','10001','10001','10101','10010','01101'],R:['11110','10001','10001','11110','10100','10010','10001'],
  S:['01111','10000','10000','01110','00001','00001','11110'],T:['11111','00100','00100','00100','00100','00100','00100'],
  U:['10001','10001','10001','10001','10001','10001','01110'],V:['10001','10001','10001','10001','10001','01010','00100'],
  W:['10001','10001','10001','10101','10101','11011','10001'],X:['10001','10001','01010','00100','01010','10001','10001'],
  Y:['10001','10001','01010','00100','00100','00100','00100'],Z:['11111','00001','00010','00100','01000','10000','11111'],
  0:['01110','10001','10011','10101','11001','10001','01110'],1:['010','110','010','010','010','010','111'],
  2:['01110','10001','00001','00010','00100','01000','11111'],3:['11110','00001','00001','01110','00001','00001','11110'],
  4:['00010','00110','01010','10010','11111','00010','00010'],5:['11111','10000','10000','11110','00001','00001','11110'],
  6:['01110','10000','10000','11110','10001','10001','01110'],7:['11111','00001','00010','00100','01000','01000','01000'],
  8:['01110','10001','10001','01110','10001','10001','01110'],9:['01110','10001','10001','01111','00001','00001','01110'],
  '.':['0','0','0','0','0','0','1'],':':['0','1','0','0','1','0','0'],'-':['000','000','000','111','000','000','000'],
  '+':['000','010','010','111','010','010','000'],'/':['00001','00001','00010','00100','01000','10000','10000'],
  '(' :['01','10','10','10','10','10','01'],')':['10','01','01','01','01','01','10'],
  '<':['001','010','100','010','001','000','000'],'>':['100','010','001','010','100','000','000'],
  '?':['1110','0001','0001','0010','0100','0000','0100'],'!':['1','1','1','1','1','0','1'],
  ',':['0','0','0','0','0','1','1'],"'":['1','1','0','0','0','0','0'],'=':['000','111','000','111','000','000','000'],
  ';':['00','01','00','00','01','01','10'],'&':['01100','10010','10100','01000','10101','10010','01101'],
};
const COLORS = { background:'#101010', ink:'#f4f4f4', muted:'#b4b4b4', line:'#464646', selected:'#343434' };
const inside = (x,y,r) => x>=r[0] && y>=r[1] && x<r[0]+r[2] && y<r[1]+r[3];

export function createGameSettings({ canvas, language, sections, onOpen, onClose, onChange, onAction }) {
  let open=false, section='display', page=0, focused='close', pressed=null, items=[], hangul=null, fontState='loading';
  let modal=null, modalPage=0, notice='', savedStyle=null, backdrop=null, patch=null, patchRect=null, stageDisplay=null;
  const stage=document.getElementById('game-stage');
  const entryControl=document.getElementById('game-settings-open')??document.createElement('button');
  if(!entryControl.isConnected){entryControl.id='game-settings-open';entryControl.type='button';canvas.before(entryControl);}
  entryControl.addEventListener('click',()=>open?hide():show());
  const mirror=document.createElement('div');
  mirror.id='game-settings-accessibility';
  mirror.setAttribute('aria-label','Game settings');
  Object.assign(mirror.style,{position:'absolute',width:'1px',height:'1px',padding:'0',margin:'-1px',overflow:'hidden',clipPath:'inset(50%)',whiteSpace:'nowrap'});
  mirror.hidden=true; canvas.parentElement.append(mirror);
  const ko=()=>language()==='ko';
  const label=(en,korean)=>ko()?korean:en;
  const glyph=(c)=> {
    const n=c.codePointAt(0)-0xac00;
    if(n>=0&&n<11172) return {rows:hangul?.subarray(n*8,n*8+8)??[],width:8,fallback:!hangul};
    if(c===' ')return {rows:[],width:3};
    const equivalent=({'—':'-','–':'-','×':'X','…':'.','·':'.','’':"'"})[c]??c.toUpperCase();
    const a=ASCII[equivalent]??ASCII['?'];
    return {rows:a.map(r=>parseInt(r,2)<<(8-r.length)),width:a[0].length};
  };
  const measure=(s,scale=2)=>[...String(s)].reduce((n,c)=>n+(glyph(c).width+1)*scale,0);
  function text(ctx,s,x,y,color=COLORS.ink,scale=2,max=Infinity) {
    ctx.fillStyle=color;
    let pen=Math.round(x);
    for(const c of String(s)) {
      const g=glyph(c);
      if(pen+(g.width+1)*scale>x+max)break;
      if(g.fallback){ctx.font=`${8*scale}px monospace`;ctx.textBaseline='top';ctx.fillText(c,pen,Math.round(y));}
      for(let v=0;v<g.rows.length;v++)for(let u=0;u<g.width;u++)if(g.rows[v]&(0x80>>u))ctx.fillRect(pen+u*scale,Math.round(y)+v*scale,scale,scale);
      pen+=(g.width+1)*scale;
    }
  }
  function wrap(s,width,scale=2) {
    const result=[];
    for(const paragraph of String(s).split('\n')) {
      let line='';
      for(const word of paragraph.split(' ')) {
        const next=line?line+' '+word:word;
        if(measure(next,scale)>width&&line){result.push(line);line=word;} else line=next;
        while(measure(line,scale)>width) {
          let k=1;while(k<line.length&&measure(line.slice(0,k+1),scale)<=width)k++;
          result.push(line.slice(0,k));line=line.slice(k);
        }
      }
      result.push(line);
    }
    return result;
  }
  const copyCanvas=()=>{const c=document.createElement('canvas');c.width=canvas.width;c.height=canvas.height;c.getContext('2d').drawImage(canvas,0,0);return c;};
  function restore() {
    const ctx=canvas.getContext('2d');
    if(open&&backdrop){if(canvas.width!==backdrop.width)canvas.width=backdrop.width;if(canvas.height!==backdrop.height)canvas.height=backdrop.height;ctx.drawImage(backdrop,0,0);}
    else if(patch&&patchRect&&patchRect[4]===canvas.width&&patchRect[5]===canvas.height)ctx.drawImage(patch,patchRect[0],patchRect[1]);
    patch=null;patchRect=null;
  }
  function geometry() {
    const r=canvas.getBoundingClientRect();
    return {r,w:r.width,h:r.height,margin:Math.max(12,(r.width-680)/2)};
  }
  function entryRect() {
    const {w}=geometry();return [Math.max(0,w-44),0,44,44];
  }
  function button(ctx,id,title,rect,extra={}) {
    items.push({id,label:title,value:extra.value??'',valueLabel:extra.valueLabel??'',options:extra.options??[],disabled:!!extra.disabled,rect});
    ctx.fillStyle=focused===id?COLORS.selected:COLORS.background;ctx.fillRect(...rect);
    ctx.strokeStyle=focused===id?COLORS.ink:COLORS.line;ctx.lineWidth=1;ctx.strokeRect(rect[0]+.5,rect[1]+.5,rect[2]-1,rect[3]-1);
    if(extra.value==='active'){ctx.fillStyle=COLORS.ink;ctx.fillRect(rect[0]+8,rect[1]+rect[3]-4,rect[2]-16,3);}
    text(ctx,title,extra.tab?rect[0]+Math.max(0,(rect[2]-measure(title))/2):rect[0]+12,rect[1]+(extra.labelTop??(rect[3]-14)/2),extra.disabled?COLORS.muted:COLORS.ink,2,extra.tab?rect[2]:rect[2]-24);
  }
  function semantic() {
    const prior=document.activeElement?.dataset?.settingId;
    mirror.replaceChildren();mirror.hidden=!open;
    if(!open)return;
    mirror.setAttribute('role','dialog');mirror.setAttribute('aria-modal','true');mirror.setAttribute('aria-label',label('Settings — game paused','설정 — 게임 일시정지'));
    for(const item of items) {
      const b=document.createElement('button');b.type='button';b.id=`game-setting-${item.id}`;b.dataset.settingId=item.id;b.disabled=item.disabled;b.tabIndex=item.id===focused?0:-1;
      if(item.id.startsWith('nav-'))b.setAttribute('aria-current',item.value==='active'?'page':'false');
      b.textContent=[item.label,item.valueLabel].filter(Boolean).join(': ');
      b.addEventListener('click',()=>activate(item.id,1));
      b.addEventListener('focus',()=>{focused=item.id;paint(false);});
      b.addEventListener('keydown',key);
      mirror.append(b);
    }
    if(notice){const status=document.createElement('p');status.setAttribute('role','status');status.textContent=notice;mirror.append(status);}
    if(prior)mirror.querySelector(`[data-setting-id="${focused}"]`)?.focus({preventScroll:true});
  }
  function paint(updateSemantic=true) {
    const title=label('Settings','설정'),expanded=String(open);
    if(entryControl.textContent!==title)entryControl.textContent=title;
    if(entryControl.getAttribute('aria-expanded')!==expanded)entryControl.setAttribute('aria-expanded',expanded);
    if(!open){items=[];return;}
    if(!canvas.width||!canvas.height)return;
    const {r,w,h,margin}=geometry();if(w<1||h<1)return;
    if(open){const dpr=window.devicePixelRatio||1;const cw=Math.round(w*dpr),ch=Math.round(h*dpr);if(canvas.width!==cw)canvas.width=cw;if(canvas.height!==ch)canvas.height=ch;}
    const ctx=canvas.getContext('2d');
    ctx.save();ctx.setTransform(canvas.width/w,0,0,canvas.height/h,0,0);ctx.imageSmoothingEnabled=false;
    items=[];
    ctx.fillStyle=COLORS.background;ctx.fillRect(0,0,w,h);
    const right=w-margin,contentWidth=right-margin;
    text(ctx,label('Settings','설정'),margin,22);
    text(ctx,label('PAUSED','일시정지'),margin+measure(label('Settings','설정'))+20,24,COLORS.muted,1);
    button(ctx,'close',label('Close','닫기'),[right-92,8,92,44]);
    const categories=[['display',label('View','화면')],['play',label('Play','진행')],['audio',label('Audio','소리')],['files',label('Files','파일')]];
    if(modal) {
      text(ctx,modal.title,margin,70,COLORS.ink,2,contentWidth);
      const lines=wrap(modal.body,contentWidth,2), capacity=Math.max(1,Math.floor((h-(modal.kind==='terms'?264:214))/22));
      const pages=Math.max(1,Math.ceil(lines.length/capacity));modalPage=Math.min(modalPage,pages-1);
      for(const [i,line]of lines.slice(modalPage*capacity,(modalPage+1)*capacity).entries())text(ctx,line,margin,100+i*22,COLORS.muted,2,contentWidth);
      if(modal.kind==='terms') {
        button(ctx,'terms-license','LICENSE',[margin,h-158,112,44]);
        button(ctx,'terms-notice','NOTICE',[margin+120,h-158,112,44]);
        button(ctx,'terms-decline',label('Back','뒤로'),[margin,h-54,100,44]);
        button(ctx,'terms-accept',label('Agree & load','동의 후 받기'),[right-176,h-54,176,44]);
      } else {
        button(ctx,'confirm-cancel',label('Cancel','취소'),[margin,h-54,112,44]);
        button(ctx,'confirm-accept',label('Delete','삭제'),[right-112,h-54,112,44]);
      }
      if(pages>1) {
        button(ctx,'terms-previous','<',[right-112,h-106,48,44],{disabled:modalPage===0});
        button(ctx,'terms-next','>',[right-48,h-106,48,44],{disabled:modalPage===pages-1});
        text(ctx,`${modalPage+1}/${pages}`,margin,h-91,COLORS.muted,2);
      }
    } else {
      const tabWidth=(contentWidth-18)/4;
      for(const [i,[id,title]]of categories.entries())button(ctx,`nav-${id}`,title,[margin+i*(tabWidth+6),64,tabWidth,44],{value:id===section?'active':'',tab:true});
      const rows=sections(section).map(item=>{
        const stack=!!item.valueLabel&&measure(item.label)+measure(item.valueLabel)+48>contentWidth;
        const valueLines=stack?wrap(item.valueLabel,contentWidth-24,2):[];
        return {...item,stack,valueLines,height:stack?(valueLines.length>1?84:64):48};
      });
      const available=h-228,groups=[[]];let used=0;
      for(const row of rows){if(used+row.height>available&&groups.at(-1).length){groups.push([]);used=0;}groups.at(-1).push(row);used+=row.height+8;}
      const pages=groups.length;page=Math.min(page,pages-1);let y=120;
      for(const item of groups[page]) {
        const rect=[margin,y,contentWidth,item.height];
        button(ctx,item.id,item.label,rect,{...item,labelTop:item.stack?8:undefined});
        if(item.valueLabel) {
          if(item.stack)for(const [i,line]of item.valueLines.slice(0,2).entries())text(ctx,line,margin+12,y+32+i*20,COLORS.muted,2,contentWidth-24);
          else text(ctx,item.valueLabel,right-12-measure(item.valueLabel),y+17,COLORS.muted,2,contentWidth-24);
        }
        y+=item.height+8;
      }
      if(pages>1) {
        button(ctx,'previous','<',[margin,h-54,48,44],{disabled:page===0});
        text(ctx,`${page+1}/${pages}`,margin+64,h-39,COLORS.muted,2);
        button(ctx,'next','>',[margin+118,h-54,48,44],{disabled:page===pages-1});
      }
      if(notice)for(const [i,line]of wrap(notice,contentWidth,2).slice(0,2).entries())text(ctx,line,margin,h-102+i*20,COLORS.muted,2,contentWidth);
      else if(contentWidth>430)text(ctx,label('F10 / ESC TO RETURN','F10 / ESC 닫기'),right-200,h-36,COLORS.muted,1);
    }
    ctx.restore();
    if(updateSemantic)semantic();
  }
  function saveEntry() {}
  function refresh(){restore();if(open)paint();else{saveEntry();paint();}}
  function show(which='display') {
    if(open){section=which;page=0;modal=null;refresh();return;}
    restore();backdrop=copyCanvas();if(stage){stageDisplay=stage.style.display;stage.style.display='block';}open=true;section=which;page=0;modal=null;notice='';focused='close';
    savedStyle={height:canvas.style.height,aspectRatio:canvas.style.aspectRatio,imageRendering:canvas.style.imageRendering};
    const box=canvas.getBoundingClientRect();const height=Math.max(320,Math.min(560,window.innerHeight-Math.max(0,box.top)-12));
    canvas.style.height=`${height}px`;canvas.style.aspectRatio='auto';canvas.style.imageRendering='pixelated';
    document.body.dataset.settings='open';canvas.dataset.pointer='page';canvas.setAttribute('aria-label',label('Settings. F10 or Escape to return to game.','설정. F10 또는 Escape로 게임 복귀.'));
    onOpen();canvas.focus({preventScroll:true});canvas.scrollIntoView({block:'nearest'});paint();
  }
  function hide() {
    if(!open)return;
    restore();open=false;const declined=modal?.kind==='terms';modal=null;if(declined)onAction('terms-decline');Object.assign(canvas.style,savedStyle);savedStyle=null;backdrop=null;if(stage)stage.style.display=stageDisplay;
    document.body.dataset.settings='closed';mirror.hidden=true;canvas.setAttribute('aria-label',label('Ultima VI. F10 opens settings.','울티마 VI. F10으로 설정 열기.'));
    onClose();canvas.focus({preventScroll:true});if(!patch){saveEntry();paint();}
  }
  async function activate(id,direction=1) {
    const item=items.find(i=>i.id===id);if(item?.disabled)return;
    focused=id;
    if(id==='close'){hide();return;}
    if(id.startsWith('nav-')){section=id.slice(4);page=0;notice='';modal=null;}
    else if(id==='previous')page=Math.max(0,page-1);
    else if(id==='next')page++;
    else if(id==='terms-previous')modalPage=Math.max(0,modalPage-1);
    else if(id==='terms-next')modalPage++;
    else if(id==='terms-decline'||id==='confirm-cancel'){const kind=modal?.kind;modal=null;if(kind==='terms')onAction('terms-decline');}
    else if(id==='terms-license'||id==='terms-notice')onAction(id);
    else if(id==='terms-accept'){modal=null;const result=await onAction(id);if(typeof result==='string')notice=result;}
    else if(id==='confirm-accept'){const action=modal?.action;modal=null;notice=label('Applying...','적용 중...');refresh();const result=await onAction(action);if(typeof result==='string')notice=result;}
    else if(item?.options.length) {
      const index=item.options.findIndex(o=>o.value===item.value);
      const next=item.options[(index+direction+item.options.length)%item.options.length];
      notice=label('Applying...','적용 중...');refresh();const result=await onChange(id,next.value);if(typeof result==='string')notice=result;
    } else {
      const result=await onAction(id);if(typeof result==='string')notice=result;
    }
    if(open)refresh();
  }
  function key(e) {
    if(e.key==='F10'&&!e.ctrlKey&&!e.metaKey&&!e.altKey){e.preventDefault();if(!e.repeat)open?hide():show();return true;}
    if(!open)return false;
    if(e.metaKey||e.ctrlKey||e.altKey)return true;
    if(e.key==='Escape'){e.preventDefault();if(modal){const kind=modal.kind;modal=null;if(kind==='terms')onAction('terms-decline');refresh();}else hide();return true;}
    const available=items.filter(i=>!i.disabled);let at=available.findIndex(i=>i.id===focused);
    if(['Tab','ArrowDown','ArrowUp'].includes(e.key)) {
      e.preventDefault();const d=e.key==='ArrowUp'||(e.key==='Tab'&&e.shiftKey)?-1:1;
      focused=available[(at+d+available.length)%available.length]?.id??'close';refresh();mirror.querySelector(`[data-setting-id="${focused}"]`)?.focus({preventScroll:true});
    } else if(['Enter',' ','ArrowLeft','ArrowRight'].includes(e.key)) {
      e.preventDefault();const choice=items.find(i=>i.id===focused);
      if(!e.repeat&&(!e.key.startsWith('Arrow')||choice?.options.length))activate(focused,e.key==='ArrowLeft'?-1:1);
    } else e.preventDefault();
    return true;
  }
  function pointer(e) {
    const r=canvas.getBoundingClientRect(),x=e.clientX-r.left,y=e.clientY-r.top;
    if(!open)return false;
    e.preventDefault();const hit=items.find(i=>inside(x,y,i.rect)&&!i.disabled);
    if(e.type==='pointerdown'){pressed=hit?.id??null;try{canvas.setPointerCapture?.(e.pointerId);}catch{/* Cancelled/synthetic pointer owns no capture. */}}
    else if(e.type==='pointerup'){const id=pressed;pressed=null;if(id&&hit?.id===id)activate(id);}
    else if(e.type==='pointercancel'||e.type==='lostpointercapture')pressed=null;
    if(hit&&focused!==hit.id){focused=hit.id;refresh();}
    return true;
  }
  function wheel(e){if(!open)return false;e.preventDefault();if(e.deltaY!==0){if(modal)modalPage=Math.max(0,modalPage+Math.sign(e.deltaY));else page=Math.max(0,page+Math.sign(e.deltaY));refresh();}return true;}
  function confirm(title,body,action){modal={kind:'confirm',title,body,action};modalPage=0;focused='confirm-cancel';refresh();}
  function terms(title,body){if(!open)show('audio');modal={kind:'terms',title,body};modalPage=0;focused='terms-decline';refresh();}
  const api={
    get open(){return open;},get section(){return section;},get page(){return page;},get focused(){return focused;},get notice(){return notice;},get fontState(){return fontState;},
    get items(){const r=canvas.getBoundingClientRect();return items.map(i=>({...i,rect:[r.left+i.rect[0],r.top+i.rect[1],...i.rect.slice(2)],options:i.options.map(o=>({...o}))}));},
    get entry(){const r=entryControl.getBoundingClientRect();return[r.left,r.top,r.width,r.height];},
    get entryPaint(){return[0,0,0,0];},
  };
  Object.defineProperty(window,'u6Settings',{value:Object.freeze(api),configurable:false,writable:false});
  fetch(new URL('./fonts/galmuri7-hangul.bin',import.meta.url)).then(r=>{if(!r.ok)throw new Error('font');return r.arrayBuffer();}).then(b=>{if(b.byteLength!==11172*8)throw new Error('font size');hangul=new Uint8Array(b);fontState='ready';refresh();}).catch(()=>{fontState='fallback';if(open){notice=label('Using the system Korean font.','기본 한글 글꼴로 표시합니다.');refresh();}});
  window.addEventListener('resize',()=>{if(open)canvas.style.height=`${Math.max(320,Math.min(560,window.innerHeight-Math.max(0,canvas.getBoundingClientRect().top)-12))}px`;refresh();});
  return {get open(){return open;},get section(){return section;},show,hide,key,pointer,wheel,confirm,terms,refresh,
    beforeGameDraw:restore,
    afterGameDraw(){if(open){backdrop=copyCanvas();paint();}else{saveEntry();paint();}},
    covers(){return open;},
    notify(message){notice=message;if(open)refresh();},
  };
}
