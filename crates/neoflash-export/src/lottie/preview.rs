//! Standalone interactive HTML preview generator (`<stem>.preview.html`) for Lottie exports.

use super::LOTTIE_VERSION;

fn escape_html_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            '\u{2028}' => out.push_str("&#x2028;"),
            '\u{2029}' => out.push_str("&#x2029;"),
            _ => out.push(ch),
        }
    }
    out
}

fn escape_json_for_html_script(json: &str) -> String {
    let mut out = String::with_capacity(json.len() + 16);
    for ch in json.chars() {
        match ch {
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '&' => out.push_str("\\u0026"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            _ => out.push(ch),
        }
    }
    out
}

pub(super) fn build_lottie_html_preview(
    title: &str,
    width: u32,
    height: u32,
    lottie_json: &str,
) -> String {
    let safe_title = escape_html_text(title);
    let safe_json = escape_json_for_html_script(lottie_json);
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>{safe_title} — Lottie Preview</title>
<style>
  body {{ margin: 0; background: #14171f; color: #e6e9f0; font-family: system-ui, sans-serif; display: flex; flex-direction: column; align-items: center; justify-content: center; min-height: 100vh; }}
  #player {{ width: {width}px; height: {height}px; max-width: 92vw; max-height: 80vh; background: #1e222d; border-radius: 8px; box-shadow: 0 8px 28px rgba(0,0,0,0.45); overflow: hidden; }}
  .meta {{ margin-top: 12px; font-size: 13px; color: #9aa2b2; }}
</style>
<script src="https://cdnjs.cloudflare.com/ajax/libs/lottie-web/5.12.2/lottie.min.js" async></script>
</head>
<body>
  <div id="player"></div>
  <div class="meta">{safe_title} · {width} × {height} · Bodymovin v{LOTTIE_VERSION}</div>
  <script>
    (function() {{
      const rootEl = document.documentElement;
      const container = document.getElementById('player');
      const animationData = {safe_json};
      window.__lottieAnimationData = animationData;

      function mountPlayer() {{
        if (window.lottie && !container.dataset.mounted) {{
          container.dataset.mounted = 'lottie-web';
          container.textContent = '';
          window.lottie.loadAnimation({{
            container: container,
            renderer: 'svg',
            loop: true,
            autoplay: true,
            animationData: animationData
          }});
          return true;
        }}
        return false;
      }}

      if (!mountPlayer()) {{
        const ns = 'http://www.w3.org/2000/svg';
        const svg = document.createElementNS(ns, 'svg');
        svg.setAttribute('viewBox', '0 0 ' + (animationData.w || {width}) + ' ' + (animationData.h || {height}));
        svg.setAttribute('width', '100%');
        svg.setAttribute('height', '100%');
        const layers = Array.isArray(animationData.layers) ? animationData.layers : [];
        for (let i = layers.length - 1; i >= 0; i--) {{
          const layer = layers[i];
          if (layer && layer.ty === 1 && typeof layer.sc === 'string') {{
            const rect = document.createElementNS(ns, 'rect');
            rect.setAttribute('width', String(layer.sw || animationData.w || {width}));
            rect.setAttribute('height', String(layer.sh || animationData.h || {height}));
            rect.setAttribute('fill', layer.sc);
            svg.appendChild(rect);
          }}
        }}
        container.appendChild(svg);
        window.addEventListener('load', mountPlayer);
      }}

      const assets = Array.isArray(animationData.assets) ? animationData.assets : [];
      const assetMap = new Map();
      const imageAssets = [];
      for (const asset of assets) {{
        if (asset && typeof asset.id === 'string') {{
          assetMap.set(asset.id, asset);
          if (typeof asset.p === 'string' && asset.p.length > 0) {{
            imageAssets.push(asset);
          }}
        }}
      }}

      const dangling = [];
      function checkLayerRefs(list) {{
        if (!Array.isArray(list)) return;
        for (const layer of list) {{
          if (layer && typeof layer.refId === 'string' && !assetMap.has(layer.refId)) {{
            dangling.push(layer.refId);
          }}
        }}
      }}
      checkLayerRefs(animationData.layers);
      for (const asset of assets) {{
        if (asset && Array.isArray(asset.layers)) {{
          checkLayerRefs(asset.layers);
        }}
      }}

      rootEl.setAttribute('data-lottie-xss', String(Boolean(window.__xss || window.__t051_marker)));
      rootEl.setAttribute('data-lottie-dangling-refs', String(dangling.length));
      rootEl.setAttribute('data-lottie-images-expected', String(imageAssets.length));

      if (imageAssets.length === 0) {{
        rootEl.setAttribute('data-lottie-images-loaded', '0');
        rootEl.setAttribute('data-lottie-images-failed', '0');
        rootEl.setAttribute('data-lottie-images-verified', String(dangling.length === 0));
        rootEl.setAttribute('data-lottie-ready', 'true');
      }} else {{
        let loaded = 0;
        let failed = 0;
        const finishOne = () => {{
          if (loaded + failed === imageAssets.length) {{
            rootEl.setAttribute('data-lottie-xss', String(Boolean(window.__xss || window.__t051_marker)));
            rootEl.setAttribute('data-lottie-images-loaded', String(loaded));
            rootEl.setAttribute('data-lottie-images-failed', String(failed));
            rootEl.setAttribute(
              'data-lottie-images-verified',
              String(dangling.length === 0 && failed === 0 && loaded === imageAssets.length)
            );
            rootEl.setAttribute('data-lottie-ready', 'true');
          }}
        }};
        for (const imgAsset of imageAssets) {{
          const probe = new Image();
          probe.onload = () => {{ loaded += 1; finishOne(); }};
          probe.onerror = () => {{ failed += 1; finishOne(); }};
          probe.src = imgAsset.e === 1 ? imgAsset.p : ((imgAsset.u || '') + imgAsset.p);
        }}
      }}
    }})();
  </script>
</body>
</html>
"#
    )
}
