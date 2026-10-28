// Shared test plumbing for the actual Rust WASM scripting services.
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';
const root = fileURLToPath(new URL('../../..', import.meta.url));
const context = vm.createContext({console, TextEncoder, TextDecoder, WebAssembly, URL});
vm.runInContext(readFileSync(resolve(root, 'target/web-runtime/neoflash_web.js'), 'utf8'), context);
context.wasmBytes = readFileSync(resolve(root, 'target/web-runtime/neoflash_web_bg.wasm'));
export const runtimeWasm = await vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)', context);
if (typeof runtimeWasm.as2_string_json !== 'function') throw new Error('Runtime tests require a fresh Rust WASM build: scripts/build-web.sh');
const bridge = createRequire(import.meta.url)(resolve(root, 'web/as2-strings.js'));
export function createRustStrings() { return bridge.create({execute: runtimeWasm.as2_string_json}); }

const require = createRequire(import.meta.url);
export function createStringServices() {
  return {strings:createRustStrings(),strings3:require(resolve(root,'web/as3-strings.js')).create({execute:runtimeWasm.as3_string_json}),math:require(resolve(root,'web/math.js')).create(runtimeWasm)};
}
export function createGeometryFamily() {
  let two;
  const three=require(resolve(root,'web/geom3d.js')).create(runtimeWasm,{getPoint:()=>two.Point});
  two=require(resolve(root,'web/geom.js')).create(runtimeWasm,{getVector3D:()=>three.Vector3D});
  return two;
}
