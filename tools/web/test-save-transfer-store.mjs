// Real IndexedDB in disposable browser contexts; synthetic files only, no game or model.
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {dirname,join,resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {chromium} from 'playwright';
import {serve} from './serve.mjs';

const ROOT=resolve(dirname(fileURLToPath(import.meta.url)),'../..');
const output=process.env.U6_SAVE_STORE_OUT??resolve(ROOT,'../evidence/save-transfer/store');
await mkdir(output,{recursive:true});
const server=await serve({root:join(ROOT,'site')});
const browser=await chromium.launch();
const cases=[],results=[];
const test=(name,fn)=>cases.push({name,fn});

async function pageFixture() {
  const context=await browser.newContext();
  await context.route('**/*',async route=>{
    const url=new URL(route.request().url());
    assert.equal(url.origin,new URL(server.url).origin,'storage tests only use loopback');
    if(url.pathname==='/')return route.fulfill({contentType:'text/html',body:'<!doctype html><title>Save store test</title>'});
    return route.continue();
  });
  const page=await context.newPage();await page.goto(server.url);
  await page.evaluate(async()=>{
    window.S=await import('/store.js');
    const ok=(value,label)=>{if(!value)throw new Error(label);};
    const plain=files=>files.map(f=>({path:f.path,bytes:Array.from(f.bytes)})).sort((a,b)=>a.path.toLowerCase()<b.path.toLowerCase()?-1:a.path.toLowerCase()>b.path.toLowerCase()?1:0);
    const same=(a,b,label)=>ok(JSON.stringify(a)===JSON.stringify(b),label);
    const files=n=>[{path:'savegame/objlist',bytes:new Uint8Array([n,2,3])},{path:'savegame/objblkaa',bytes:new Uint8Array([4,n])}];
    const raw=async(mode,op)=>{
      const db=await new Promise((resolve,reject)=>{const r=indexedDB.open('ultima6-port',1);r.onupgradeneeded=()=>r.result.createObjectStore('files');r.onsuccess=()=>resolve(r.result);r.onerror=()=>reject(r.error);});
      try{return await new Promise((resolve,reject)=>{const t=db.transaction('files',mode),s=t.objectStore('files');let result;op(s,v=>result=v);t.oncomplete=()=>resolve(result);t.onabort=t.onerror=()=>reject(t.error??new Error('fixture transaction aborted'));});}
      finally{db.close();}
    };
    const dump=()=>raw('readonly',(s,done)=>{const out=[];const r=s.openCursor();r.onsuccess=()=>{const c=r.result;if(!c){done(out);return;}out.push([c.key,c.value]);c.continue();};});
    const reject=async(fn,name)=>{try{await fn();}catch(e){if(name)ok(e.name===name,`expected ${name}, got ${e.name}`);return e.name;}throw new Error('operation unexpectedly succeeded');};
    window.T={ok,same,plain,files,raw,dump,reject};
  });
  return{context,page};
}

test('strict snapshot and atomic replacement APIs are exported',page=>page.evaluate(()=>{
  for(const key of ['readSaveSnapshot','replaceSavesWithBackup','listSaveBackups','loadSaveBackup'])T.ok(typeof S[key]==='function',`missing API: ${key}`);
}));
test('first import into empty storage creates no unnecessary backup',page=>page.evaluate(async()=>{
  const {same,plain,files}=T;
  same(await S.readSaveSnapshot(),{files:[],epoch:0},'empty strict snapshot');
  same(await S.replaceSavesWithBackup(files(1),[]),{changed:true,backupId:null,epoch:1},'first import result');
  same(plain((await S.readSaveSnapshot()).files),plain(files(1)),'imported native files');
  same(await S.listSaveBackups(),[],'no previous state to back up');
}));
test('replacement preserves every earlier backup and returns newest metadata first',page=>page.evaluate(async()=>{
  const {same,plain,files,ok}=T;
  await S.storeSaves(files(1),true);
  const nativeNow=Date.now;let first,second,third;
  try{
    Date.now=()=>1700000000000;
    first=await S.replaceSavesWithBackup(files(2),(await S.readSaveSnapshot()).files);
    second=await S.replaceSavesWithBackup(files(3),(await S.readSaveSnapshot()).files);
    Date.now=()=>1699999999000;
    third=await S.replaceSavesWithBackup(files(4),(await S.readSaveSnapshot()).files);
  }finally{Date.now=nativeNow;}
  ok(first.changed&&second.changed&&first.backupId!==second.backupId,'distinct backup identities');
  same(plain(await S.loadSaveBackup(first.backupId)),plain(files(1)),'first backup immutable');
  same(plain(await S.loadSaveBackup(second.backupId)),plain(files(2)),'second backup retained');
  same(plain(await S.loadSaveBackup(third.backupId)),plain(files(3)),'third backup retained after clock rollback');
  const list=await S.listSaveBackups();
  same(list.map(x=>x.id),[third.backupId,second.backupId,first.backupId],'newest first, including equal and backwards timestamps');
  ok(list.every(x=>Number.isSafeInteger(x.createdAt)&&x.fileCount===2&&x.totalBytes===5),'backup metadata');
  same(plain((await S.readSaveSnapshot()).files),plain(files(4)),'latest save active');
}));
test('repeated identical import is a no-op and keeps backup records byte-for-byte',page=>page.evaluate(async()=>{
  const {same,files,dump}=T;
  await S.storeSaves(files(1),true);await S.replaceSavesWithBackup(files(2),files(1));
  const before=await dump();const candidate=files(2).reverse().map(f=>({...f,path:f.path.toUpperCase()}));
  same(await S.replaceSavesWithBackup(candidate,files(2)),{changed:false,backupId:null,epoch:2},'normalized identical import result');
  same(await dump(),before,'no-op writes nothing');
}));
test('a different tab changing save after preview produces conflict without any write',async(page,context)=>{
  const preview=await page.evaluate(async()=>{await S.storeSaves(T.files(1),true);return T.plain((await S.readSaveSnapshot()).files);});
  const other=await context.newPage();await other.goto(server.url);
  await other.evaluate(async()=>{const s=await import('/store.js');await s.storeSaves([{path:'savegame/objlist',bytes:new Uint8Array([9,9])}]);});
  await page.evaluate(async preview=>{
    const before=await T.dump();
    await T.reject(()=>S.replaceSavesWithBackup(T.files(2),preview.map(f=>({...f,bytes:new Uint8Array(f.bytes)}))),'SaveConflictError');
    T.same(await T.dump(),before,'conflict changes neither current save nor backups');
  },preview);await other.close();
});
test('a stale game in another tab cannot partially save after import committed',async(page,context)=>{
  await page.evaluate(async()=>{
    await S.storeSaves(T.files(1),true);
    const loaded=await S.readSaveSnapshot();T.ok(Number.isSafeInteger(loaded.epoch),'boot snapshot includes epoch');window.ownerEpoch=loaded.epoch;
  });
  const importer=await context.newPage();await importer.goto(server.url);
  await importer.evaluate(async()=>{
    const s=await import('/store.js'),preview=await s.readSaveSnapshot();
    await s.replaceSavesWithBackup([{path:'savegame/objlist',bytes:new Uint8Array([20])},{path:'savegame/objblkaa',bytes:new Uint8Array([21])}],preview.files,preview.epoch);
  });
  await page.evaluate(async()=>{
    const before=await T.dump();
    await T.reject(()=>S.storeSaves([{path:'savegame/objlist',bytes:new Uint8Array([99])}],false,window.ownerEpoch),'SaveConflictError');
    T.same(await T.dump(),before,'stale normal save cannot mix old objlist with imported blocks or change backups');
  });await importer.close();
});
test('adopting the exact imported snapshot permits normal partial saves without changing its epoch',page=>page.evaluate(async()=>{
  const initial=await S.readSaveSnapshot();
  const installed=await S.replaceSavesWithBackup(T.files(1),initial.files,initial.epoch);
  const owner=await S.readSaveSnapshot();T.same(owner.epoch,installed.epoch,'committed epoch matches attached snapshot');
  const changed=[{path:'savegame/objlist',bytes:new Uint8Array([33])}];
  T.same(await S.storeSaves(changed,false,owner.epoch),{epoch:owner.epoch},'normal save retains ownership epoch');
  const after=await S.readSaveSnapshot();T.same(after.epoch,owner.epoch,'normal partial writes do not invalidate owner');
  T.same(T.plain(after.files),T.plain([...changed,T.files(1)[1]]),'current epoch saves native partial block updates');
}));
test('first import invalidates an earlier unsaved game and stale forget cannot delete it',page=>page.evaluate(async()=>{
  const empty=await S.readSaveSnapshot();
  const imported=await S.replaceSavesWithBackup(T.files(1),empty.files,empty.epoch),before=await T.dump();
  T.ok(imported.epoch>empty.epoch,'first import advances even without backup');
  await T.reject(()=>S.storeSaves(T.files(9),false,empty.epoch),'SaveConflictError');
  await T.reject(()=>S.forgetSaves(empty.epoch),'SaveConflictError');
  T.same(await T.dump(),before,'neither stale new-game save nor stale deletion can change imported state');
}));
test('full-folder adoption and forgetting invalidate older owners and return the next epoch',page=>page.evaluate(async()=>{
  const old=await S.readSaveSnapshot();
  const adopted=await S.storeSaves(T.files(1),true,old.epoch);
  T.same(adopted,{epoch:old.epoch+1},'legacy folder import returns new epoch');
  const before=await T.dump();await T.reject(()=>S.storeSaves(T.files(9),false,old.epoch),'SaveConflictError');
  T.same(await T.dump(),before,'folder replacement rejects former owner without any write');
  const forgotten=await S.forgetSaves(adopted.epoch);
  T.same(forgotten,{epoch:adopted.epoch+1},'forget returns new epoch');
  T.same(await S.readSaveSnapshot(),{files:[],epoch:forgotten.epoch},'forget atomically empties save and advances ownership');
  const empty=await T.dump();await T.reject(()=>S.storeSaves(T.files(9),false,adopted.epoch),'SaveConflictError');
  T.same(await T.dump(),empty,'forgotten save cannot be resurrected by former owner');
  T.same(await S.storeSaves(T.files(2),false,forgotten.epoch),forgotten,'explicit new start adopts forget result');
}));
test('preview epoch detects an intervening replacement even when bytes return to the preview',page=>page.evaluate(async()=>{
  await S.storeSaves(T.files(1),true);const preview=await S.readSaveSnapshot();
  await S.replaceSavesWithBackup(T.files(2),preview.files,preview.epoch);
  const current=await S.readSaveSnapshot();await S.replaceSavesWithBackup(preview.files,current.files,current.epoch);
  const before=await T.dump();
  await T.reject(()=>S.replaceSavesWithBackup(T.files(3),preview.files,preview.epoch),'SaveConflictError');
  await T.reject(()=>S.storeSaves(T.files(9),false,preview.epoch),'SaveConflictError');
  T.same(await T.dump(),before,'ABA conflict preserves active save, epoch and backup history');
}));
test('normal save and import compete within one transaction so neither can mix generations',page=>page.evaluate(async()=>{
  await S.storeSaves(T.files(1),true);const owner=await S.readSaveSnapshot();
  const imported=T.files(2),normal=[{path:'savegame/objlist',bytes:new Uint8Array([99])}];
  const outcomes=await Promise.allSettled([S.replaceSavesWithBackup(imported,owner.files,owner.epoch),S.storeSaves(normal,false,owner.epoch)]);
  T.same(outcomes.map(x=>x.status).sort(),['fulfilled','rejected'],'exactly one competing operation commits');
  T.same(outcomes.find(x=>x.status==='rejected').reason.name,'SaveConflictError','losing operation detects either byte or epoch conflict');
  const after=await S.readSaveSnapshot();
  if(outcomes[0].status==='fulfilled')T.same(T.plain(after.files),T.plain(imported),'import won with no old blocks');
  else T.same(T.plain(after.files),T.plain([...normal,T.files(1)[1]]),'normal save won with original-generation blocks');
}));
test('malformed or exhausted epochs reject without mutating active saves or backups',page=>page.evaluate(async()=>{
  await S.storeSaves(T.files(1),true);const initial=await T.dump();
  for(const epoch of [null,-1,1.5,NaN,'1']){
    await T.reject(()=>S.storeSaves(T.files(2),false,epoch),'TypeError');
    await T.reject(()=>S.replaceSavesWithBackup(T.files(2),T.files(1),epoch),'TypeError');
    await T.reject(()=>S.forgetSaves(epoch),'TypeError');
  }
  T.same(await T.dump(),initial,'invalid caller epochs never write');
  for(const value of [null,-1,'1',Number.MAX_SAFE_INTEGER]){
    await T.raw('readwrite',(s,done)=>{s.put(value,'save-epoch');done(null);});const before=await T.dump();
    if(value!==Number.MAX_SAFE_INTEGER)await T.reject(()=>S.readSaveSnapshot(),'TypeError');
    await T.reject(()=>S.replaceSavesWithBackup(T.files(2),T.files(1)));
    await T.reject(()=>S.storeSaves(T.files(2),true));
    await T.reject(()=>S.forgetSaves());
    T.same(await T.dump(),before,'corrupt/exhausted epoch aborts all replacement writes');
  }
}));
for(const fault of ['abort','quota','throw','request-error'])test(`failure after a backup request rolls back all records: ${fault}`,page=>page.evaluate(async fault=>{
  await S.storeSaves(T.files(1),true);
  await T.raw('readwrite',(s,done)=>{s.put({marker:17},'unrelated');done(null);});
  const before=await T.dump();const native=IDBObjectStore.prototype.add;let injected=false;
  IDBObjectStore.prototype.add=function(value,key){
    const request=native.call(this,value,key);
    if(!injected&&String(key).startsWith('save-backup:')){
      injected=true;
      if(fault==='abort')this.transaction.abort();
      else if(fault==='quota')throw new DOMException('synthetic quota exhaustion','QuotaExceededError');
      else if(fault==='throw')throw new Error('synthetic throw after backup request');
      else native.call(this,{marker:18},'unrelated'); // actual IndexedDB ConstraintError
    }
    return request;
  };
  try{await T.reject(()=>S.replaceSavesWithBackup(T.files(2),T.files(1)));}
  finally{IDBObjectStore.prototype.add=native;}
  T.ok(injected,'failure was injected after a real backup request');
  T.same(await T.dump(),before,'save and all backup records rolled back');
},fault));
test('strict reads distinguish unavailable storage from an empty save',page=>page.evaluate(async()=>{
  T.same(await S.readSaveSnapshot(),{files:[],epoch:0},'empty save is a successful read');
  const native=IDBDatabase.prototype.transaction;
  IDBDatabase.prototype.transaction=function(){throw new DOMException('synthetic storage unavailable','InvalidStateError');};
  try{
    await T.reject(()=>S.readSaveSnapshot(),'InvalidStateError');
    await T.reject(()=>S.listSaveBackups(),'InvalidStateError');
  }finally{IDBDatabase.prototype.transaction=native;}
}));
test('unknown or unreadable backup rejects instead of returning empty files',page=>page.evaluate(async()=>{
  await T.reject(()=>S.loadSaveBackup('missing'));
  await T.reject(()=>S.loadSaveBackup('1-00000000-0000-4000-8000-000000000000'),'SaveBackupNotFoundError');
  await S.storeSaves(T.files(1),true);const {backupId}=await S.replaceSavesWithBackup(T.files(2),T.files(1));
  const native=IDBDatabase.prototype.transaction;
  IDBDatabase.prototype.transaction=function(){throw new DOMException('synthetic unreadable backup','InvalidStateError');};
  try{await T.reject(()=>S.loadSaveBackup(backupId),'InvalidStateError');}finally{IDBDatabase.prototype.transaction=native;}
}));
test('a backup identity collision cannot overwrite existing history or current save',page=>page.evaluate(async()=>{
  const nativeNow=Date.now,nativeUuid=crypto.randomUUID;
  Date.now=()=>1700000000000;crypto.randomUUID=()=> '00000000-0000-4000-8000-000000000000';
  try{
    await S.storeSaves(T.files(1),true);await S.replaceSavesWithBackup(T.files(2),T.files(1));
    const before=await T.dump();await T.reject(()=>S.replaceSavesWithBackup(T.files(3),T.files(2)),'ConstraintError');
    T.same(await T.dump(),before,'identity collision preserves all records');
  }finally{Date.now=nativeNow;crypto.randomUUID=nativeUuid;}
}));
test('simultaneous imports sharing one preview serialize to one commit and one conflict',page=>page.evaluate(async()=>{
  await S.storeSaves(T.files(1),true);
  const outcomes=await Promise.allSettled([S.replaceSavesWithBackup(T.files(2),T.files(1)),S.replaceSavesWithBackup(T.files(3),T.files(1))]);
  T.same(outcomes.map(x=>x.status).sort(),['fulfilled','rejected'],'only one commit');
  T.ok(outcomes.find(x=>x.status==='rejected').reason.name==='SaveConflictError','second transaction detects committed replacement');
  const list=await S.listSaveBackups();T.ok(list.length===1,'one immutable backup');
  T.same(T.plain(await S.loadSaveBackup(list[0].id)),T.plain(T.files(1)),'original preview retained');
}));
test('malformed current records fail strict snapshot rather than masking data as empty',page=>page.evaluate(async()=>{
  await T.raw('readwrite',(s,done)=>{s.put({path:'savegame/objlist',bytes:[1,2]},'save:savegame/objlist');done(null);});
  const before=await T.dump();await T.reject(()=>S.readSaveSnapshot(),'TypeError');
  await T.reject(()=>S.replaceSavesWithBackup(T.files(2),[]),'TypeError');
  T.same(await T.dump(),before,'malformed existing records are preserved');
}));
test('missing backup payload is rejected and normal save deletion preserves all backups',page=>page.evaluate(async()=>{
  await S.storeSaves(T.files(1),true);const {backupId}=await S.replaceSavesWithBackup(T.files(2),T.files(1));
  await S.forgetSaves();T.same((await S.readSaveSnapshot()).files,[],'ordinary delete clears active save');
  T.same(T.plain(await S.loadSaveBackup(backupId)),T.plain(T.files(1)),'ordinary delete preserves backup');
  await T.raw('readwrite',(s,done)=>{
    const r=s.openCursor();r.onsuccess=()=>{const c=r.result;if(!c){done(null);return;}if(String(c.key).startsWith('save-backup-files:')){c.delete();done(null);}else c.continue();};
  });
  await T.reject(()=>S.loadSaveBackup(backupId),'TypeError');
}));
test('imports preserve unrelated game, pack and model records and ordinary saves still work',page=>page.evaluate(async()=>{
  const keys=['extracted','hd:pack','locale:ko','locale:ko:source','voice-pack:voices','voice-pack:source','npc-bundle:7','models:kept'];
  await T.raw('readwrite',(s,done)=>{for(const key of keys)s.put({marker:key,bytes:new Uint8Array([4,5])},key);done(null);});
  const before=(await T.dump()).filter(([key])=>keys.includes(key));
  await S.replaceSavesWithBackup(T.files(1),[]);
  const changed=[{path:'savegame/objlist',bytes:new Uint8Array([5])}];await S.storeSaves(changed);
  const expected=[...changed,T.files(1)[1]].sort((a,b)=>a.path.localeCompare(b.path));
  T.same(T.plain((await S.readSaveSnapshot()).files),T.plain(expected),'normal partial save after import');
  T.same((await T.dump()).filter(([key])=>keys.includes(key)),before,'unrelated namespaces unchanged');
}));
test('legacy unknown paths and original case survive snapshots and backups unchanged',page=>page.evaluate(async()=>{
  const old=[{path:'SaveGame/ObjList',bytes:new Uint8Array([3])},{path:'legacy\\unknown:data',bytes:new Uint8Array([7,8])}];
  await S.storeSaves(old,true);
  const snapshot=await S.readSaveSnapshot();
  T.same(T.plain(snapshot.files).sort((a,b)=>a.path.localeCompare(b.path)),T.plain(old).sort((a,b)=>a.path.localeCompare(b.path)),'snapshot preserves old paths');
  const {backupId}=await S.replaceSavesWithBackup(T.files(1),snapshot.files);
  T.same(T.plain(await S.loadSaveBackup(backupId)).sort((a,b)=>a.path.localeCompare(b.path)),T.plain(old).sort((a,b)=>a.path.localeCompare(b.path)),'backup preserves old paths and bytes');
}));
test('candidate validation rejects traversal, aliases, unknown leaves and duplicate names without writes',page=>page.evaluate(async()=>{
  await S.storeSaves(T.files(1),true);const before=await T.dump();
  for(const path of ['../savegame/objlist','/savegame/objlist','savegame\\objlist','savegame/../objlist','savegame/objlist\u0000','savegame/objblkfi','savegame/unknown','SAVEGAME/OBJLİST']){
    await T.reject(()=>S.replaceSavesWithBackup([{path,bytes:new Uint8Array([1])}],T.files(1)));
  }
  await T.reject(()=>S.replaceSavesWithBackup([{path:'savegame/objlist',bytes:[1]}],T.files(1)));
  await T.reject(()=>S.replaceSavesWithBackup([{path:'savegame/objlist',bytes:new Uint8Array([1])},{path:'SAVEGAME/OBJLIST',bytes:new Uint8Array([2])}],T.files(1)));
  T.same(await T.dump(),before,'invalid candidates change nothing');
}));
test('input snapshots and returned backup bytes do not alias stored data',page=>page.evaluate(async()=>{
  const old=T.files(1);await S.storeSaves(old,true);
  const candidate=T.files(2),expected=(await S.readSaveSnapshot()).files;
  const operation=S.replaceSavesWithBackup(candidate,expected);
  candidate[0].bytes.fill(99);expected[0].bytes.fill(98);
  const {backupId}=await operation;
  T.same(T.plain((await S.readSaveSnapshot()).files),T.plain(T.files(2)),'candidate copied before await');
  const loaded=await S.loadSaveBackup(backupId);loaded[0].bytes.fill(77);
  T.same(T.plain(await S.loadSaveBackup(backupId)),T.plain(old),'loaded backup cannot mutate persistent bytes');
}));
test('listing reads metadata with a cursor rather than loading every backup payload',page=>page.evaluate(async()=>{
  await S.storeSaves(T.files(1),true);await S.replaceSavesWithBackup(T.files(2),T.files(1));
  const nativeAll=IDBObjectStore.prototype.getAll,nativeCursor=IDBObjectStore.prototype.openCursor;const ranges=[];
  IDBObjectStore.prototype.getAll=function(){throw new Error('listing must not getAll backup data');};
  IDBObjectStore.prototype.openCursor=function(range,...args){ranges.push([range?.lower,range?.upper]);return nativeCursor.call(this,range,...args);};
  let list;try{list=await S.listSaveBackups();}finally{IDBObjectStore.prototype.getAll=nativeAll;IDBObjectStore.prototype.openCursor=nativeCursor;}
  T.ok(list.length===1&&ranges.length>0,'metadata cursor enumerates backup');
  T.ok(ranges.every(([lo,hi])=>String(lo).startsWith('save-backup:')&&String(hi).startsWith('save-backup:')),'cursor range only contains metadata');
}));

try {
  for(const c of cases){
    if(process.argv[2]&&!c.name.includes(process.argv[2]))continue;
    const {context,page}=await pageFixture();
    try{await c.fn(page,context);results.push({name:c.name,status:'pass'});console.log(`ok - ${c.name}`);}
    catch(error){results.push({name:c.name,status:'fail',error:error.message});console.log(`not ok - ${c.name}\n  ${error.message}`);}
    finally{await context.close();}
  }
}finally{await browser.close();await server.close();}
const failed=results.filter(x=>x.status==='fail').length;
const receipt={at:new Date().toISOString(),storeSha256:createHash('sha256').update(await readFile(join(ROOT,'site/store.js'))).digest('hex'),ran:results.length,passed:results.length-failed,failed,results};
await writeFile(join(output,`${Date.now()}-${failed?'red':'green'}.json`),JSON.stringify(receipt,null,2));
console.log(`# save transfer store: ${receipt.ran} ran, ${receipt.passed} passed, ${failed} failed`);process.exitCode=failed?1:0;
