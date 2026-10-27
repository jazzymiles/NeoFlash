// Shared test plumbing: execute the actual exported Rust WASM String service.
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';
const root = fileURLToPath(new URL('../../..', import.meta.url));
const context = vm.createContext({console, TextEncoder, TextDecoder, WebAssembly, URL});
vm.runInContext(readFileSync(resolve(root, 'target/web-runtime/neoflash_web.js'), 'utf8'), context);
context.wasmBytes = readFileSync(resolve(root, 'target/web-runtime/neoflash_web_bg.wasm'));
export const as2StringsWasm = await vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)', context);
if (typeof as2StringsWasm.as2_string_json !== 'function') throw new Error('AS2 String tests require a fresh Rust WASM build: scripts/build-web.sh');
const bridge = createRequire(import.meta.url)(resolve(root, 'web/as2-strings.js'));
export function createRustStrings() { return bridge.create({execute: as2StringsWasm.as2_string_json}); }
