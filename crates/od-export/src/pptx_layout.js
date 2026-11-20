// Measures the slide the renderer pinned at the top-left of the viewport
// (`[data-od-capture-active]`, else the first slide, else <body>) into the
// SlideLayout JSON of `pptx_editable.rs`: shapes for painted boxes, text
// boxes with styled runs, pictures re-encoded as PNG data URLs.
(async () => {
  const slide = document.querySelector('[data-od-capture-active]')
    || document.querySelector('.slide, [data-screen-label], .deck-slide, .ppt-slide')
    || document.body;
  const origin = slide.getBoundingClientRect();
  const out = { width: origin.width, height: origin.height, background: null, elements: [] };
  const box = (r) => ({ x: r.left - origin.left, y: r.top - origin.top, w: r.width, h: r.height });
  const visible = (cs) => cs.display !== 'none' && cs.visibility !== 'hidden' && parseFloat(cs.opacity || '1') > 0.01;
  // Computed colours keep their syntax (oklch(), lab(), color(srgb …),
  // color-mix() …), and `parse_color` reads only #hex, rgb() and rgba():
  // the canvas turns any colour Chrome knows into sRGB bytes (QA-121).
  // rgb()/rgba() pass as they are, exact. `null` = no colour or invisible.
  const probe = document.createElement('canvas').getContext('2d', { willReadFrequently: true });
  const rgb = (c) => {
    if (!c || c === 'transparent') return null;
    if (/^rgba?\(/.test(c)) return /^rgba\([^)]*,\s*0\)$/.test(c) ? null : c;
    probe.clearRect(0, 0, 1, 1);
    probe.fillStyle = 'rgba(0, 0, 0, 0)';
    probe.fillStyle = c;
    probe.fillRect(0, 0, 1, 1);
    const [r, g, b, a] = probe.getImageData(0, 0, 1, 1).data;
    if (a === 0) return null;
    return a === 255 ? `rgb(${r}, ${g}, ${b})` : `rgba(${r}, ${g}, ${b}, ${Math.round(a / 255 * 1000) / 1000})`;
  };
  const BLOCK = /^(block|flex|grid|table|list-item|flow-root|table-cell|table-caption|inline-block|inline-flex|inline-grid)$/;

  // The first box from the slide up that paints an opaque background:
  // decks often colour a stage between the slide and a body painted as the
  // dark shell around it. A box's own colour wins over its gradients, which
  // on a coloured box are mostly texture (ruled lines, soft spots); a box
  // painted only by a gradient gives its first opaque stop, as it covers
  // every ancestor (html-ppt-graphify-dark-graph). A gradient with no
  // opaque stop lets the ancestors show through.
  const COLOR = /(?:rgba?|hsla?|hwb|lab|lch|oklab|oklch|color|color-mix)\((?:[^()]|\([^()]*\))*\)|#[0-9a-fA-F]{3,8}\b/g;
  const paint = (cs) => {
    const own = rgb(cs.backgroundColor);
    if (own) return own;
    const image = cs.backgroundImage || '';
    if (!/gradient\(/.test(image)) return null;
    for (const [c] of image.matchAll(COLOR)) {
      const stop = rgb(c);
      if (stop && stop.startsWith('rgb(')) return stop;
    }
    return null;
  };
  let bg = null;
  for (let a = slide; a && !bg; a = a.parentElement) bg = paint(getComputedStyle(a));
  out.background = bg || '#ffffff';

  const toPng = async (el, r) => {
    const w = Math.max(1, Math.round(r.width * 2)), h = Math.max(1, Math.round(r.height * 2));
    const canvas = document.createElement('canvas');
    canvas.width = w; canvas.height = h;
    const ctx = canvas.getContext('2d');
    try {
      if (el instanceof HTMLCanvasElement) {
        ctx.drawImage(el, 0, 0, w, h);
      } else {
        let src;
        if (el instanceof SVGSVGElement) {
          const clone = el.cloneNode(true);
          clone.setAttribute('xmlns', 'http://www.w3.org/2000/svg');
          clone.setAttribute('width', r.width); clone.setAttribute('height', r.height);
          src = 'data:image/svg+xml;charset=utf-8,' + encodeURIComponent(new XMLSerializer().serializeToString(clone));
        } else {
          src = el.currentSrc || el.src;
          if (/^data:image\/(png|jpeg|gif);base64,/.test(src)) return src;
        }
        const img = new Image();
        await new Promise((ok, fail) => { img.onload = ok; img.onerror = fail; img.src = src; });
        ctx.drawImage(img, 0, 0, w, h);
      }
      return canvas.toDataURL('image/png');
    } catch (e) {
      return null;
    }
  };

  const runStyle = (el) => {
    const cs = getComputedStyle(el);
    const weight = parseInt(cs.fontWeight, 10) || (cs.fontWeight === 'bold' ? 700 : 400);
    return {
      size: parseFloat(cs.fontSize) || 16,
      color: rgb(cs.color),
      bold: weight >= 600,
      italic: cs.fontStyle === 'italic' || cs.fontStyle === 'oblique',
      underline: (cs.textDecorationLine || cs.textDecoration || '').includes('underline'),
      font: (cs.fontFamily || '').split(',')[0].trim().replace(/^["']|["']$/g, '') || null,
      transform: cs.textTransform,
      pre: /^(pre|pre-wrap|pre-line|break-spaces)$/.test(cs.whiteSpace),
    };
  };

  // Inline text under `block`, stopping at nested blocks (they get their
  // own text box). `<br>` becomes a newline.
  const collectRuns = (block) => {
    const runs = [];
    const walk = (node) => {
      for (const child of node.childNodes) {
        if (child.nodeType === Node.TEXT_NODE) {
          const parent = child.parentElement;
          const st = runStyle(parent);
          let text = child.textContent;
          if (!st.pre) text = text.replace(/\s+/g, ' ');
          if (st.transform === 'uppercase') text = text.toUpperCase();
          else if (st.transform === 'lowercase') text = text.toLowerCase();
          if (text) runs.push({ text, size: st.size, color: st.color, bold: st.bold, italic: st.italic, underline: st.underline, font: st.font });
        } else if (child.nodeType === Node.ELEMENT_NODE) {
          if (child.tagName === 'BR') { runs.push({ text: '\n', size: runStyle(node).size }); continue; }
          const cs = getComputedStyle(child);
          if (!visible(cs) || BLOCK.test(cs.display) || /^(IMG|SVG|CANVAS|svg|VIDEO|SCRIPT|STYLE)$/.test(child.tagName)) continue;
          walk(child);
        }
      }
    };
    walk(block);
    // Trim the outer whitespace of the box.
    while (runs.length && !runs[0].text.trim()) runs.shift();
    while (runs.length && !runs[runs.length - 1].text.trim()) runs.pop();
    if (runs.length) {
      runs[0].text = runs[0].text.replace(/^\s+/, '');
      runs[runs.length - 1].text = runs[runs.length - 1].text.replace(/\s+$/, '');
    }
    return runs;
  };

  const visit = async (el) => {
    const cs = getComputedStyle(el);
    if (!visible(cs) || /^(SCRIPT|STYLE|TEMPLATE|NOSCRIPT)$/.test(el.tagName)) return;
    if (el.matches('aside.notes, .notes, .speaker-notes')) return;
    const r = el.getBoundingClientRect();
    if (r.width < 0.5 || r.height < 0.5) return;
    const tag = el.tagName;
    if (tag === 'IMG' || tag === 'CANVAS' || el instanceof SVGSVGElement) {
      const src = await toPng(el, r);
      if (src) out.elements.push({ kind: 'image', ...box(r), src });
      return;
    }
    if (el instanceof SVGElement) return;
    if (el !== slide) {
      const fill = rgb(cs.backgroundColor);
      const bw = parseFloat(cs.borderTopWidth) || 0;
      const line = bw > 0 && cs.borderTopStyle !== 'none' ? rgb(cs.borderTopColor) : null;
      const url = /url\(["']?(data:image\/(png|jpeg|gif);base64,[^"')]+)["']?\)/.exec(cs.backgroundImage || '');
      if (fill || line) {
        out.elements.push({ kind: 'shape', ...box(r), fill, line, lineWidth: line ? bw : 0, radius: parseFloat(cs.borderTopLeftRadius) || 0 });
      }
      if (url) out.elements.push({ kind: 'image', ...box(r), src: url[1] });
    }
    const isBlock = BLOCK.test(cs.display) || el === slide;
    const runs = isBlock ? collectRuns(el) : [];
    if (runs.length) {
      {
        const pl = parseFloat(cs.paddingLeft) || 0, pr = parseFloat(cs.paddingRight) || 0;
        const pt = parseFloat(cs.paddingTop) || 0, pb = parseFloat(cs.paddingBottom) || 0;
        const bl = parseFloat(cs.borderLeftWidth) || 0, bt = parseFloat(cs.borderTopWidth) || 0;
        const inner = { left: r.left + pl + bl, top: r.top + pt + bt, width: Math.max(1, r.width - pl - pr - 2 * bl), height: Math.max(1, r.height - pt - pb - 2 * bt) };
        // Fonts render a little wider in PowerPoint: give the box 3 % slack.
        inner.width *= 1.03;
        const lh = parseFloat(cs.lineHeight);
        const size = parseFloat(cs.fontSize) || 16;
        const align = { start: 'left', left: 'left', center: 'center', right: 'right', end: 'right', justify: 'justify' }[cs.textAlign] || 'left';
        const valign = cs.display.includes('flex') && cs.alignItems === 'center' ? 'middle' : 'top';
        out.elements.push({ kind: 'text', ...box(inner), valign, paragraphs: [{ align, lineHeight: lh ? lh / size : null, runs }] });
      }
    }
    for (const child of el.children) {
      const ccs = getComputedStyle(child);
      // Inline children were already folded into this block's runs.
      if (runs.length && !BLOCK.test(ccs.display) && !/^(IMG|CANVAS)$/.test(child.tagName) && !(child instanceof SVGSVGElement)) continue;
      await visit(child);
    }
  };
  await visit(slide);
  return out;
})()
