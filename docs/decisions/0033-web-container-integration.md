# Web container calls, navigation and request dispatch

Status: accepted for the bounded HTML5 profile, 2026-10-27.

## A browser publication is part of a web page

ExternalInterface connects a publication's ActionScript to its containing HTML/JavaScript environment. The browser host provides an actual element identity and synchronous callbacks; Rust remains the document/compiler/runtime owner. The bridge does not emulate ActiveX or a second Flash plugin. AS2 and AS3 share value transport and lifetime ownership but retain their different addCallback signatures and return contracts.

Named JavaScript functions must retain their owning receiver. Calls do not evaluate source expressions or traverse prototype machinery. Callback installation must refuse collisions with the host element's existing API, support replacement/removal where specified and invalidate saved callbacks on Restart/dispose. Argument/result copies need bounded structure and call depth; cyclic, unsupported native or executable values must not accidentally pass as empty records. Exception propagation follows the declared dialect/profile, including AS3 marshallExceptions.

## One network policy, distinct operations

navigateToURL, AS2 getURL and LoadVars.send request browser navigation, while sendToURL sends a request and ignores its response. Reuse URLRequest/body/header validation and the existing HTTP(S) transport. Do not accidentally turn an embedded resource lookup into a network request or report a discarded response as URLLoader completion. Browser navigation uses native contexts and forms; popup creation, cross-origin navigation and user activation remain subject to the browser.

GET and URL-encoded form POST can preserve useful Flash web workflows without introducing a desktop HTTP path. Unsupported URL schemes, arbitrary navigation headers/body types, browser-reserved fields and Flash security-domain behavior remain explicit boundaries. Network requests and callback registrations belong to the publication generation; Restart aborts or invalidates pending work without claiming to undo a request already received by a server.

The bridge bounds each copy to 16,384 values, 64 nested levels and 2 Mi UTF-16 string units; at most 256 callbacks and 32 synchronous crossings are retained. AS2 recursive crossings are explicitly unsupported. Public source-class data can flatten through existing program metadata; ordinary data records cannot execute accessor properties during marshalling. AS3 exception propagation is opt-in through marshallExceptions, while suppressed exceptions retain a diagnostic.

## Verification scope

One self-contained example uses an authored JavaScript bootstrap to create HTML host controls and functions, plus AS2/AS3 callbacks. This is ordinary project code, not a patched exported runtime. One local HTTP receipt endpoint records real GET/POST navigation and fire-and-forget sends. The grouped host/WASM workflow verifies transport and lifetime; actual browser controls verify page callbacks and native navigation. Follow the collect-all family runner and rerun only concrete failures.

References: Adobe/AIR [ExternalInterface](https://airsdk.dev/reference/actionscript/3.0/flash/external/ExternalInterface.html), [flash.net functions](https://airsdk.dev/reference/actionscript/3.0/flash/net/package.html), and Adobe [AS2 ExternalInterface reference mirror](https://open-flash.github.io/mirrors/as2-language-reference/flash/external/ExternalInterface.html). These define the compatibility target; implementation and observed browser behavior retain separate evidence.
