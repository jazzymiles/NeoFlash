// Runs against the actual compiled Rust WASM module. Build with scripts/build-web.sh first.
const assert = require('node:assert/strict');
const test = require('node:test');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const runtimeDir = path.resolve(__dirname, '../../target/web-runtime');
const context = vm.createContext({TextEncoder, TextDecoder, WebAssembly, console, URL, Request, Response});
vm.runInContext(fs.readFileSync(path.join(runtimeDir, 'neoflash_web.js'),'utf8'), context);
const wasm = vm.runInContext('NeoFlashWasm',context);
context.wasmBytes = fs.readFileSync(path.join(runtimeDir,'neoflash_web_bg.wasm'));
const ready = vm.runInContext('NeoFlashWasm({module_or_path: wasmBytes})', context);
const transform = x => ({x,y:0,scale_x:1,scale_y:1,rotation:0,alpha:1});
const script = source => ({language:'java_script',source});
const shape = x => ({id:'shape',name:'disc',transform:transform(x),kind:{type:'ellipse',width:20,height:20,fill:[220,0,0,255],stroke:[0,0,0,0],stroke_width:0},scripts:[{event:'click',script:script('trace("leaf")')}]});
const instance = (id,x) => ({id,name:id,transform:transform(x),kind:{type:'instance',symbol_id:'clip',playback:'independent'},scripts:[{event:'click',script:script('this.rotation += 15;')}]});
const key = (frame,elements,label='',tween=null) => ({frame,elements,label,tween});
function project() {
  return {format_version:1,name:'WASM verification',width:320,height:200,fps:24,background:[255,255,255,255],root:'root',symbols:[
    {id:'root',name:'Scene',kind:'movie_clip',timeline:{frame_count:4,layers:[{id:'root-layer',name:'Instances',visible:true,locked:false,keyframes:[key(0,[instance('left',0),instance('right',100)],'start'),key(3,[instance('left',0),instance('right',100)],'end')]}],scripts:[{frame:0,script:script('globalThis.__neverExecute = true;')}] }},
    {id:'clip',name:'Nested clip',kind:'movie_clip',timeline:{frame_count:4,layers:[{id:'clip-layer',name:'Drawing',visible:true,locked:false,keyframes:[key(0,[shape(0)],'', 'linear'),key(3,[shape(30)])]}],scripts:[]}},
  ]};
}
test('WASM executes shared independent clocks, labels, hierarchy and transform overrides',async()=>{
  await ready;
  const player=new wasm.WebPlayer(JSON.stringify(project()));
  assert.equal(player.current_frame(),0);
  assert.equal(player.resolve_frame('[]','end'),3);
  assert.equal(JSON.parse(player.child_path('[]','right'))[0],'right');
  assert.equal(player.control('[]','stop',0),true);
  assert.equal(player.control('["left"]','stop',0),true);
  player.tick();
  assert.equal(player.current_frame(),0);
  assert.equal(player.clip_frame('["left"]'),0);
  assert.equal(player.clip_frame('["right"]'),1);
  let scene=JSON.parse(player.scene_json());
  assert.equal(scene.find(item=>item.instance_path[0]==='left').transform[4],0);
  assert.equal(scene.find(item=>item.instance_path[0]==='right').transform[4],110);
  assert.equal(player.set_property('["right"]','x',150),true);
  scene=JSON.parse(player.scene_json());
  assert.equal(scene.find(item=>item.instance_path[0]==='right').transform[4],160);
  assert.equal(player.control('["right"]','goto_and_stop',3),true);
  assert.equal(player.clip_frame('["right"]'),3);
  assert.equal(player.control('["right"]','goto_and_stop',4),false);
  player.set_playing(false); player.reset();
  assert.equal(player.is_playing(),true);
  player.free();
});
test('WASM queues source without executing it and bubbles click from leaf to instance',async()=>{
  await ready;
  const player=new wasm.WebPlayer(JSON.stringify(project()));
  const startup=JSON.parse(player.scripts_json());
  assert.equal(startup.length,1);
  assert.equal(player.is_dispatch_current(JSON.stringify(startup[0])),true);
  assert.equal(globalThis.__neverExecute,undefined);
  player.click('["right","shape"]');
  const events=JSON.parse(player.scripts_json());
  assert.deepEqual(events.map(event=>event.target_path),[['right','shape'],['right']]);
  assert.ok(events.every(event=>event.event==='click'));
  player.free();
});
test('WASM rejects malformed or unsupported project versions',async()=>{
  await ready;
  const invalid=project(); invalid.format_version=987;
  assert.throws(()=>new wasm.WebPlayer(JSON.stringify(invalid)),/unsupported version/);
});
test('WASM changes instance lifetime when a placement disappears and returns', async()=>{
  await ready;
  const data=project();
  data.symbols[0].timeline.layers[0].keyframes.splice(1,0,key(1,[]),key(2,[instance('left',0),instance('right',100)]));
  const player=new wasm.WebPlayer(JSON.stringify(data));
  const before=player.lifetime_id('["left"]');
  assert.notEqual(before,'');
  player.tick();
  assert.equal(player.lifetime_id('["left"]'),'');
  player.tick();
  assert.notEqual(player.lifetime_id('["left"]'),'');
  assert.notEqual(player.lifetime_id('["left"]'),before);
  player.free();
});
test('WASM preserves curved compound paths and nested affine transforms',async()=>{
  await ready;
  const data=project();
  const geometry={type:'path',commands:[
    {op:'move_to',x:0,y:0},{op:'quadratic_to',cx:50,cy:100,x:100,y:0},{op:'close'},
    {op:'move_to',x:30,y:10},{op:'cubic_to',cx1:40,cy1:30,cx2:60,cy2:30,x:70,y:10},{op:'close'},
  ],fill_rule:'evenodd',fill:[255,100,50,255],stroke:[0,0,0,255],stroke_width:3};
  data.symbols[1].timeline.layers[0].keyframes.forEach(key=>{key.elements[0].kind=geometry});
  const right=data.symbols[0].timeline.layers[0].keyframes[0].elements[1];
  right.transform={...right.transform,rotation:90,scale_x:2,scale_y:3};
  const player=new wasm.WebPlayer(JSON.stringify(data));
  const item=JSON.parse(player.scene_json()).find(item=>item.instance_path[0]==='right');
  assert.deepEqual(item.kind,geometry);
  assert.ok(Math.abs(item.transform[0])<1e-10);
  assert.equal(item.transform[1],2);
  assert.equal(item.transform[2],-3);
  assert.equal(item.transform[4],100);
  player.free();
});
test('WASM exposes independent authored targets and validates activation lifetime/disabled state',async()=>{
 await ready;const data=project();
 const meta=name=>({name,description:'Counter',role:'button',tab_order:0,disabled:false});
 for(const keyframe of data.symbols[0].timeline.layers[0].keyframes){keyframe.elements[0].accessibility=meta('Left counter');keyframe.elements[1].accessibility=meta('Right counter');}
 data.symbols[0].timeline.layers[0].keyframes.splice(1,0,key(1,[]),key(2,[{...instance('left',0),accessibility:meta('Left counter')},{...instance('right',100),accessibility:{...meta('Right counter'),disabled:true}}]));
 const player=new wasm.WebPlayer(JSON.stringify(data));player.scripts_json();
 let nodes=JSON.parse(player.accessibility_json());assert.deepEqual(nodes.map(n=>n.accessibility.name),['Left counter','Right counter']);
 const old=nodes[0];assert.equal(player.activate_accessible(JSON.stringify(old.instance_path),old.lifetime_id),true);
 assert.equal(JSON.parse(player.scripts_json()).filter(x=>x.event==='click').length,1);
 player.tick();assert.equal(JSON.parse(player.accessibility_json()).length,0);
 player.tick();nodes=JSON.parse(player.accessibility_json());assert.equal(nodes.length,2);
 assert.equal(player.activate_accessible(JSON.stringify(old.instance_path),old.lifetime_id),false);
 const replacement=nodes.find(n=>n.accessibility.name==='Left counter');assert.notEqual(replacement.lifetime_id,old.lifetime_id);
 assert.equal(player.activate_accessible(JSON.stringify(replacement.instance_path),replacement.lifetime_id),true);
 const disabled=nodes.find(n=>n.accessibility.disabled);assert.equal(player.activate_accessible(JSON.stringify(disabled.instance_path),disabled.lifetime_id),false);
 player.free();
});
test('WASM schedules authored timeline video for the published host boundary',async()=>{
  await ready;
  const video={id:'video',name:'video',transform:transform(0),kind:{type:'video',width:320,height:180},scripts:[]};
  const data={format_version:9,name:'Timeline video',width:320,height:180,fps:10,background:[0,0,0,255],root:'root',
    resources:[{id:'movie',path:'media/imported.mp4',media_type:'video/mp4',data_base64:Buffer.from('\0\0\0\x18ftypisom\0\0\x02\0isomiso2avc1mp41','binary').toString('base64')}],
    symbols:[{id:'root',name:'Scene',kind:'movie_clip',timeline:{frame_count:4,layers:[{id:'video-layer',name:'Video',visible:true,locked:false,keyframes:[
      {...key(0,[video]),video:{resource:'movie',sync:'stream',start_offset_ms:0,loop_video:false,muted:false}},
      key(2,[]),
    ]}],scripts:[]}}]};
  const player=new wasm.WebPlayer(JSON.stringify(data));
  let commands=JSON.parse(player.video_commands_json());
  assert.equal(commands.length,1);assert.equal(commands[0].op,'seek_and_present');
  assert.deepEqual(commands[0].target_path,['video']);assert.equal(commands[0].resource,'movie');assert.equal(commands[0].target_time_ms,0);
  player.tick();commands=JSON.parse(player.video_commands_json());assert.equal(commands[0].target_time_ms,100);
  player.set_playing(false);assert.deepEqual(JSON.parse(player.video_commands_json()).map(c=>c.op),['pause_streams']);
  player.set_playing(true);commands=JSON.parse(player.video_commands_json());assert.equal(commands[0].playing,true);assert.equal(commands[0].target_time_ms,100);
  player.control('[]','goto_and_stop',3);assert.deepEqual(JSON.parse(player.video_commands_json()).map(c=>c.op),['stop_channel']);
  player.reset();assert.deepEqual(JSON.parse(player.video_commands_json()).map(c=>c.op),['stop_all','seek_and_present']);
  player.free();
});
test('WASM timeline sounds reach the Web Audio host as play/stream/stop commands',async()=>{
  await ready;
  const data=JSON.parse(fs.readFileSync(path.resolve(__dirname,'../../examples/timeline-sound.nflash'),'utf8'));
  const player=new wasm.WebPlayer(JSON.stringify(data));
  assert.deepEqual(JSON.parse(player.timeline_sound_resources_json()),['beep','melody']);
  const ops=()=>JSON.parse(player.sound_commands_json()).map(c=>c.op==='stream_start'?`${c.op} ${c.resource} ${c.offset.toFixed(3)}`:c.resource?`${c.op} ${c.resource}`:c.op);
  assert.deepEqual(ops(),['play beep','stream_start melody 0.000']);
  for(let frame=1;frame<24;frame++){player.tick();assert.deepEqual(ops(),[],`frame ${frame}`);}
  player.tick();assert.deepEqual(ops(),['play beep']);
  player.control('[]','goto_and_play',30);assert.deepEqual(ops(),['stream_stop','stream_start melody 1.250']);
  for(let frame=31;frame<36;frame++){player.tick();assert.deepEqual(ops(),[]);}
  player.tick();assert.deepEqual(ops(),['stream_stop'],'the stream ends with its span at frame 37');
  player.control('[]','goto_and_play',10);assert.deepEqual(ops(),['stream_start melody 0.417']);
  player.set_playing(false);assert.deepEqual(ops(),['stream_stop']);
  player.set_playing(true);player.stop_all_sounds();player.tick();assert.deepEqual(ops(),['stop_all','stream_start melody 0.458']);
  // The browser adapter decodes each embedded resource once and schedules silent sources.
  const sources=[],decoded=[];let time=0;
  const param=()=>({value:1,events:[],setValueAtTime(v,t){this.events.push(['set',v,t]);},linearRampToValueAtTime(v,t){this.events.push(['ramp',v,t]);}});
  const node=kind=>({kind,gain:param(),connect(){},disconnect(){}});
  const context={state:'suspended',sampleRate:8000,get currentTime(){return time;},destination:node('destination'),
    resume(){this.state='running';return Promise.resolve();},close(){return Promise.resolve();},
    decodeAudioData(buffer){decoded.push(buffer.byteLength);return Promise.resolve({duration:buffer.byteLength>5000?1.5:0.15,length:1,numberOfChannels:1});},
    createGain(){return node('gain');},createStereoPanner(){return {...node('pan'),pan:param()};},
    createChannelSplitter(){return node('splitter');},createChannelMerger(){return node('merger');},
    createBufferSource(){const source={...node('source'),loop:false,start(when,offset){this.started=[when,offset];},stop(when){this.stopped=when??time;}};sources.push(source);return source;}};
  const resources=new Map(data.resources.map(r=>[r.path,Buffer.from(r.data_base64,'base64')]));
  const audio=require('../audio.js').create({events:require('../events.js'),net:{URLLoader:class{},mediaSource:p=>({bytes:new Uint8Array(resources.get(p))})},binary:{toBytes:x=>x},
    resources:data.resources,createContext:()=>context,isUserGesture:()=>true,now:()=>time,onError:error=>{throw error;}});
  audio.preloadTimeline(['beep','melody']);await new Promise(done=>setTimeout(done,0));
  assert.equal(decoded.length,2,'both resources decode once ahead of their keyframes');
  audio.timeline([{op:'play',resource:'beep',sync:'event',loops:1,loop_forever:false,envelope:[[0,1,1]]}]);
  assert.equal(sources.length,0,'Event sounds before Enable audio are skipped');
  assert.equal(audio.status().state,'awaiting_gesture');
  audio.timeline([{op:'stream_start',channel:'music',resource:'melody',offset:0.25,loops:1,loop_forever:false,envelope:[[0,1,0],[1,0,1]]}]);
  time=0.5;await audio.resumeFromGesture();
  assert.equal(sources.length,1);assert.deepEqual(sources[0].started,[0.5,0.75],'a waiting stream resumes at its timeline position');
  audio.timeline([{op:'play',resource:'beep',sync:'start',loops:2,loop_forever:false,envelope:[[0,0,0],[1,1,1]]}]);
  audio.timeline([{op:'play',resource:'beep',sync:'start',loops:1,loop_forever:false,envelope:[[0,1,1]]}]);
  assert.equal(sources.length,2,'Start sync does not overlap a playing instance');
  assert.equal(sources[1].loop,true);assert.ok(Math.abs(sources[1].stopped-0.8)<1e-9,'two plays of 0.15 s');
  audio.timeline([{op:'stop',resource:'beep'},{op:'stream_stop',channel:'music'}]);
  assert.ok(sources.every(source=>source.stopped!==undefined));assert.equal(audio.status().active,0);
  audio.timeline([{op:'play',resource:'beep',sync:'event',loops:1,loop_forever:false,envelope:[[0,1,1]]},{op:'play',resource:'beep',sync:'event',loops:1,loop_forever:false,envelope:[[0,1,1]]}]);
  assert.equal(audio.status().active,2,'Event sounds overlap');
  audio.timeline([{op:'stop_all'}]);assert.equal(audio.status().active,0);
  audio.dispose();player.free();
});

test('WASM evaluates Bounce/Elastic/per-property motion eases and emits custom stereo sound envelopes',async()=>{
  await ready;
  const base=JSON.parse(fs.readFileSync(path.resolve(__dirname,'../../examples/timeline-sound.nflash'),'utf8'));
  const customEnvelope=[[0,0.2,0.9],[0.5,1,0.3],[1,0,0.75]];
  const doc={
    format_version:7,
    name:'WASM format 7 timeline',
    width:320,
    height:200,
    fps:24,
    background:[255,255,255,255],
    root:'root',
    resources:base.resources.filter(r=>r.id==='beep'),
    symbols:[{
      id:'root',
      name:'Scene 1',
      kind:'movie_clip',
      timeline:{
        frame_count:11,
        layers:[{
          id:'motion-layer',
          name:'Motion',
          visible:true,
          locked:false,
          keyframes:[
            {
              frame:0,
              elements:[shape(0)],
              label:'',
              tween:'linear',
              tween_kind:'motion',
              property_eases:[['x','ease_in'],['y','bounce']],
              sound:{resource:'beep',effect:'custom',envelope:customEnvelope},
            },
            {
              frame:10,
              elements:[{...shape(100),transform:{...transform(100),y:200}}],
              label:'',
              tween:'linear',
              tween_kind:'motion',
              keyed_properties:['x','y'],
            },
          ],
        }],
        scripts:[],
      },
    }],
  };
  const player=new wasm.WebPlayer(JSON.stringify(doc));
  const startupSounds=JSON.parse(player.sound_commands_json());
  assert.equal(startupSounds.length,1);
  assert.equal(startupSounds[0].op,'play');
  assert.deepEqual(startupSounds[0].envelope,customEnvelope);

  for(let i=0;i<5;i++)player.tick();
  assert.equal(player.current_frame(),5);
  const item=JSON.parse(player.scene_json())[0];
  assert.ok(Math.abs(item.transform[4]-25)<1e-6,`X should follow EaseIn at t=0.5, got ${item.transform[4]}`);
  assert.ok(item.transform[5]>150,`Y should follow Bounce (>150 at t=0.5), got ${item.transform[5]}`);
  player.free();
});

test('WASM format 8 curves retain arc-length motion, orientation and authored shape hints',async()=>{
  await ready;
  const doc={format_version:8,name:'Editable timeline paths',width:320,height:240,fps:24,
    background:[255,255,255,255],root:'root',symbols:[{id:'root',name:'Scene',kind:'movie_clip',
    timeline:{frame_count:21,scripts:[],layers:[{id:'motion',name:'Motion',visible:true,locked:false,
      keyframes:[{...key(0,[shape(0)],'','linear'),tween_kind:'motion',orient_to_path:true,
        motion_path:[{op:'move_to',x:0,y:0},{op:'cubic_to',cx1:0,cy1:100,cx2:100,cy2:100,x:100,y:0}]},
        {...key(20,[shape(100)]),keyed_properties:['x','y']},
      ]}]}}]};
  const player=new wasm.WebPlayer(JSON.stringify(doc));
  player.control('[]','goto_and_stop',10);
  const midpoint=JSON.parse(player.scene_json())[0].transform;
  assert.ok(Math.abs(midpoint[4]-50)<0.05,`curve midpoint x: ${midpoint[4]}`);
  assert.ok(Math.abs(midpoint[5]-75)<0.05,`curve midpoint y: ${midpoint[5]}`);
  assert.ok(Math.abs(midpoint[0])<0.2 && midpoint[1]<-0.9,'orientation turns with the curve tangent');
  player.control('[]','goto_and_stop',20);
  assert.deepEqual(JSON.parse(player.scene_json())[0].transform.slice(4),[100,0]);
  player.free();
  const wrongVersion=structuredClone(doc);wrongVersion.format_version=7;
  assert.throws(()=>new wasm.WebPlayer(JSON.stringify(wrongVersion)),/version 8/);

  const polygon={...shape(0),scripts:[],kind:{type:'path',fill_rule:'nonzero',fill:[220,0,0,255],stroke:[0,0,0,0],stroke_width:0,
    commands:[{op:'move_to',x:0,y:0},{op:'line_to',x:100,y:0},{op:'line_to',x:100,y:100},{op:'line_to',x:0,y:100},{op:'close'}]}};
  const morph=structuredClone(doc);morph.format_version=7;
  morph.symbols[0].timeline.layers[0].keyframes=[{...key(0,[polygon],'','linear'),tween_kind:'shape',
    shape_hints:[{label:'a',start_uv:[0,0],end_uv:[1,1]}]},key(20,[polygon])];
  const hinted=new wasm.WebPlayer(JSON.stringify(morph));hinted.control('[]','goto_and_stop',10);
  const commands=JSON.parse(hinted.scene_json())[0].kind.commands;
  assert.ok(commands.some(c=>Math.abs(c.x-50)<1e-6 && Math.abs(c.y-50)<1e-6),
    'the authored correspondence pulls the top-left vertex to the opposite corner midpoint');
  hinted.free();
  delete morph.symbols[0].timeline.layers[0].keyframes[0].shape_hints;
  const unhinted=new wasm.WebPlayer(JSON.stringify(morph));unhinted.control('[]','goto_and_stop',10);
  assert.notDeepEqual(JSON.parse(unhinted.scene_json())[0].kind.commands,commands);
  unhinted.free();
});

test('WASM format 10 keeps same-layer object motion paths, easing and orientation independent',async()=>{
  await ready;
  const moving=(id,x,y,alpha=1)=>({...shape(x),id,name:id,scripts:[],transform:{...transform(x),y,alpha}});
  const curve=(startX,controlY,endX)=>({type:'path',commands:[
    {op:'move_to',x:startX,y:0},
    {op:'cubic_to',cx1:startX,cy1:controlY,cx2:endX,cy2:controlY,x:endX,y:0},
  ]});
  const doc={format_version:10,name:'Object motion tracks',width:400,height:260,fps:24,
    background:[255,255,255,255],root:'root',symbols:[{id:'root',name:'Scene',kind:'movie_clip',
    timeline:{frame_count:21,scripts:[],layers:[{id:'motion',name:'Motion',visible:true,locked:false,
      keyframes:[
        {...key(0,[moving('a',0,0),moving('b',200,0)],'','linear'),tween_kind:'motion',motion_tracks:[
          {element_id:'a',easing:'ease_in',property_eases:[['x','linear'],['y','linear']],orient_to_path:true,motion_path:curve(0,100,100)},
          {element_id:'b',easing:'ease_out',property_eases:[['x','linear'],['y','linear']],orient_to_path:false,motion_path:curve(200,-100,300)},
        ]},
        {...key(20,[moving('a',100,0,0),moving('b',300,0,0)],'','linear'),tween_kind:'motion',
          keyed_properties:['x','y','alpha'],motion_keys:[
            {element_id:'a',properties:['x','y','alpha']},
            {element_id:'b',properties:['x','y','alpha']},
          ]},
      ]}]}}]};
  const player=new wasm.WebPlayer(JSON.stringify(doc));
  player.control('[]','goto_and_stop',10);
  const scene=JSON.parse(player.scene_json());
  const a=scene.find(item=>item.element_id==='a');
  const b=scene.find(item=>item.element_id==='b');
  assert.ok(a&&b,'both same-layer objects remain addressable by stable ID');
  assert.ok(Math.abs(a.transform[4]-50)<0.05&&Math.abs(a.transform[5]-75)<0.05,`a path: ${a.transform}`);
  assert.ok(Math.abs(b.transform[4]-250)<0.05&&Math.abs(b.transform[5]+75)<0.05,`b path: ${b.transform}`);
  assert.ok(Math.abs(a.kind.fill[3]/255-0.75)<0.01,`a EaseIn alpha: ${a.kind.fill[3]}`);
  assert.ok(Math.abs(b.kind.fill[3]/255-0.25)<0.01,`b EaseOut alpha: ${b.kind.fill[3]}`);
  assert.ok(Math.abs(a.transform[0]-1)>0.2||Math.abs(a.transform[1])>0.2,'a follows its path orientation');
  assert.ok(Math.abs(b.transform[0]-1)<1e-9&&Math.abs(b.transform[1])<1e-9,'b keeps authored orientation');
  player.free();
  const wrongVersion=structuredClone(doc);wrongVersion.format_version=9;
  assert.throws(()=>new wasm.WebPlayer(JSON.stringify(wrongVersion)),/version 10/);
});

test('WASM and web/video.js drive Embedded, External and Progressive video deployment modes with diagnostics',async()=>{
  await ready;
  const videoEl=id=>({id,name:id,transform:transform(0),kind:{type:'video',width:320,height:180},scripts:[]});
  const mp4Base64=Buffer.from('\0\0\0\x18ftypisom\0\0\x02\0isomiso2avc1mp41','binary').toString('base64');
  const doc={
    format_version:7,
    name:'Three video deployment modes',
    width:320,
    height:180,
    fps:10,
    background:[0,0,0,255],
    root:'root',
    resources:[
      {id:'emb_vid',path:'media/embedded.mp4',media_type:'video/mp4',data_base64:mp4Base64},
      {id:'ext_vid',path:'assets/video/cutscene.mp4',media_type:'video/mp4',data_base64:''},
      {id:'prog_vid',path:'https://cdn.example.com/streams/live.webm',media_type:'video/webm',data_base64:''},
    ],
    symbols:[{
      id:'root',
      name:'Scene',
      kind:'movie_clip',
      timeline:{
        frame_count:4,
        scripts:[],
        layers:[
          {id:'l-emb',name:'Embedded',visible:true,locked:false,keyframes:[
            {...key(0,[videoEl('v_emb')]),video:{resource:'emb_vid',sync:'stream',deployment:'embedded'}},
          ]},
          {id:'l-ext',name:'External',visible:true,locked:false,keyframes:[
            {...key(0,[videoEl('v_ext')]),video:{resource:'ext_vid',sync:'stream',deployment:'external',external_path:'assets/video/cutscene.mp4'}},
          ]},
          {id:'l-prog',name:'Progressive',visible:true,locked:false,keyframes:[
            {...key(0,[videoEl('v_prog')]),video:{resource:'prog_vid',sync:'event',deployment:'progressive',stream_url:'https://cdn.example.com/streams/live.webm'}},
          ]},
        ],
      },
    }],
  };

  const player=new wasm.WebPlayer(JSON.stringify(doc));
  const manifest=JSON.parse(player.video_manifest_json());
  assert.equal(manifest.length,3);
  const byId=Object.fromEntries(manifest.map(m=>[m.resource_id,m]));
  assert.equal(byId.emb_vid.source.mode,'embedded');
  assert.equal(byId.emb_vid.embedded_in_bundle,true);
  assert.equal(byId.ext_vid.source.mode,'external');
  assert.equal(byId.ext_vid.requires_external_file,true);
  assert.equal(byId.ext_vid.source.url_or_path,'assets/video/cutscene.mp4');
  assert.equal(byId.prog_vid.source.mode,'progressive');
  assert.equal(byId.prog_vid.requires_network_cors,true);
  assert.equal(byId.prog_vid.source.url_or_path,'https://cdn.example.com/streams/live.webm');

  const commands=JSON.parse(player.video_commands_json());
  assert.equal(commands.length,3);

  const createdMedia=[];
  function mockVideoElement(){
    const listeners=new Map();
    const el={
      src:'',
      muted:false,
      volume:1,
      crossOrigin:null,
      preload:'none',
      playsInline:false,
      loop:false,
      currentTime:0,
      duration:5,
      videoWidth:320,
      videoHeight:180,
      readyState:4,
      paused:true,
      error:null,
      addEventListener(type,fn){if(!listeners.has(type))listeners.set(type,[]);listeners.get(type).push(fn);},
      removeEventListener(type,fn){const arr=listeners.get(type)||[];const idx=arr.indexOf(fn);if(idx>=0)arr.splice(idx,1);},
      removeAttribute(attr){if(attr==='src')this.src='';},
      load(){
        for(const fn of (listeners.get('loadedmetadata')||[]))fn();
        for(const fn of (listeners.get('loadeddata')||[]))fn();
      },
      play(){this.paused=false;for(const fn of (listeners.get('playing')||[]))fn();return Promise.resolve();},
      pause(){this.paused=true;for(const fn of (listeners.get('pause')||[]))fn();},
      emitError(code){this.error={code};for(const fn of (listeners.get('error')||[]))fn();},
    };
    createdMedia.push(el);
    return el;
  }

  const refObjects=new Map();
  const proxyForRef=ref=>{
    const keyStr=JSON.stringify(ref);
    if(!refObjects.has(keyStr))refObjects.set(keyStr,{key:keyStr,objectRef:ref});
    return refObjects.get(keyStr);
  };
  const referenceOf=target=>target&&target.key?target:null;
  const diags=[];
  const videoHost=require('../video.js').create({
    player,
    events:require('../events.js'),
    net:{
      mediaSource:url=>({
        kind:'resource',
        mediaType:'video/mp4',
        bytes:Buffer.from(mp4Base64,'base64'),
        length:32,
      }),
    },
    referenceOf,
    proxyForRef,
    baseUrl:'https://publish.example.com/bundle/index.html',
    createVideo:mockVideoElement,
    createObjectURL:()=>'blob:neoflash-embedded-video',
    revokeObjectURL:()=>{},
    isUserGesture:()=>true,
    onDiagnostic:d=>diags.push(d),
  });

  videoHost.timeline(commands);
  const activeSources=videoHost.timelineSources();
  assert.equal(activeSources.length,3);
  const srcByRes=Object.fromEntries(activeSources.map(s=>[s.resource,s]));
  assert.equal(srcByRes.emb_vid.mode,'embedded');
  assert.equal(srcByRes.emb_vid.url,'media/embedded.mp4');
  assert.equal(srcByRes.emb_vid.mediaSrc,'blob:neoflash-embedded-video');
  assert.equal(srcByRes.ext_vid.mode,'external');
  assert.equal(srcByRes.ext_vid.url,'https://publish.example.com/bundle/assets/video/cutscene.mp4');
  assert.equal(srcByRes.ext_vid.mediaSrc,'https://publish.example.com/bundle/assets/video/cutscene.mp4');
  assert.equal(srcByRes.prog_vid.mode,'progressive');
  assert.equal(srcByRes.prog_vid.url,'https://cdn.example.com/streams/live.webm');
  assert.equal(srcByRes.prog_vid.mediaSrc,'https://cdn.example.com/streams/live.webm');
  assert.equal(srcByRes.prog_vid.crossOrigin,'anonymous');
  assert.equal(srcByRes.prog_vid.preload,'auto');

  // Simulate missing external file and CORS/HTTP failure on progressive stream without crashing playback.
  const extMedia=createdMedia.find(m=>m.src.includes('cutscene.mp4'));
  const progMedia=createdMedia.find(m=>m.src.includes('live.webm'));
  extMedia.emitError(2);
  progMedia.emitError(4);
  assert.equal(diags.length,2);
  assert.equal(diags[0].code,'video-external-missing-or-http-error');
  assert.equal(diags[1].code,'video-progressive-codec-error');
  assert.equal(videoHost.diagnostics().length,2);

  videoHost.dispose();
  player.free();
});
