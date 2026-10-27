/* Bounded metadata inspection before Web Audio allocates decoded PCM.
 * This is not an audio decoder. The browser still validates compressed data.
 * MP3 header layout: https://github.com/lieff/minimp3 (CC0 reference).
 * ID3 size: https://id3.org/id3v2.4.0-structure
 * WAV layout: https://learn.microsoft.com/en-us/windows/win32/api/mmeapi/ns-mmeapi-waveformatex
 */
(function(global){
  'use strict';
  function fail(message){throw new TypeError(`Sound audio format: ${message} Use standard MP3 or mono/stereo PCM/float WAV.`);}
  function tag(bytes,offset,text){return offset+text.length<=bytes.length&&[...text].every((c,i)=>bytes[offset+i]===c.charCodeAt(0));}
  function result(format,samples,sampleRate,channels,targetSampleRate){
    if(!Number.isSafeInteger(samples)||samples<1)fail('The file has no complete audio samples.');
    // Include two MPEG-1 frames of decoder/resampler padding, even for WAV.
    // Never trust VBR duration metadata to reduce the full frame-count budget.
    const decodedBytes=(Math.ceil((samples+2304)*targetSampleRate/sampleRate)+32)*channels*4;
    if(!Number.isSafeInteger(decodedBytes))fail('The decoded sample count is too large.');
    return Object.freeze({format,samples,sampleRate,channels,duration:samples/sampleRate,decodedBytes});
  }
  function mp3(bytes,targetSampleRate){
    let offset=0,end=bytes.length;
    if(tag(bytes,0,'ID3')){
      if(end<10||bytes[3]<2||bytes[3]>4||bytes[4]===255)fail('Unsupported or truncated ID3 header.');
      let size=0;for(let i=6;i<10;i++){if(bytes[i]&128)fail('Invalid ID3 size.');size=size*128+bytes[i];}
      offset=10+size+(bytes[3]===4&&(bytes[5]&16)?10:0);
      if(offset>end)fail('Truncated ID3 tag.');
    }
    if(end-offset>=128&&tag(bytes,end-128,'TAG'))end-=128;
    const rates=[44100,48000,32000],mpeg1=[0,32,40,48,56,64,80,96,112,128,160,192,224,256,320],mpeg2=[0,8,16,24,32,40,48,56,64,80,96,112,128,144,160];
    let sampleRate=0,channels=0,samples=0,version=-1;
    while(offset<end){
      // Some encoders leave a small zero-filled tail after their last frame.
      if(bytes[offset]===0&&end-offset<=4096){let zero=true;for(let i=offset;i<end;i++)if(bytes[i]!==0){zero=false;break;}if(zero)break;}
      if(offset+4>end||bytes[offset]!==255||(bytes[offset+1]&224)!==224)fail('Invalid or truncated MP3 frame sequence.');
      const b1=bytes[offset+1],b2=bytes[offset+2],v=(b1>>3)&3,layer=(b1>>1)&3,rateIndex=(b2>>2)&3,bitrateIndex=b2>>4;
      if(v===1||layer!==1||rateIndex===3||bitrateIndex===0||bitrateIndex===15)fail('Only standard-rate MPEG-1/2/2.5 Layer III frames are supported (no free-format MP3).');
      const rate=rates[rateIndex]/(v===3?1:v===2?2:4),count=(bytes[offset+3]>>6)===3?1:2;
      if(sampleRate&&(rate!==sampleRate||count!==channels||v!==version))fail('Changing MP3 sample rates or channel layouts are unsupported.');
      sampleRate=rate;channels=count;version=v;
      const frameBytes=Math.floor((v===3?144000:72000)*(v===3?mpeg1:mpeg2)[bitrateIndex]/rate)+((b2>>1)&1);
      if(frameBytes<4||offset+frameBytes>end)fail('Truncated MP3 frame.');
      samples+=v===3?1152:576;offset+=frameBytes;
    }
    return result('mp3',samples,sampleRate,channels,targetSampleRate);
  }
  function wav(bytes,targetSampleRate){
    if(bytes.length<12||!tag(bytes,8,'WAVE'))fail('Invalid RIFF/WAVE header.');
    const view=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength),end=view.getUint32(4,true)+8;
    if(end>bytes.length||end<12)fail('Truncated RIFF/WAVE container.');
    let format=null,dataBytes=null,offset=12;
    while(offset<end){
      if(offset+8>end)fail('Truncated WAV chunk header.');
      const size=view.getUint32(offset+4,true),start=offset+8,next=start+size+(size&1);
      if(next>end)fail('Truncated WAV chunk.');
      if(tag(bytes,offset,'fmt ')){
        if(format||size<16)fail('Invalid or repeated WAV format chunk.');
        const kind=view.getUint16(start,true),channels=view.getUint16(start+2,true),rate=view.getUint32(start+4,true),byteRate=view.getUint32(start+8,true),align=view.getUint16(start+12,true),bits=view.getUint16(start+14,true);
        if((kind!==1&&kind!==3)||(channels!==1&&channels!==2)||rate<8000||rate>192000||(kind===1?![8,16,24,32].includes(bits):bits!==32))fail('Unsupported WAV encoding, sample rate or channel layout.');
        if(align!==channels*bits/8||byteRate!==rate*align)fail('Inconsistent WAV sample sizes.');
        format={rate,channels,align};
      }else if(tag(bytes,offset,'data')){
        if(dataBytes!==null)fail('Repeated WAV data chunks are unsupported.');
        dataBytes=size;
      }
      offset=next;
    }
    if(!format||dataBytes===null||dataBytes%format.align)fail('Missing or incomplete WAV samples.');
    return result('wav',dataBytes/format.align,format.rate,format.channels,targetSampleRate);
  }
  function inspect(bytes,targetSampleRate){
    if(!ArrayBuffer.isView(bytes)||bytes.BYTES_PER_ELEMENT!==1)throw new TypeError('Sound inspection requires a byte array.');
    if(bytes.byteLength<4||bytes.byteLength>16*1024*1024)throw new RangeError('Sound input must contain 4 bytes through 16 MiB.');
    if(!Number.isInteger(targetSampleRate)||targetSampleRate<8000||targetSampleRate>192000)throw new RangeError('Sound output sample rate must be from 8000 through 192000 Hz.');
    return tag(bytes,0,'RIFF')?wav(bytes,targetSampleRate):mp3(bytes,targetSampleRate);
  }
  const api=Object.freeze({inspect});if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashAudioFormats=api;
})(globalThis);
