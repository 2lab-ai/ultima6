// Actual canvas settings receipts. Owned originals are runtime-only and never emitted.
import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import * as h from './harness.mjs';
import { RESTRICTIONS } from '../../site/voice/terms.js';
const out=process.env.U6_SETTINGS_OUT;
if(out)await mkdir(out,{recursive:true});
const env=await h.setup(), results=[];
const click=async(page,id)=>{const item=await h.settingsItem(page,id);assert.ok(item&&!item.disabled,id);const[x,y,w,t]=item.rect;await page.mouse.click(x+w/2,y+t/2);};
const observed=page=>page.evaluate(()=>({ticks:__ticks,clock:__clock,starts:__starts.length,keys:__shownGame.keys_pending(),generation:__shownGame.save_generation(),position:Array.from(__shownGame.leader_position()),sent:__sent.length,buttons:__buttons.length}));
try {
  for(const [layout,width,height]of [['original',1024,900],['hd',390,844],['wide',844,390]]) {
    const {page,context}=await h.openPage(env,{layout,font:'hd'});
    await page.setViewportSize({width,height});
    const errors=[];page.on('pageerror',e=>errors.push(e.name));
    await h.load(page,env);
    assert.equal(await page.evaluate(()=>document.querySelector('#load-details').open),false,'normal idle details start collapsed');
    assert.equal(await page.locator('#feedback-summary').textContent(),'Ready for your original files.','idle uses a short preparation summary');
    assert.ok((await page.locator('#status').textContent()).length>0,'raw loading information is preserved');
    assert.equal(await page.locator('#settings-bridge').isVisible(),false,'external game settings hidden');
    assert.equal(await page.locator('input[name="font"]').count(),0,'font choice removed');
    // F10 is usable even without original files; never opens an invisible keyboard trap.
    await page.keyboard.press('F10');
    assert.equal(await page.locator('#game-stage').isVisible(),true,'initial menu is visible');
    await page.keyboard.press('Escape');
    assert.equal(await page.evaluate(()=>u6Settings.open),false);
    await h.dropFiles(page,[h.ZIP]);await h.waitSettled(page);await h.waitPrompt(page);
    assert.equal(await page.evaluate(()=>document.body.dataset.font),'pixel','legacy hd preference ignored');
    await h.virtualDisplay(page,60);
    const originalSize=await page.evaluate(()=>{const c=document.querySelector('#screen');return[c.width,c.height];});
    await page.evaluate(()=>document.querySelector('#game-settings-open').scrollIntoView({block:'start'}));
    const entry=await page.evaluate(()=>u6Settings.entry);await page.mouse.click(entry[0]+entry[2]/2,entry[1]+entry[3]/2);
    assert.equal(await page.evaluate(()=>u6Settings.open),true,'canvas pointer opens settings');
    await page.keyboard.press('Tab');
    assert.equal(await page.evaluate(()=>document.activeElement?.dataset.settingId),'nav-display','Tab moves real accessible focus');
    await page.evaluate(()=>{
      window.__settingsCalls=Object.fromEntries(['step','pace','ran','set_clock','key_down','key_repeat','pointer'].map(n=>[n,0]));
      for(const name of Object.keys(__settingsCalls)){const fn=__shownGame[name].bind(__shownGame);__shownGame[name]=(...args)=>{__settingsCalls[name]++;return fn(...args);};}
      window.__settingsIdentity=__shownGame;
    });
    const before=await observed(page);
    await h.runFrames(page,600);
    await page.keyboard.press('ArrowRight');await page.keyboard.press('a');await page.keyboard.press('Control');
    await h.runFrames(page,60);
    h.assertSame(await observed(page),before,'menu frames and keys do not enter the game');
    await h.chooseSetting(page,'speed','2');await h.chooseSetting(page,'turns','4');
    await h.chooseSetting(page,'mute','on');await h.chooseSetting(page,'memory','on');await h.chooseSetting(page,'motion','on');
    const changed=await observed(page);
    assert.equal(changed.ticks,before.ticks,'changing preferences cannot step');
    assert.equal(changed.clock,before.clock,'changing preferences cannot advance clock');
    assert.equal(changed.starts,before.starts,'changing preferences cannot restart');
    assert.equal(changed.generation,before.generation,'changing preferences cannot save');
    h.assertSame(changed.position,before.position,'changing preferences cannot move');
    const calls=await page.evaluate(()=>({same:__settingsIdentity===__shownGame,calls:__settingsCalls}));
    assert.equal(calls.same,true,'settings retain the same live game object');
    assert.ok(Object.values(calls.calls).every(n=>n===0),'settings never call engine time, step or input methods');
    await h.chooseSetting(page,'layout',layout==='wide'?'original':'wide');
    await h.chooseSetting(page,'language','ko');
    assert.equal((await observed(page)).starts,before.starts,'language/build changes defer without restart');
    await h.clickSetting(page,'forget-save');
    await click(page,'confirm-cancel');
    assert.equal((await observed(page)).generation,before.generation,'cancel delete leaves save generation intact');
    await h.clickSetting(page,'voice-terms');
    const geometry=await page.evaluate(()=>u6Settings.items);
    for(let i=0;i<geometry.length;i++)for(let j=i+1;j<geometry.length;j++) {
      const a=geometry[i].rect,b=geometry[j].rect;
      assert.ok(!(a[0]<b[0]+b[2]&&b[0]<a[0]+a[2]&&a[1]<b[1]+b[3]&&b[1]<a[1]+a[3]),'terms hit areas do not overlap');
    }
    await click(page,'terms-decline');
    await h.settingsItem(page,'memory');
    const menu=await page.evaluate(()=>{const c=document.querySelector('#screen'),r=c.getBoundingClientRect();return{width:c.width,height:c.height,css:[r.width,r.height],items:u6Settings.items,domOverflow:document.documentElement.scrollWidth>innerWidth};});
    assert.equal(menu.domOverflow,false,'no horizontal overflow');
    assert.ok(Math.abs(menu.width-menu.css[0]*await page.evaluate(()=>devicePixelRatio))<=1,'menu renders at CSS device resolution');
    assert.ok(menu.items.every(i=>i.rect[2]>=44&&i.rect[3]>=44),'touch targets at least 44 CSS pixels');
    if(out)await page.locator('#screen').screenshot({path:join(out,`menu-${layout}-${width}.png`)});
    // Cancellation cannot activate a control when a later pointerup arrives.
    const target=(await h.settingsItem(page,'memory')).rect;
    const initial=await page.evaluate(()=>localStorage.getItem('u6.memory'));
    await page.dispatchEvent('#screen','pointerdown',{pointerId:31,pointerType:'touch',clientX:target[0]+10,clientY:target[1]+10,buttons:1,button:0});
    await page.dispatchEvent('#screen','pointercancel',{pointerId:31,pointerType:'touch',clientX:target[0]+10,clientY:target[1]+10,buttons:0});
    await page.dispatchEvent('#screen','pointerup',{pointerId:31,pointerType:'touch',clientX:target[0]+10,clientY:target[1]+10,buttons:0,button:0});
    assert.equal(await page.evaluate(()=>localStorage.getItem('u6.memory')),initial,'cancelled touch does not change preference');
    const blockedCalls=await page.evaluate(()=>({...__settingsCalls}));
    assert.ok(Object.values(blockedCalls).every(n=>n===0),'all menu flows keep engine time and input calls at zero');
    const expectedSize=await page.evaluate(()=>document.body.dataset.layout==='wide'?[1920,1080]:document.body.dataset.layout==='hd'?[1280,720]:[__shownGame.width()*(__shownGame.hd_on()?__shownGame.hd_scale():1),__shownGame.height()*(__shownGame.hd_on()?__shownGame.hd_scale():1)]);
    await h.closeSettings(page);
    assert.deepEqual(await page.evaluate(()=>{const c=document.querySelector('#screen');return[c.width,c.height];}),expectedSize,'game framebuffer restored on close');
    const closed=await observed(page);await h.runFrames(page,1);
    assert.ok((await observed(page)).ticks-closed.ticks<=4,'closing does not catch up paused wall time');
    assert.equal(await page.evaluate(()=>__shownGame.keys_pending()),0,'no queued menu keys leak');
    await page.keyboard.press('F10');await page.keyboard.press('Escape');
    assert.equal(await page.evaluate(()=>u6Settings.open),false,'reentry closes cleanly');
    assert.deepEqual(errors,[],'no page errors');
    results.push({layout,width,height,pausedFrames:660,canvas:true,blockedEngineCalls:blockedCalls,gameIdentityAndObservedStateStable:true,targets44:true,termsNoOverlap:true,framebufferRestored:true});
    console.log(`PASS canvas settings ${layout} ${width}x${height}`);await context.close();
  }
  // Browser storage can be denied independently of canvas controls. Both preferences must
  // remain reversible within this visit, and an unavailable optional menu font stays readable.
  {
    const {page,context}=await h.openPage(env,{layout:'original'});
    await context.addInitScript(()=>{
      const original=Storage.prototype.setItem;
      Storage.prototype.setItem=function(key,value){if(key==='u6.memory'||key==='u6.motion')throw new DOMException('blocked','SecurityError');return original.call(this,key,value);};
      window.__menuHangulFallback=0;
      const fill=CanvasRenderingContext2D.prototype.fillText;
      CanvasRenderingContext2D.prototype.fillText=function(text,...args){if(/[가-힣]/.test(text))__menuHangulFallback++;return fill.call(this,text,...args);};
    });
    await context.route('**/fonts/galmuri7-hangul.bin',route=>route.abort());
    await h.load(page,env);await page.keyboard.press('F10');
    await h.chooseSetting(page,'language','ko');
    await page.waitForFunction(()=>u6Settings.fontState==='fallback');
    assert.ok(await page.evaluate(()=>__menuHangulFallback>0),'missing menu bitmap falls back to readable Korean glyphs');
    for(const id of ['memory','motion'])for(const value of ['on','off','on']){
      await h.chooseSetting(page,id,value);
      assert.equal((await h.settingsItem(page,id)).value,value,'blocked storage leaves a reversible visit preference');
    }
    await h.closeSettings(page);await context.close();
    results.push({storageFailure:'visit preferences reversible',fontFailure:'readable Korean fallback'});
    console.log('PASS settings storage and menu-font failure');
  }
  // The actual consent page must paint its punctuation, not silently replace it with '?'.
  {
    const {page,context}=await h.openPage(env,{layout:'original'});
    await page.setViewportSize({width:1024,height:900});await h.load(page,env);await h.clickSetting(page,'voice-terms');
    const audit=pageNo=>page.evaluate(({restrictions,pageNo})=>{
      const c=document.querySelector('#screen'),r=c.getBoundingClientRect(),pixels=c.getContext('2d').getImageData(0,0,c.width,c.height).data;
      const widthOf=character=>({' ':3,I:3,'1':3,'.':1,':':1,'-':3,'+':3,'(':2,')':2,'<':3,'>':3,'?':4,'!':1,',':1,"'":1,';':2,'=':3}[character.toUpperCase()]??5);
      const measure=value=>[...value].reduce((total,c)=>total+(widthOf(c==='’'?"'":c)+1)*2,0);
      const margin=Math.max(12,(r.width-680)/2),room=r.width-2*margin,lines=[];
      for(const paragraph of ('About 400 MB is kept in this browser. Dialogue stays on this device.\n\n'+restrictions).split('\n')){
        let line='';for(const word of paragraph.split(' ')){const next=line?line+' '+word:word;if(measure(next)>room&&line){lines.push(line);line=word;}else line=next;while(measure(line)>room){let n=1;while(n<line.length&&measure(line.slice(0,n+1))<=room)n++;lines.push(line.slice(0,n));line=line.slice(n);}}lines.push(line);
      }
      const matches=(rows,x,y,ink)=>rows.every((row,v)=>[...row].every((bit,u)=>{
        const px=Math.floor((x+u*2+1)*c.width/r.width),py=Math.floor((y+v*2+1)*c.height/r.height),at=(py*c.width+px)*4;
        const expected=bit==='1'?ink:16;return pixels[at]===expected&&pixels[at+1]===expected&&pixels[at+2]===expected;
      }));
      const accept=u6Settings.items.find(item=>item.id==='terms-accept'),x=accept.rect[0]-r.left+12+measure('Agree '),y=accept.rect[1]-r.top+15;
      const ampersand=matches(['01100','10010','10100','01000','10101','10010','01101'],x,y,244);
      const capacity=Math.max(1,Math.floor((r.height-264)/22)),first=lines.findIndex(line=>line.includes(';')),targetPage=Math.floor(first/capacity);
      const localLine=first-pageNo*capacity,offset=lines[first].indexOf(';');
      const semicolon=pageNo===targetPage&&matches(['00','01','00','00','01','01','10'],margin+measure(lines[first].slice(0,offset)),100+localLine*22,180);
      return {ampersand,semicolon,targetPage};
    },{restrictions:RESTRICTIONS,pageNo});
    let result=await audit(0);assert.equal(result.ampersand,true,'consent ampersand has its own painted bitmap');
    for(let n=0;n<result.targetPage;n++)await click(page,'terms-next');
    result=await audit(result.targetPage);assert.equal(result.semicolon,true,'verbatim licence semicolon has its own painted bitmap');
    if(out)await page.locator('#screen').screenshot({path:join(out,'terms-punctuation.png')});
    await click(page,'terms-decline');await h.closeSettings(page);await context.close();
    results.push({termsAmpersand:true,termsSemicolon:true,modelLoaded:false});
    console.log('PASS actual consent punctuation bitmaps');
  }
  // Stub only the model worker transport. The real menu, consent handler, player state
  // machine and asynchronous callbacks still run; no model or private dialogue is sent.
  {
    const {page,context}=await h.openPage(env,{layout:'original'});
    await context.addInitScript(()=>{
      window.Worker=class extends EventTarget {
        constructor(){super();window.__settingsVoiceWorker=this;}
        postMessage(message){if(message.type==='load')setTimeout(()=>this.onmessage?.(new MessageEvent('message',{data:{type:'progress',got:1,total:2}})),0);}
        terminate(){}
      };
    });
    await h.load(page,env);await page.keyboard.press('F10');
    await h.clickSetting(page,'voices');
    await click(page,'terms-accept');
    await page.waitForFunction(()=>document.body.dataset.voice==='loading');
    await page.evaluate(()=>__settingsVoiceWorker.onmessage(new MessageEvent('message',{data:{type:'ready',ms:1,refetched:0,path:'cpu'}})));
    await page.waitForFunction(()=>document.body.dataset.voice==='ready');
    assert.equal(await page.evaluate(()=>u6Settings.notice),'Voice ready.','asynchronous ready notice reaches canvas');
    assert.equal((await h.settingsItem(page,'voices')).valueLabel,'Ready','asynchronous ready label reaches canvas');
    await page.evaluate(()=>__settingsVoiceWorker.onmessage(new MessageEvent('message',{data:{type:'error',code:'download'}})));
    await page.waitForFunction(()=>u6Settings.items.find(i=>i.id==='voices')?.valueLabel==='Failed / retry');
    assert.match(await page.evaluate(()=>u6Settings.notice),/Voice failed: download/,'asynchronous failure reaches canvas');
    await page.evaluate(()=>__fail.add('forget'));
    await h.clickSetting(page,'forget-files');await click(page,'confirm-accept');await h.waitSettled(page);
    assert.equal(await page.evaluate(()=>u6Settings.notice),'Could not remove data.','failed confirmation action is awaited and reported');
    assert.equal(await page.evaluate(()=>document.body.dataset.feedback),'error','failure cannot become a ready summary');
    assert.equal(await page.evaluate(()=>document.querySelector('#load-details').open),true,'failure details stay open');
    await page.evaluate(()=>__fail.delete('forget'));
    await h.clickSetting(page,'forget-files');await click(page,'confirm-accept');await h.waitSettled(page);
    assert.equal(await page.evaluate(()=>u6Settings.notice),'Selected data removed.','successful confirmation action is awaited and reported');
    await h.closeSettings(page);
    assert.equal(await page.evaluate(()=>document.querySelector('#load-details').open),false,'successful recovery collapses technical details');
    await h.dropFiles(page,[],[{name:'test.u6lp',text:''},{name:'extra.txt',text:''}]);await h.waitSettled(page);
    assert.equal(await page.evaluate(()=>document.body.dataset.feedback),'error','translated refusal is a semantic error');
    assert.equal(await page.evaluate(()=>document.querySelector('#load-details').open),true,'mixed-file refusal remains visible');
    await context.close();
    results.push({voiceWorker:'synthetic transport',asyncReady:true,asyncFailure:true,deleteFailure:true,deleteSuccess:true});
    console.log('PASS asynchronous voice and delete outcomes');
  }
  if(out)await writeFile(join(out,'settings.json'),JSON.stringify(results,null,2));
}finally{await env.close();}
