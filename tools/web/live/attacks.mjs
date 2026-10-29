// Local-only receipt. Runtime saves are made by write_runtime_attack_fixtures from the
// owner's archive; no original data, saved names or world positions are logged.
// U6_ATTACK_FIXTURE=/private/fixture U6_ATTACK_OUT=/private/receipt node tools/web/live/attacks.mjs
import assert from 'node:assert/strict';
import {mkdir,readdir,readFile,realpath,writeFile} from 'node:fs/promises';
import {existsSync} from 'node:fs';
import {dirname,join} from 'node:path';
import * as h from '../harness.mjs';
const fixture=process.env.U6_ATTACK_FIXTURE, output=process.env.U6_ATTACK_OUT;
assert.ok(fixture && output,'private fixture and output required');
await mkdir(output,{recursive:true});const out=await realpath(output);
for(let p=out;;p=dirname(p)){assert.ok(!existsSync(join(p,'.git')),'receipt must stay outside Git');if(dirname(p)===p)break;}
const env=await h.setup();const results=[];
try {
 for(const layout of ['hd','wide'])for(const mode of ['melee','ranged']){
  const {page,context}=await h.openPage(env,{layout,motion:'on',font:'pixel'});const errors=[];
  page.on('pageerror',e=>errors.push(e.name));await h.load(page,env);
  const extra=[];for(const name of await readdir(join(fixture,mode,'savegame'))){extra.push({name:'PLAYER/SAVEGAME/'+name,bytes:[...await readFile(join(fixture,mode,'savegame',name))]});}
  await h.dropFiles(page,[h.ZIP],extra);await h.waitSettled(page);await h.waitPrompt(page);
  await page.evaluate(()=>__shownGame.set_footstep_pattern(2));
  await h.virtualDisplay(page,120);await page.focus('#screen');
  await page.keyboard.press('a');await h.runFrames(page,180);
  assert.equal(await page.evaluate(()=>document.querySelector('#screen').dataset.prompt),'0','Attack asks for target');
  await page.keyboard.press('ArrowDown');await h.runFrames(page,12);await page.keyboard.press('Enter');
  const active=await page.evaluate(()=>{
   const g=__shownGame,v=__display;const seen=[];let zeros=0,last=[],priorPixels=null,different=0;
   for(let i=0;i<1200;i++){
    const ticks=__ticks;v.run();const p=Array.from(g.attack_offsets());
    if(p.length){seen.push(p);if(__ticks===ticks&&JSON.stringify(p)!==JSON.stringify(last))zeros++;
     const c=document.querySelector('#screen'),d=c.getContext('2d').getImageData(0,0,c.width,c.height).data;
     let hash=2166136261;for(let n=0;n<d.length;n+=4)hash=Math.imul(hash^d[n],16777619);
     if(priorPixels!==null&&priorPixels!==hash)different++;priorPixels=hash;
     if(p.some((x,n)=>n%3===2&&Math.abs(x)>=(Math.max(...p.filter((_,n)=>n%3===2).map(Math.abs))>=5?6:3))&&zeros>0&&different>0){return{seen,zeros,different,active:true};}
    }last=p;
   }return{seen,zeros,different,active:false};
  });
  const poses=active.seen.flatMap(p=>Array.from({length:p.length/3},(_,i)=>p.slice(i*3,i*3+3))).filter(p=>p[0]===1);
  assert.ok(poses.length>0 && active.zeros>0 && active.different>0,'actual canvas changes on zero-step attack frames');
  assert.ok(poses.every(p=>p[1]===0&&(mode==='ranged'?p[2]<0:p[2]>0)),'lunge toward target, recoil away');
  await page.locator('#screen-box').screenshot({path:join(out,`${layout}-${mode}-active.png`)});
  // Settings opens during a live envelope; later frames must not recreate the cue.
  await page.locator('#settings').click();await h.runFrames(page,240);
  assert.deepEqual(await page.evaluate(()=>Array.from(__shownGame.attack_offsets())),[],'Settings clears and suppresses');
  await page.locator('#setup-ok').click();await h.runFrames(page,120);
  assert.deepEqual(await page.evaluate(()=>Array.from(__shownGame.attack_offsets())),[],'no replay after closing');
  await page.locator('#screen-box').screenshot({path:join(out,`${layout}-${mode}-rest.png`)});
  assert.deepEqual(errors,[]);
  results.push({layout,mode,actualCommand:true,zeroStepChanges:active.zeros,canvasChanges:active.different,settingsClears:true,returnToRest:true,pageErrors:0});
  console.log(JSON.stringify(results.at(-1)));await context.close();
 }
 await writeFile(join(out,'receipt.json'),JSON.stringify(results,null,2));
}finally{await env.close()}
